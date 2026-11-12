//! Public translation catalog and direct conditional WebVTT transport.
//!
//! Requests build an authorized descriptor in memory and revalidate or translate
//! a complete document. Only the browser retains the `ETag` and bytes pair.

use std::collections::HashMap;
use std::net::SocketAddr;

use arachnea_core::controler::{ControlerStreamInput, ControlerStreamOutput, ResponseBody};
use arachnea_scrapyfy::scrapyfy::conditional::hash62;
use arachnea_translation::{
    AccessScope, LanguageCode, LocalTranslationLimits, SelectionPolicy, SupportedPair,
};
use serde::{Deserialize, Serialize};

use super::StreamScraper;
use crate::subtitles::{
    service_supports_pair, LocalSubtitleLimits, SubtitleFormat, SubtitleSourceDescriptor,
    SubtitleTranslationDescriptor, SubtitleTranslationError, SubtitleTranslationOutcome,
    SubtitleTranslationRequest, SubtitleTranslationValidator,
};
use crate::translation_catalog::TranslationCatalogEntry;
use crate::DEFAULT_SUBTITLE_CACHE_CONTROL;

/// Maximum number of `If-None-Match` candidates evaluated per request.
const IF_NONE_MATCH_MAX_TAGS: usize = 32;

/// HEAD and other unsupported methods never retrieve or translate.
const SUBTITLE_FILE_ALLOWED_METHODS: &str = "GET";

/// Empty request payload for the public translation catalog command.
#[derive(Debug, Deserialize)]
pub(crate) struct GetTranslationServicesRequest {}

/// Parameters of one direct conditional subtitle translation request.
#[derive(Debug, Deserialize)]
pub(crate) struct TranslateSubtitleRequest {
    /// Source subtitle URL or recognized application proxy link.
    #[serde(alias = "link")]
    pub(crate) url: String,
    /// Stable source track identifier when the caller already knows one.
    #[serde(default)]
    pub(crate) track_id: Option<String>,
    /// Known source language, or omitted for automatic detection.
    #[serde(default, alias = "lang")]
    pub(crate) source_language: Option<String>,
    /// Requested target language.
    pub(crate) target_language: String,
    /// Automatic, preferred or constrained service policy.
    #[serde(default)]
    pub(crate) policy: Option<SelectionPolicy>,
    /// Source subtitle format; only WebVTT is accepted initially.
    #[serde(default)]
    pub(crate) format: Option<String>,
}

/// Public capability of one published translation service.
#[derive(Debug, Serialize)]
struct TranslationServiceView {
    service_id: String,
    title: String,
    languages: Vec<String>,
    source_languages: Vec<String>,
    target_languages: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    supported_pairs: Option<Vec<TranslationPairView>>,
    supports_text_translation: bool,
    supports_url_translation: bool,
    source_detection: bool,
    preferred: bool,
}

/// One directional public language pair.
#[derive(Debug, Serialize)]
struct TranslationPairView {
    source: String,
    target: String,
}

/// Public translation catalog response.
#[derive(Debug, Serialize)]
pub(crate) struct TranslationServicesView {
    services: Vec<TranslationServiceView>,
    languages: Vec<String>,
}

impl StreamScraper {
    /// Returns the public effective translation catalog without credentials.
    pub(crate) fn get_translation_services(
        &self,
        _request: GetTranslationServicesRequest,
    ) -> TranslationServicesView {
        let services = self
            .translation_catalog
            .services()
            .iter()
            .map(service_view)
            .collect();
        let languages = self
            .translation_catalog
            .available_languages()
            .iter()
            .map(|language| language.as_str().to_string())
            .collect();
        TranslationServicesView {
            services,
            languages,
        }
    }

    /// Validates the requested source, languages and policy into an in-memory descriptor.
    ///
    /// # Arguments
    /// * `remote_addr` - Trusted peer address used to isolate cache validators.
    /// * `request` - Source URL, languages, format and selection policy.
    ///
    /// # Errors
    /// Returns a descriptor, format or service eligibility error.
    fn translation_descriptor(
        &self,
        remote_addr: Option<SocketAddr>,
        request: TranslateSubtitleRequest,
    ) -> Result<SubtitleTranslationDescriptor, SubtitleTranslationError> {
        let source_language = request
            .source_language
            .as_deref()
            .map(|value| parse_language(value, "source language"))
            .transpose()?;
        let target_language = parse_language(&request.target_language, "target language")?;
        let format = parse_format(request.format.as_deref())?;
        let track_id = request
            .track_id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("subtitle-{}", hash62(request.url.as_bytes())));
        let source = SubtitleSourceDescriptor {
            track_id,
            url: request.url,
            declared_language: source_language.clone(),
            format,
        };
        source.validate()?;
        validate_source_url(
            &source.url,
            self.player_resolver_endpoints
                .http_proxy_public_path
                .as_deref(),
        )?;
        if source_language.as_ref() == Some(&target_language) {
            return Err(SubtitleTranslationError::InvalidDescriptor {
                detail: "source and target languages are identical".to_string(),
            });
        }
        let policy = request.policy.unwrap_or(SelectionPolicy::Auto {
            preferred_service: None,
        });
        validate_policy(
            self.translation_catalog.services(),
            &policy,
            source_language.as_ref(),
            &target_language,
        )?;
        Ok(SubtitleTranslationDescriptor {
            source,
            source_language,
            target_language,
            policy,
            access_scope: access_scope(remote_addr),
        })
    }

    /// Returns a complete translated WebVTT file or an empty conditional `304`.
    ///
    /// # Arguments
    /// * `input` - GET request with JSON-encoded query parameters and optional ETag.
    ///
    /// # Errors
    /// Protocol and translation failures are returned as explicit non-WebVTT responses.
    pub(crate) async fn get_translated_subtitle(
        &self,
        input: ControlerStreamInput,
    ) -> Result<ControlerStreamOutput, SubtitleTranslationError> {
        if !input.method.eq_ignore_ascii_case("GET") {
            return Ok(method_not_allowed());
        }
        if !input.path.trim_matches('/').is_empty() {
            return Ok(not_found());
        }
        let request = match parse_request(&input.query) {
            Ok(request) => request,
            Err(error) => return Ok(error_output(error)),
        };
        let descriptor = match self.translation_descriptor(input.remote_addr, request) {
            Ok(descriptor) => descriptor,
            Err(error) => return Ok(error_output(error)),
        };
        let access_scope = access_scope(input.remote_addr);
        let client_etag = input
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("if-none-match"))
            .and_then(|(_, value)| {
                select_client_etag(value, &descriptor, &self.translation_catalog)
            });
        let outcome = self
            .translate_subtitle(
                SubtitleTranslationRequest {
                    descriptor: &descriptor,
                    access_scope: &access_scope,
                    client_etag,
                    provider_accessible_url: None,
                    subtitle_limits: LocalSubtitleLimits::default(),
                    translation_limits: LocalTranslationLimits::default(),
                },
                HashMap::new(),
                self.player_resolver_endpoints
                    .http_proxy_public_path
                    .as_deref(),
            )
            .await;
        match outcome {
            Ok(SubtitleTranslationOutcome::NotModified { etag }) => Ok(file_response(
                304,
                Vec::new(),
                Some(etag),
                DEFAULT_SUBTITLE_CACHE_CONTROL,
            )),
            Ok(SubtitleTranslationOutcome::Translated(document)) => Ok(file_response(
                200,
                document.bytes,
                Some(document.etag),
                &document.cache_control,
            )),
            Ok(SubtitleTranslationOutcome::Original { bytes }) => Ok(file_response(
                200,
                bytes,
                None,
                DEFAULT_SUBTITLE_CACHE_CONTROL,
            )),
            Err(error) => Ok(error_output(error)),
        }
    }
}

/// Serializes one service capability without credentials or YAML internals.
fn service_view(entry: &TranslationCatalogEntry) -> TranslationServiceView {
    TranslationServiceView {
        service_id: entry.service_id.clone(),
        title: entry.title.clone(),
        languages: language_strings(&entry.languages),
        source_languages: language_strings(&entry.source_languages),
        target_languages: language_strings(&entry.target_languages),
        supported_pairs: entry
            .supported_pairs
            .as_ref()
            .map(|pairs| pairs.iter().map(pair_view).collect()),
        supports_text_translation: entry.supports_text_translation,
        supports_url_translation: entry.supports_url_translation,
        source_detection: entry.source_detection,
        preferred: entry.preferred,
    }
}

/// Converts language codes to their public wire representation.
fn language_strings(languages: &[LanguageCode]) -> Vec<String> {
    languages
        .iter()
        .map(|language| language.as_str().to_string())
        .collect()
}

/// Converts one declared pair to its public wire representation.
fn pair_view(pair: &SupportedPair) -> TranslationPairView {
    TranslationPairView {
        source: pair.source.as_str().to_string(),
        target: pair.target.as_str().to_string(),
    }
}

/// Parses and normalizes one caller-supplied language code.
fn parse_language(value: &str, field: &str) -> Result<LanguageCode, SubtitleTranslationError> {
    LanguageCode::parse(value.trim().to_ascii_lowercase()).map_err(|_| {
        SubtitleTranslationError::InvalidDescriptor {
            detail: format!("{field} must be a two-letter language code"),
        }
    })
}

/// Parses the source subtitle format, accepting only initial-delivery formats.
fn parse_format(value: Option<&str>) -> Result<SubtitleFormat, SubtitleTranslationError> {
    let media_type = value.unwrap_or("webvtt").trim().to_ascii_lowercase();
    let format = match media_type.as_str() {
        "webvtt" | "vtt" | "text/vtt" => SubtitleFormat::WebVtt,
        "srt" | "application/x-subrip" => SubtitleFormat::Srt,
        other => {
            return Err(SubtitleTranslationError::UnsupportedFormat {
                media_type: other.to_string(),
            })
        }
    };
    if !format.is_initial_delivery_supported() {
        return Err(SubtitleTranslationError::UnsupportedFormat {
            media_type: format.media_type().to_string(),
        });
    }
    Ok(format)
}

/// Validates the source location without resolving DNS or retrieving content.
///
/// A recognized application proxy path is accepted unchanged. Direct sources
/// must be absolute HTTP(S) URLs; error text never includes the URL.
fn validate_source_url(
    url: &str,
    proxy_public_path: Option<&str>,
) -> Result<(), SubtitleTranslationError> {
    if let Some(prefix) = proxy_public_path {
        let prefix = prefix.trim_end_matches('/');
        if url
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('/'))
            .is_some()
        {
            return Ok(());
        }
    }
    let parsed = url::Url::parse(url).map_err(|_| SubtitleTranslationError::InvalidDescriptor {
        detail: "source URL must be absolute".to_string(),
    })?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(SubtitleTranslationError::InvalidDescriptor {
            detail: "source URL must use HTTP or HTTPS with a host".to_string(),
        });
    }
    Ok(())
}

/// Rejects a policy without at least one text-compatible eligible service.
fn validate_policy(
    services: &[TranslationCatalogEntry],
    policy: &SelectionPolicy,
    source_language: Option<&LanguageCode>,
    target_language: &LanguageCode,
) -> Result<(), SubtitleTranslationError> {
    let eligible = |entry: &&TranslationCatalogEntry| {
        entry.service_yaml_hash.is_some()
            && entry.supports_text_translation
            && service_supports_pair(entry, source_language, target_language)
    };
    match policy {
        SelectionPolicy::Constrained { service_id } => {
            let found = services
                .iter()
                .filter(eligible)
                .any(|entry| entry.service_id == *service_id);
            if !found {
                return Err(SubtitleTranslationError::Translation(
                    arachnea_translation::TranslationError::NoEligibleService {
                        detail: format!(
                            "translation service `{service_id}` is unavailable or incompatible"
                        ),
                    },
                ));
            }
        }
        SelectionPolicy::Auto { .. } => {
            if !services.iter().filter(eligible).any(|_| true) {
                return Err(SubtitleTranslationError::Translation(
                    arachnea_translation::TranslationError::NoEligibleService {
                        detail: "no eligible subtitle translation service".to_string(),
                    },
                ));
            }
        }
    }
    Ok(())
}

/// Derives the access scope from the trusted peer address.
///
/// Desktop IPC has no peer address and is local by construction. REST loopback
/// callers share the local scope; other peers are isolated by IP. The port is
/// deliberately ignored because it changes on every connection.
fn access_scope(remote_addr: Option<SocketAddr>) -> AccessScope {
    let peer = match remote_addr {
        Some(address) => {
            let ip = address.ip();
            if ip.is_loopback() {
                "local".to_string()
            } else if ip.is_unspecified() {
                "remote".to_string()
            } else {
                format!("ip:{ip}")
            }
        }
        None => "local".to_string(),
    };
    AccessScope::new(format!("subtitle-translation/{peer}"))
        .expect("derived subtitle translation access scope is never blank")
}

/// Decodes JSON query values using the same parameter convention as JSON commands.
fn parse_request(query: &str) -> Result<TranslateSubtitleRequest, SubtitleTranslationError> {
    let mut parameters = serde_json::Map::new();
    for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
        let value = serde_json::from_str(&value)
            .unwrap_or_else(|_| serde_json::Value::String(value.into_owned()));
        if parameters.insert(name.into_owned(), value).is_some() {
            return Err(SubtitleTranslationError::InvalidDescriptor {
                detail: "duplicate subtitle translation parameter".to_string(),
            });
        }
    }
    serde_json::from_value(serde_json::Value::Object(parameters)).map_err(|_| {
        SubtitleTranslationError::InvalidDescriptor {
            detail: "invalid subtitle translation parameters".to_string(),
        }
    })
}

/// Selects one complete usable candidate from an `If-None-Match` list.
///
/// Wildcards add no source validator and never prove source stability, so they
/// are ignored and the request is translated again. Each candidate is decoded
/// and accepted as a whole; components from several tags are never mixed.
fn select_client_etag<'a>(
    header: &'a str,
    descriptor: &SubtitleTranslationDescriptor,
    catalog: &crate::translation_catalog::TranslationCatalog,
) -> Option<&'a str> {
    entity_tags(header)
        .into_iter()
        .take(IF_NONE_MATCH_MAX_TAGS)
        .find(|candidate| {
            if candidate.is_empty() || *candidate == "*" {
                return false;
            }
            let Ok(validator) = SubtitleTranslationValidator::from_etag(candidate) else {
                return false;
            };
            let Some(service) = catalog.service(&validator.service_id) else {
                return false;
            };
            let policy_allows = match &descriptor.policy {
                SelectionPolicy::Constrained { service_id } => service_id == &validator.service_id,
                SelectionPolicy::Auto { .. } => true,
            };
            policy_allows
                && service.service_yaml_hash.as_deref()
                    == Some(validator.service_yaml_hash.as_str())
                && service_supports_pair(
                    service,
                    descriptor.source_language.as_ref(),
                    &descriptor.target_language,
                )
        })
}

/// Splits an `If-None-Match` value without breaking quoted commas.
fn entity_tags(header: &str) -> Vec<&str> {
    let mut tags = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (index, byte) in header.bytes().enumerate() {
        match byte {
            b'"' => quoted = !quoted,
            b',' if !quoted => {
                tags.push(header[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    tags.push(header[start..].trim());
    tags
}

/// Builds a successful or not-modified subtitle file response.
fn file_response(
    status: u16,
    body: Vec<u8>,
    etag: Option<String>,
    cache_control: &str,
) -> ControlerStreamOutput {
    let mut headers = HashMap::from([("cache-control".to_string(), cache_control.to_string())]);
    if let Some(etag) = etag {
        headers.insert("etag".to_string(), etag);
        headers.insert(
            "access-control-expose-headers".to_string(),
            "ETag".to_string(),
        );
    }
    ControlerStreamOutput {
        status,
        body: ResponseBody::Buffered(body),
        content_type: "text/vtt".to_string(),
        headers,
    }
}

/// Builds an explicit error response that is never served as WebVTT.
fn error_output(error: SubtitleTranslationError) -> ControlerStreamOutput {
    ControlerStreamOutput {
        status: error.http_status().as_u16(),
        body: ResponseBody::Buffered(error.to_string().into_bytes()),
        content_type: "text/plain; charset=utf-8".to_string(),
        headers: HashMap::new(),
    }
}

/// Responds to unsupported methods, including HEAD, without side effects.
fn method_not_allowed() -> ControlerStreamOutput {
    ControlerStreamOutput {
        status: 405,
        body: ResponseBody::Buffered(Vec::new()),
        content_type: "text/plain; charset=utf-8".to_string(),
        headers: HashMap::from([(
            "allow".to_string(),
            SUBTITLE_FILE_ALLOWED_METHODS.to_string(),
        )]),
    }
}

/// Rejects paths nested below the direct translation endpoint.
fn not_found() -> ControlerStreamOutput {
    ControlerStreamOutput {
        status: 404,
        body: ResponseBody::Buffered(Vec::new()),
        content_type: "text/plain; charset=utf-8".to_string(),
        headers: HashMap::new(),
    }
}
