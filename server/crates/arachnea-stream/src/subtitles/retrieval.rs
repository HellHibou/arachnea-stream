//! Direct HTTP retrieval with destination checks and pinned DNS answers.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use arachnea_proxy::core::ArachneaProxyCore;
use arachnea_translation::AccessScope;
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http::header::{
    ACCEPT_ENCODING, CONTENT_TYPE, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, LOCATION,
};
use url::Url;

use super::{
    LocalSubtitleLimits, RetrievedSubtitle, SubtitleRetrievalOutcome, SubtitleRetrievalPort,
    SubtitleSourceDescriptor, SubtitleSourceValidator, SubtitleTranslationError,
};
use crate::{SUBTITLE_SOURCE_MAX_REDIRECTS, SUBTITLE_SOURCE_TIMEOUT_SECONDS};

/// Authorized source retriever with caller-supplied headers and bounded bodies.
///
/// HTTP(S) sources use direct retrieval. Recognized application proxy URLs use
/// the configured proxy core without calling the local controller. Every hop
/// pins a validated public IP while retaining the hostname for routing and TLS.
pub struct SubtitleHttpRetrieval {
    headers: HashMap<String, String>,
    limits: LocalSubtitleLimits,
    proxy_public_path: Option<String>,
    proxy_core: Option<ArachneaProxyCore>,
}

impl SubtitleHttpRetrieval {
    /// Creates a direct retriever for one authorized source and its headers.
    ///
    /// # Arguments
    ///
    /// * `headers` - Trusted headers provided by the source resolver.
    /// * `limits` - Source byte and document bounds.
    pub fn new(headers: HashMap<String, String>, limits: LocalSubtitleLimits) -> Self {
        Self {
            headers,
            limits,
            proxy_public_path: None,
            proxy_core: None,
        }
    }
    /// Uses the application's routing core for recognized internal proxy URLs.
    ///
    /// # Arguments
    /// * `core` - Shared application proxy runtime, including routing and TLS policy.
    pub fn with_proxy_core(mut self, core: ArachneaProxyCore) -> Self {
        self.proxy_core = Some(core);
        self
    }

    /// Allows a trusted application proxy path to be decoded locally.
    ///
    /// With a proxy core, the shared controller decoder preserves routing,
    /// cookies, actions and allowlisted TLS options. Without a core, only
    /// ordinary upstream headers can be applied by the direct retriever.
    ///
    /// # Arguments
    ///
    /// * `path` - Exact public proxy prefix configured by the application.
    pub fn with_proxy_public_path(mut self, path: impl Into<String>) -> Self {
        self.proxy_public_path = Some(path.into().trim_end_matches('/').to_string());
        self
    }
}

#[async_trait]
impl SubtitleRetrievalPort for SubtitleHttpRetrieval {
    async fn retrieve(
        &self,
        source: &SubtitleSourceDescriptor,
        _access_scope: &AccessScope,
        validator: Option<&SubtitleSourceValidator>,
    ) -> Result<SubtitleRetrievalOutcome, SubtitleTranslationError> {
        let result = self.retrieve_raw(source, validator).await?;
        match result {
            SubtitleRetrievalOutcome::Modified(document)
                if document.bytes.starts_with(b"#EXTM3U") =>
            {
                tokio::time::timeout(
                    Duration::from_secs(crate::SUBTITLE_HLS_TIMEOUT_SECONDS),
                    super::hls::assemble(
                        self,
                        source,
                        document,
                        self.limits,
                        self.proxy_public_path.as_deref(),
                    ),
                )
                .await
                .map_err(|_| retrieval_error("HLS subtitle assembly timed out"))?
            }
            other => Ok(other),
        }
    }
}

impl SubtitleHttpRetrieval {
    /// Retrieves one bounded resource using the same destination and proxy policies.
    ///
    /// # Arguments
    /// * `source` - Authorized resource descriptor.
    /// * `validator` - Optional conditional validator for this resource only.
    ///
    /// # Errors
    /// Returns destination, transport or size failures.
    pub(super) async fn retrieve_raw(
        &self,
        source: &SubtitleSourceDescriptor,
        validator: Option<&SubtitleSourceValidator>,
    ) -> Result<SubtitleRetrievalOutcome, SubtitleTranslationError> {
        if let (Some(core), Some(prefix)) = (&self.proxy_core, &self.proxy_public_path) {
            if let Some(suffix) = source
                .url
                .strip_prefix(prefix)
                .and_then(|rest| rest.strip_prefix('/'))
            {
                return super::retrieval_proxy::retrieve(
                    core,
                    prefix,
                    suffix,
                    &self.headers,
                    self.limits,
                    validator,
                )
                .await;
            }
        }
        let (target, headers) = resolve_source(
            &source.url,
            self.proxy_public_path.as_deref(),
            &self.headers,
        )?;
        let mut url = Url::parse(&target).map_err(|_| retrieval_error("invalid source URL"))?;
        let original_origin = url.origin();
        for hop in 0..=SUBTITLE_SOURCE_MAX_REDIRECTS {
            let addresses = resolve_public_addresses(&url).await?;
            let host = url
                .host_str()
                .ok_or_else(|| retrieval_error("source URL has no host"))?;
            let client = rquest::Client::builder()
                .no_proxy()
                .redirect(rquest::redirect::Policy::none())
                .timeout(Duration::from_secs(SUBTITLE_SOURCE_TIMEOUT_SECONDS))
                .resolve_to_addrs(host, &addresses)
                .build()
                .map_err(|_| retrieval_error("source HTTP client initialization failed"))?;
            let mut query = client.get(url.as_str()).header(ACCEPT_ENCODING, "identity");
            if url.origin() == original_origin {
                for (name, value) in &headers {
                    let name = http::header::HeaderName::from_bytes(name.as_bytes())
                        .map_err(|_| retrieval_error("invalid source header name"))?;
                    if matches!(
                        name,
                        IF_NONE_MATCH
                            | IF_MODIFIED_SINCE
                            | ACCEPT_ENCODING
                            | http::header::HOST
                            | http::header::RANGE
                            | http::header::CONTENT_LENGTH
                            | http::header::TRANSFER_ENCODING
                            | http::header::CONNECTION
                            | http::header::PROXY_AUTHORIZATION
                    ) {
                        return Err(retrieval_error(
                            "source header conflicts with retrieval policy",
                        ));
                    }
                    let value = http::header::HeaderValue::from_str(value)
                        .map_err(|_| retrieval_error("invalid source header value"))?;
                    query = query.header(name, value);
                }
            }
            query = match if hop == 0 { validator } else { None } {
                Some(SubtitleSourceValidator::Etag { etag }) => query.header(IF_NONE_MATCH, etag),
                Some(SubtitleSourceValidator::Content {
                    last_modified: Some(date),
                    ..
                }) => query.header(IF_MODIFIED_SINCE, date),
                _ => query,
            };
            let mut response = query
                .send()
                .await
                .map_err(|_| retrieval_error("source request failed"))?;
            let status = response.status();
            if status.as_u16() == 304 {
                if hop != 0
                    || !matches!(
                        validator,
                        Some(SubtitleSourceValidator::Etag { .. })
                            | Some(SubtitleSourceValidator::Content {
                                last_modified: Some(_),
                                ..
                            })
                    )
                {
                    return Err(retrieval_error(
                        "unexpected upstream 304 without a conditional request",
                    ));
                }
                return Ok(SubtitleRetrievalOutcome::NotModified);
            }
            if status.is_redirection() {
                if hop == SUBTITLE_SOURCE_MAX_REDIRECTS {
                    return Err(retrieval_error("source redirect limit exceeded"));
                }
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| retrieval_error("source redirect lacks a valid location"))?;
                url = url
                    .join(location)
                    .map_err(|_| retrieval_error("invalid source redirect"))?;
                continue;
            }
            if status.as_u16() != 200 {
                return Err(SubtitleTranslationError::SourceUnavailable);
            }
            let read_header = |header| {
                response
                    .headers()
                    .get(header)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned)
            };
            let content_type = read_header(CONTENT_TYPE);
            // A redirect can change its target between requests. Use a body
            // hash instead of replaying a validator from a different resource.
            let remote_etag = if hop == 0 { read_header(ETAG) } else { None };
            let last_modified = if hop == 0 {
                read_header(LAST_MODIFIED)
            } else {
                None
            };
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| retrieval_error("source body could not be read"))?
            {
                if bytes.len().saturating_add(chunk.len()) as u64 > self.limits.max_document_bytes {
                    return Err(SubtitleTranslationError::DocumentTooLarge {
                        bytes: bytes.len().saturating_add(chunk.len()) as u64,
                        limit: self.limits.max_document_bytes,
                    });
                }
                bytes.extend_from_slice(&chunk);
            }
            return Ok(SubtitleRetrievalOutcome::Modified(RetrievedSubtitle {
                bytes,
                content_type,
                remote_etag,
                last_modified,
                final_url: Some(url.to_string()),
            }));
        }
        Err(retrieval_error("source redirect limit exceeded"))
    }
}

pub(super) fn validate_url(url: &Url) -> Result<(), SubtitleTranslationError> {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host().is_none()
    {
        return Err(retrieval_error(
            "source URL is not an authorized HTTP destination",
        ));
    }
    Ok(())
}

fn allowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => allowed_ipv4(ip),
        IpAddr::V6(ip) => {
            if let Some(ip) = ip.to_ipv4_mapped() {
                return allowed_ipv4(ip);
            }
            let octets = ip.octets();
            octets[0] & 0xe0 == 0x20
                && !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && octets[0] & 0xfe != 0xfc
                && !(octets[0] == 0xfe && octets[1] & 0xc0 == 0x80)
                && !(octets[0] == 0x20 && octets[1] == 0x02)
                && !(octets[0] == 0x20 && octets[1] == 0x01 && octets[2] == 0 && octets[3] == 0)
                && !(octets[0] == 0x20
                    && octets[1] == 0x01
                    && octets[2] == 0x0d
                    && octets[3] == 0xb8)
        }
    }
}

fn allowed_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !ip.is_private()
        && !ip.is_loopback()
        && !ip.is_link_local()
        && !ip.is_multicast()
        && !ip.is_unspecified()
        && !ip.is_broadcast()
        && a != 0
        && a < 224
        && !(a == 100 && (64..=127).contains(&b))
        && !(a == 192 && b == 0 && c == 0)
        && !(a == 192 && b == 0 && c == 2)
        && !(a == 198 && (b == 18 || b == 19))
        && !(a == 198 && b == 51 && c == 100)
        && !(a == 203 && b == 0 && c == 113)
}

pub(super) fn retrieval_error(detail: &str) -> SubtitleTranslationError {
    SubtitleTranslationError::RetrievalFailed {
        detail: detail.to_string(),
    }
}

/// Resolves a recognized local proxy path without making a loopback request.
fn resolve_source(
    source: &str,
    proxy_path: Option<&str>,
    initial_headers: &HashMap<String, String>,
) -> Result<(String, HashMap<String, String>), SubtitleTranslationError> {
    let mut headers = initial_headers.clone();
    let Some(rest) = proxy_path
        .filter(|path| !path.is_empty())
        .and_then(|path| source.strip_prefix(path))
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return Ok((source.to_string(), headers));
    };
    let (options, target) = if let Some(encoded) = rest.strip_prefix("opts_") {
        let (encoded, target) = encoded
            .split_once('/')
            .ok_or_else(|| retrieval_error("invalid proxy URL options"))?;
        if encoded.len() > crate::SUBTITLE_PROXY_OPTIONS_MAX_LENGTH {
            return Err(retrieval_error("proxy URL options exceed the limit"));
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| retrieval_error("invalid proxy URL options"))?;
        let options: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| retrieval_error("invalid proxy URL options"))?;
        (Some(options), target)
    } else {
        (None, rest)
    };
    if let Some(options) = options {
        let object = options
            .as_object()
            .ok_or_else(|| retrieval_error("invalid proxy URL options"))?;
        if object.keys().any(|key| key != "headers") {
            return Err(retrieval_error(
                "proxy URL routing options require the application proxy adapter",
            ));
        }
        if let Some(values) = object.get("headers") {
            let pairs: Vec<(String, String)> = match values {
                serde_json::Value::Object(map) => map
                    .iter()
                    .map(|(name, value)| {
                        Ok((
                            name.clone(),
                            value
                                .as_str()
                                .ok_or_else(|| retrieval_error("invalid proxy header"))?
                                .to_string(),
                        ))
                    })
                    .collect::<Result<_, SubtitleTranslationError>>()?,
                serde_json::Value::Array(items) => items
                    .iter()
                    .map(|item| {
                        let pair = item
                            .as_array()
                            .filter(|pair| pair.len() == 2)
                            .ok_or_else(|| retrieval_error("invalid proxy header"))?;
                        Ok((
                            pair[0]
                                .as_str()
                                .ok_or_else(|| retrieval_error("invalid proxy header"))?
                                .to_string(),
                            pair[1]
                                .as_str()
                                .ok_or_else(|| retrieval_error("invalid proxy header"))?
                                .to_string(),
                        ))
                    })
                    .collect::<Result<_, SubtitleTranslationError>>()?,
                _ => return Err(retrieval_error("invalid proxy headers")),
            };
            for (name, value) in pairs {
                if name.to_ascii_lowercase().starts_with("arachnea-") {
                    return Err(retrieval_error(
                        "proxy URL routing options require the application proxy adapter",
                    ));
                }
                if headers
                    .keys()
                    .any(|existing| existing.eq_ignore_ascii_case(&name))
                {
                    return Err(retrieval_error("duplicate source header"));
                }
                headers.insert(name, value);
            }
        }
    }
    let target = if target.starts_with("http://") || target.starts_with("https://") {
        target.to_string()
    } else if let Some(rest) = target.strip_prefix("http/") {
        format!("http://{rest}")
    } else if let Some(rest) = target.strip_prefix("https/") {
        format!("https://{rest}")
    } else {
        return Err(retrieval_error("invalid application proxy target"));
    };
    Ok((target, headers))
}

/// Resolves and validates every address before a source hop opens a connection.
///
/// # Arguments
/// * `url` - Absolute HTTP(S) source or redirect destination.
///
/// # Errors
/// Returns an error for invalid URLs, DNS failures, or non-public addresses.
pub(super) async fn resolve_public_addresses(
    url: &Url,
) -> Result<Vec<SocketAddr>, SubtitleTranslationError> {
    validate_url(url)?;
    let host = url
        .host_str()
        .ok_or_else(|| retrieval_error("source URL has no host"))?
        .trim_matches(['[', ']']);
    let port = url
        .port_or_known_default()
        .ok_or_else(|| retrieval_error("source URL has no port"))?;
    let addresses = tokio::time::timeout(
        Duration::from_secs(SUBTITLE_SOURCE_TIMEOUT_SECONDS),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .map_err(|_| retrieval_error("source DNS lookup timed out"))?
    .map_err(|_| retrieval_error("source DNS lookup failed"))?
    .collect::<Vec<_>>();
    if addresses.is_empty() || addresses.iter().any(|address| !allowed_ip(address.ip())) {
        return Err(retrieval_error(
            "source destination is not a public IP address",
        ));
    }
    Ok(addresses)
}
