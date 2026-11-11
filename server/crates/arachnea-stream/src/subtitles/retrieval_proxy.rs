//! Bounded in-process subtitle retrieval using the application proxy runtime.

use std::{collections::HashMap, time::Duration};

use arachnea_core::controler::ControlerStreamInput;
use arachnea_proxy::core::{ArachneaProxyCore, ProxySearchContext};
use arachnea_proxy::http::{
    actions::apply_post_actions,
    proxy_service::{prepare_proxy_http_source, PreparedProxyHttpSource},
    response_classifier::ProxyResponseDecision,
    ProxiedHttpRequest, ProxiedHttpResponse, ProxiedResponseBody, SimpleHttpClient,
};
use futures::StreamExt;
use tracing::instrument::WithSubscriber;
use url::Url;

use super::retrieval::{resolve_public_addresses, retrieval_error};
use super::{
    LocalSubtitleLimits, RetrievedSubtitle, SubtitleRetrievalOutcome, SubtitleSourceValidator,
    SubtitleTranslationError,
};
use crate::{SUBTITLE_SOURCE_MAX_REDIRECTS, SUBTITLE_SOURCE_TIMEOUT_SECONDS};

/// Retrieves a recognized proxy URL without requesting a local controller address.
///
/// # Arguments
/// * `core` - Application proxy runtime and security policy.
/// * `prefix` - Exact trusted public proxy entry point.
/// * `suffix` - Upstream route suffix, including encoded options.
/// * `headers` - Trusted source headers supplied by the resolver.
/// * `limits` - Local document bounds.
/// * `validator` - Previously issued source validator, when available.
///
/// # Errors
/// Returns decoding, access, routing, size or source-retrieval failures.
pub(super) async fn retrieve(
    core: &ArachneaProxyCore,
    prefix: &str,
    suffix: &str,
    headers: &HashMap<String, String>,
    limits: LocalSubtitleLimits,
    validator: Option<&SubtitleSourceValidator>,
) -> Result<SubtitleRetrievalOutcome, SubtitleTranslationError> {
    let input = ControlerStreamInput {
        path: suffix.to_string(),
        query: String::new(),
        method: "GET".into(),
        headers: headers.clone(),
        body: Vec::new(),
        entry_point: prefix.to_string(),
        force_buffer_response: false,
    };
    let mut prepared = prepare_proxy_http_source(core, &input)
        .map_err(|_| retrieval_error("invalid or unauthorized application proxy source options"))?;
    let original_origin = source_url(&prepared)?.origin();
    let max_redirects = prepared
        .follow_redirects()
        .unwrap_or(SUBTITLE_SOURCE_MAX_REDIRECTS)
        .min(SUBTITLE_SOURCE_MAX_REDIRECTS);
    let search = ProxySearchContext::default().with_replayable(prepared.replayable());
    search
        .scope(async {
            for hop in 0..=max_redirects {
                let url = source_url(&prepared)?;
                let addresses = resolve_public_addresses(&url).await?;
                let mut request = prepared.request.clone();
                if url.origin() != original_origin {
                    request.headers.clear();
                    request.cookies.clear();
                }
                // Read a bounded stream before executing any body transformation.
                let actions = std::mem::take(&mut request.post_actions);
                request.buffer_response_body = false;
                request.follow_redirects = None;
                request.headers.retain(|name, _| {
                    ![
                        "if-none-match",
                        "if-modified-since",
                        "range",
                        "accept-encoding",
                    ]
                    .iter()
                    .any(|blocked| name.eq_ignore_ascii_case(blocked))
                });
                request
                    .headers
                    .insert("Accept-Encoding".into(), "identity".into());
                let conditional = hop == 0
                    && actions.is_empty()
                    && match validator {
                        Some(SubtitleSourceValidator::Etag { etag }) => {
                            request.headers.insert("If-None-Match".into(), etag.clone());
                            true
                        }
                        Some(SubtitleSourceValidator::Content {
                            last_modified: Some(date),
                            ..
                        }) => {
                            request
                                .headers
                                .insert("If-Modified-Since".into(), date.clone());
                            true
                        }
                        _ => false,
                    };
                let routing_core = if request.client_context.get_string("countries").is_some()
                    || request.client_context.get_string("country").is_some()
                {
                    core.with_dynamic_proxy_observation_scope()
                } else {
                    core.clone()
                };
                let client = SimpleHttpClient::new(routing_core);
                let fetch = async {
                    let response =
                        request_hop(&client, request.clone(), addresses[0].ip(), &search).await?;
                    let status = response.status;
                    let mut headers = response.headers;
                    if status == 304 {
                        if !conditional {
                            return Err(retrieval_error("unexpected proxy source 304"));
                        }
                        return Ok(Hop::NotModified);
                    }
                    if (300..400).contains(&status) {
                        let location = header(&headers, "location").ok_or_else(|| {
                            retrieval_error("proxy source redirect lacks a location")
                        })?;
                        return Ok(Hop::Redirect(status, location.to_string()));
                    }
                    if status != 200 {
                        return Err(SubtitleTranslationError::SourceUnavailable);
                    }
                    if header(&headers, "content-encoding").is_some_and(|value| {
                        !value.is_empty() && !value.eq_ignore_ascii_case("identity")
                    }) {
                        return Err(retrieval_error(
                            "proxy source did not honor identity content encoding",
                        ));
                    }
                    let mut bytes = read_body(response.body, limits.max_document_bytes).await?;
                    if !actions.is_empty() {
                        bytes = apply_post_actions(
                            status,
                            &mut headers,
                            bytes,
                            &actions,
                            &request.context,
                        )
                        .map_err(|_| {
                            retrieval_error("subtitle proxy source transformation failed")
                        })?;
                        check_size(bytes.len(), limits.max_document_bytes)?;
                    }
                    let remote_etag = (hop == 0 && actions.is_empty())
                        .then(|| header(&headers, "etag").map(str::to_string))
                        .flatten();
                    let last_modified = (hop == 0 && actions.is_empty())
                        .then(|| header(&headers, "last-modified").map(str::to_string))
                        .flatten();
                    Ok(Hop::Document(RetrievedSubtitle {
                        bytes,
                        content_type: header(&headers, "content-type").map(str::to_string),
                        remote_etag,
                        last_modified,
                        final_url: Some(url.to_string()),
                    }))
                };
                match tokio::time::timeout(
                    Duration::from_secs(SUBTITLE_SOURCE_TIMEOUT_SECONDS),
                    fetch,
                )
                .await
                .map_err(|_| retrieval_error("application proxy source retrieval timed out"))??
                {
                    Hop::NotModified => return Ok(SubtitleRetrievalOutcome::NotModified),
                    Hop::Document(document) => {
                        return Ok(SubtitleRetrievalOutcome::Modified(document))
                    }
                    Hop::Redirect(status, location) => {
                        if hop == max_redirects {
                            return Err(retrieval_error("source redirect limit exceeded"));
                        }
                        prepared = prepared.redirect(core, status, &location).map_err(|_| {
                            retrieval_error("invalid application proxy source redirect")
                        })?;
                    }
                }
            }
            Err(retrieval_error("source redirect limit exceeded"))
        })
        .with_subscriber(tracing::subscriber::NoSubscriber::default())
        .await
}

/// Executes a safe GET hop with the normal destination rejection/retry policy.
///
/// # Arguments
/// * `client` - Application proxy HTTP client.
/// * `request` - Decoded one-hop GET request.
/// * `address` - Validated public transport IP.
/// * `search` - Bounded proxy-selection context for this source operation.
async fn request_hop(
    client: &SimpleHttpClient,
    request: ProxiedHttpRequest,
    address: std::net::IpAddr,
    search: &ProxySearchContext,
) -> Result<ProxiedHttpResponse, SubtitleTranslationError> {
    loop {
        match client
            .request_proxied_with_resolved_ip(request.clone(), address)
            .await
        {
            Ok(response) => {
                if let Some(token) = &response.selection_token {
                    token
                        .record_response_decision(&response.response_decision)
                        .await;
                }
                if matches!(
                    response.response_decision,
                    ProxyResponseDecision::RejectedForDestination { .. }
                ) {
                    if response.selection_token.is_some() && search.can_start() {
                        search.end_attempt();
                        continue;
                    }
                    return Err(SubtitleTranslationError::SourceUnavailable);
                }
                return Ok(response);
            }
            Err(error) => {
                if let arachnea_proxy::core::ProxyError::RequestAttempt {
                    selection: Some(token),
                    delivery,
                    ..
                } = &error
                {
                    token
                        .record_failure(arachnea_proxy::core::ProxyDestinationFailureReason::Other)
                        .await;
                    if search.can_start()
                        && arachnea_proxy::core::proxy_retry_allowed(
                            "GET",
                            search.replayable(),
                            *delivery,
                            false,
                        )
                    {
                        search.end_attempt();
                        continue;
                    }
                }
                return Err(retrieval_error("application proxy source request failed"));
            }
        }
    }
}

/// Source hop result before a redirect or document is exposed to orchestration.
enum Hop {
    NotModified,
    Redirect(u16, String),
    Document(RetrievedSubtitle),
}

/// Parses a prepared target without exposing its URL in an error.
///
/// # Arguments
/// * `source` - Decoded proxy source request.
fn source_url(source: &PreparedProxyHttpSource) -> Result<Url, SubtitleTranslationError> {
    Url::parse(&source.request.url)
        .map_err(|_| retrieval_error("invalid application proxy source URL"))
}

/// Reads an HTTP response header without relying on its original casing.
///
/// # Arguments
/// * `headers` - Upstream HTTP response headers.
/// * `name` - Header name to find with ASCII-insensitive comparison.
fn header<'a>(headers: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Collects a complete source while enforcing its byte limit incrementally.
///
/// # Arguments
/// * `body` - Upstream response body.
/// * `limit` - Maximum accepted source-body length in bytes.
async fn read_body(
    body: ProxiedResponseBody,
    limit: u64,
) -> Result<Vec<u8>, SubtitleTranslationError> {
    match body {
        ProxiedResponseBody::Buffered(bytes) => {
            check_size(bytes.len(), limit)?;
            Ok(bytes)
        }
        ProxiedResponseBody::Streamed(mut stream) => {
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk =
                    chunk.map_err(|_| retrieval_error("proxy source body could not be read"))?;
                check_size(bytes.len().saturating_add(chunk.len()), limit)?;
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        }
    }
}

/// Checks a materialized or prospective source-body length.
///
/// # Arguments
/// * `bytes` - Materialized or prospective byte count.
/// * `limit` - Maximum accepted byte count.
fn check_size(bytes: usize, limit: u64) -> Result<(), SubtitleTranslationError> {
    if bytes as u64 > limit {
        Err(SubtitleTranslationError::DocumentTooLarge {
            bytes: bytes as u64,
            limit,
        })
    } else {
        Ok(())
    }
}
