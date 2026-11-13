//! Public translation catalog and direct conditional document transport.
//!
//! Requests build an authorized descriptor in memory and revalidate or translate
//! a complete document. Only the browser retains the `ETag` and bytes pair.

use std::collections::HashMap;
use std::net::SocketAddr;

use crate::{AccessScope, LanguageCode, LocalTranslationLimits, SelectionPolicy, SupportedPair};
use arachnea_core::controler::{ControlerStreamInput, ControlerStreamOutput, ResponseBody};
use arachnea_scrapyfy::scrapyfy::conditional::hash62;
use serde::Serialize;

use crate::catalog::TranslationCatalogEntry;
use crate::document::{
    service_supports_pair, LocalSubtitleLimits, SubtitleSourceDescriptor,
    SubtitleTranslationDescriptor, SubtitleTranslationError, SubtitleTranslationOutcome,
    SubtitleTranslationRequest, SubtitleTranslationValidator,
};
use crate::DEFAULT_SUBTITLE_CACHE_CONTROL;

/// Maximum number of `If-None-Match` candidates evaluated per request.
const IF_NONE_MATCH_MAX_TAGS: usize = 32;

/// HEAD and other unsupported methods never retrieve or translate.
const SUBTITLE_FILE_ALLOWED_METHODS: &str = "GET";

use super::{parse_format, parse_request, TranslateSubtitleRequest};

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
    url_translation_formats: Vec<String>,
    source_detection: bool,
    source_detection_identifies_language: bool,
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
pub struct TranslationServicesView {
    services: Vec<TranslationServiceView>,
    languages: Vec<String>,
}

impl TranslationServicesView {
    /// Builds a public catalog view without constructing a source retriever.
    ///
    /// # Arguments
    /// * `catalog` - Validated provider metadata from the retained snapshot.
    pub fn from_catalog(catalog: &crate::catalog::TranslationCatalog) -> Self {
        let services = catalog.services().iter().map(service_view).collect();
        let languages = catalog
            .available_languages()
            .iter()
            .map(|language| language.as_str().to_string())
            .collect();
        TranslationServicesView {
            services,
            languages,
        }
    }
}

/// One operation's borrowed translation configuration and authorized retrieval.
pub struct TranslationHandler<'a> {
    /// Effective provider catalog from the retained runtime snapshot.
    pub translation_catalog: &'a crate::catalog::TranslationCatalog,
    /// Loaded provider services and resolved credentials.
    pub aggregator: &'a arachnea_scrapyfy::ScraperAgregator,
    /// Caller-supplied provider group name.
    pub group_name: &'a str,
    /// Authorized source retrieval implementation.
    pub retrieval: &'a dyn crate::document::SubtitleRetrievalPort,
    /// Recognized application proxy URL prefix.
    pub proxy_public_path: Option<&'a str>,
}

impl TranslationHandler<'_> {
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
            .source
            .as_deref()
            .map(|value| parse_language(value, "source language"))
            .transpose()?;
        let target_language = parse_language(&request.target, "target language")?;
        let format = parse_format(&request.content_type)?;
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
        validate_source_url(&source.url, self.proxy_public_path)?;
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

    /// Returns a complete translated document or an empty conditional `304`.
    ///
    /// # Arguments
    /// * `input` - GET request with JSON-encoded query parameters and optional ETag.
    ///
    /// # Errors
    /// Protocol and translation failures are returned as explicit error responses.
    pub async fn get_translated_subtitle(
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
        let outcome = crate::document::translate_subtitle(
            self.retrieval,
            self.aggregator,
            self.translation_catalog,
            self.group_name,
            SubtitleTranslationRequest {
                descriptor: &descriptor,
                access_scope: &access_scope,
                client_etag,
                provider_accessible_url: None,
                subtitle_limits: LocalSubtitleLimits::default(),
                translation_limits: LocalTranslationLimits::default(),
            },
        )
        .await;
        match outcome {
            Ok(SubtitleTranslationOutcome::NotModified { etag }) => Ok(file_response(
                304,
                Vec::new(),
                Some(etag),
                DEFAULT_SUBTITLE_CACHE_CONTROL,
                descriptor.source.format.media_type(),
            )),
            Ok(SubtitleTranslationOutcome::Translated(document)) => Ok(file_response(
                200,
                document.bytes,
                Some(document.etag),
                &document.cache_control,
                &document.media_type,
            )),
            Ok(SubtitleTranslationOutcome::Original { bytes }) => Ok(file_response(
                200,
                bytes,
                None,
                DEFAULT_SUBTITLE_CACHE_CONTROL,
                descriptor.source.format.media_type(),
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
        url_translation_formats: entry.url_translation_formats.clone(),
        source_detection: entry.source_detection,
        source_detection_identifies_language: entry.source_detection_identifies_language,
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
                    crate::TranslationError::NoEligibleService {
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
                    crate::TranslationError::NoEligibleService {
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

/// Selects one complete usable candidate from an `If-None-Match` list.
///
/// Wildcards add no source validator and never prove source stability, so they
/// are ignored and the request is translated again. Each candidate is decoded
/// and accepted as a whole; components from several tags are never mixed.
fn select_client_etag<'a>(
    header: &'a str,
    descriptor: &SubtitleTranslationDescriptor,
    catalog: &crate::catalog::TranslationCatalog,
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
                && descriptor.validator_hash(service) == validator.service_yaml_hash
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
    content_type: &str,
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
        content_type: content_type.to_string(),
        headers,
    }
}

/// Builds an explicit error response with a non-document media type.
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
