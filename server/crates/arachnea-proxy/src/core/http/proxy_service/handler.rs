//! Controller stream request handling through the proxy HTTP client.

use std::collections::HashMap;
use std::sync::Arc;

use url::Url;

use crate::core::http::actions::{post_actions_require_body_buffering, PostActionContext};
use crate::core::http::{ProxiedHttpRequest, ProxiedResponseBody, SimpleHttpClient};
use crate::core::ArachneaProxyCore;
use arachnea_core::controler::{ControlerStreamInput, ControlerStreamOutput, ResponseBody};

use super::headers::{
    context_from_header_map, filter_response_non_transferable, remove_header_case_insensitive,
    remove_header_variants, strip_non_forwarded_parameter_headers,
};
use super::options::{
    decode_base64url_segment, merge_opts, parse_opts_value, MergedProxyHttpInput,
};
use super::redirects::{redirect_opts_encoded, rewrite_location_header};
use super::urls::{build_target_url, parse_proxy_path};
use crate::MAX_LOCAL_URL_BYTES;

fn stream_error(status: u16, message: impl Into<String>) -> ControlerStreamOutput {
    ControlerStreamOutput {
        status,
        body: ResponseBody::Buffered(message.into().into_bytes()),
        content_type: "text/plain; charset=utf-8".to_string(),
        headers: HashMap::new(),
    }
}

/// Handles a proxied HTTP request through the controller stream interface.
///
/// This is the main entry point registered as a stream function. The
/// entry-point URL used for rewriting `Location` headers in redirect responses
/// is read from `input.entry_point`.
///
/// # Arguments
/// - `proxy_core`: Routing core shared with the application.
/// - `input`: Incoming controller stream request.
/// - `fallback_direct`: Default if no country proxy is available. The boolean
///   URL option overrides the request header, which overrides this default.
///
/// # Errors
/// Returns controller stream errors; upstream failures produce an HTTP 502 response.
pub async fn handle_proxy_http(
    proxy_core: Arc<ArachneaProxyCore>,
    input: ControlerStreamInput,
    fallback_direct: bool,
) -> Result<ControlerStreamOutput, String> {
    let proxy_core = Arc::new((*proxy_core).clone().with_fallback_direct(fallback_direct));
    // Validate URL length
    let full_url = format!(
        "{}/{}{}{}",
        input.entry_point,
        &input.path,
        if input.query.is_empty() { "" } else { "?" },
        &input.query
    );
    if full_url.len() > MAX_LOCAL_URL_BYTES {
        return Ok(stream_error(414, "URI Too Long"));
    }

    // Parse the new path format
    let parsed = match parse_proxy_path(&input.path) {
        Ok(p) => p,
        Err((status, msg)) => return Ok(stream_error(status, msg)),
    };

    // Build the target URL
    let target_url_str = build_target_url(
        &parsed.protocol,
        parsed.port,
        &parsed.host,
        &parsed.path_and_query,
        &input.query,
    );

    // Validate target URL
    let parsed_target = match Url::parse(&target_url_str) {
        Ok(url) => url,
        Err(_) => {
            return Ok(stream_error(
                400,
                format!("invalid target URL '{}'", target_url_str),
            ));
        }
    };

    let scheme = parsed_target.scheme();
    if scheme != "http" && scheme != "https" {
        return Ok(stream_error(
            400,
            format!(
                "unsupported URL scheme '{}' (only http/https allowed)",
                scheme
            ),
        ));
    }
    let method = input.method.to_ascii_uppercase();
    const SUPPORTED_METHODS: &[&str] =
        &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
    if !SUPPORTED_METHODS.contains(&method.as_str()) {
        return Ok(stream_error(
            400,
            format!("unsupported HTTP method '{}'", input.method),
        ));
    }

    // Parse opts from the path segment
    let opts = if parsed.opts_encoded.is_empty() {
        None
    } else {
        let opts_json_bytes = match decode_base64url_segment(&parsed.opts_encoded) {
            Ok(bytes) => bytes,
            Err(error) => return Ok(stream_error(400, error)),
        };
        let opts_json_str = match String::from_utf8(opts_json_bytes) {
            Ok(value) => value,
            Err(_) => return Ok(stream_error(400, "opts is not valid UTF-8")),
        };

        let opts_value: serde_json::Value = match serde_json::from_str(&opts_json_str) {
            Ok(value) => value,
            Err(error) => return Ok(stream_error(400, format!("invalid opts JSON: {error}"))),
        };

        let opts_parsed = match parse_opts_value(&opts_value) {
            Ok(opts) => opts,
            Err((status, message)) => return Ok(stream_error(status, message)),
        };

        Some(opts_parsed)
    };

    let redirect_opts = opts.clone();

    let verify_tls = if opts.as_ref().is_some_and(|opts| opts.insecure_tls) {
        let Some(host) = parsed_target.host_str() else {
            return Ok(stream_error(400, "HTTPS target is missing a host"));
        };
        if parsed_target.scheme() != "https" || !proxy_core.allows_insecure_tls_for_host(host) {
            return Ok(stream_error(
                403,
                format!("TLS bypass is not allowed for host '{host}'"),
            ));
        }
        tracing::warn!(
            host,
            "TLS certificate validation bypassed for allowlisted proxy host"
        );
        false
    } else {
        true
    };

    // Strip incoming Referer that points back to the proxy itself —
    // the proxy's own URL is meaningless to the upstream server.
    // Explicit Referer via opts headers will still be applied below.
    let mut incoming_headers = input.headers.clone();
    if let Some(referer_val) = incoming_headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("referer"))
        .map(|(_, v)| v.clone())
    {
        if referer_val.starts_with(&input.entry_point) {
            remove_header_variants(&mut incoming_headers, "referer");
        }
    }

    // Merge headers and cookies
    let MergedProxyHttpInput {
        mut headers,
        cookies,
        post_actions,
        redirect_actions,
    } = match merge_opts(&incoming_headers, opts) {
        Ok(value) => value,
        Err((status, error)) => return Ok(stream_error(status, error)),
    };
    let parameter_definitions = proxy_core.parameter_definitions();
    let mut client_context = context_from_header_map(&headers, &parameter_definitions);
    if let Some(value) = redirect_opts.as_ref().and_then(|opts| opts.fallback_direct) {
        client_context.insert(
            crate::core::PROXY_PARAMETER_FALLBACK_DIRECT,
            crate::core::ClientParameter::Bool(value),
        );
    }
    strip_non_forwarded_parameter_headers(&mut headers, &parameter_definitions);

    // Set the Host header from the target URL (host is removed as non-transferable
    //    from the incoming request since it refers to the proxy, not the target).
    let host_header = match parsed.port {
        Some(port) if port != 80 && port != 443 => format!("{}:{}", parsed.host, port),
        _ => parsed.host.clone(),
    };
    headers.insert("Host".to_string(), host_header);

    // When post-actions require body buffering, force Accept-Encoding: identity
    // to avoid dealing with compressed bodies. In streaming mode (no buffering),
    // leave Accept-Encoding untouched so the upstream can send compressed data.
    // When the controller backend does not support streaming (force_buffer_response),
    // also buffer the complete body before returning it to the controller.
    let buffer_response_body =
        post_actions_require_body_buffering(&post_actions) || input.force_buffer_response;
    if buffer_response_body {
        remove_header_variants(&mut headers, "accept-encoding");
        headers.insert("Accept-Encoding".to_string(), "identity".to_string());
    }

    // Log the target URL being called
    tracing::debug!(
        "proxy_http {} {} ({} opts segments, {} headers, {} cookies, {} post_actions) - Headers:{:?}",
        method,
        target_url_str,
        parsed.opts_encoded.len(),
        headers.len(),
        cookies.len(),
        post_actions.len(),
        headers
    );

    // Execute proxy request
    let request_proxy_core = if client_context
        .get_string("countries")
        .is_some_and(|value| !value.is_empty())
        || client_context
            .get_string("country")
            .is_some_and(|value| !value.is_empty())
    {
        proxy_core.with_dynamic_proxy_observation_scope()
    } else {
        (*proxy_core).clone()
    };
    let client = SimpleHttpClient::new(request_proxy_core.clone());
    let headers_only = method == "HEAD";
    let proxy_request = ProxiedHttpRequest {
        response_classifier: Some(
            match crate::core::http::response_classifier::resolve_response_policy(
                redirect_opts
                    .as_ref()
                    .and_then(|opts| opts.proxy_response_classifier.as_ref()),
                redirect_opts
                    .as_ref()
                    .map(|opts| opts.proxy_rejection_statuses.as_slice())
                    .unwrap_or_default(),
            ) {
                Ok(policy) => policy,
                Err(error) => return Ok(stream_error(400, error.to_string())),
            },
        ),
        url: target_url_str.clone(),
        method: method.clone(),
        headers,
        cookies,
        body: input.body,
        client_context,
        post_actions,
        buffer_response_body,
        headers_only,
        context: PostActionContext {
            entry_point: input.entry_point.clone(),
            target_url: target_url_str.clone(),
            opts_encoded: parsed.opts_encoded.clone(),
        },
        verify_tls,
        follow_redirects: redirect_opts
            .as_ref()
            .and_then(|opts| opts.follow_redirects),
    };

    let search = proxy_request
        .client_context
        .get_string("countries")
        .filter(|countries| !countries.is_empty() && *countries != "[]")
        .map(|_| {
            crate::core::ProxySearchContext::default().with_replayable(
                redirect_opts
                    .as_ref()
                    .is_some_and(|opts| opts.proxy_retry_replayable),
            )
        });
    let execute = async {
        loop {
            match client.request_proxied(proxy_request.clone()).await {
                Ok(response) => {
                    if let Some(token) = &response.selection_token {
                        token
                            .record_response_decision(&response.response_decision)
                            .await;
                    }
                    let retry = response.selection_token.is_some()
                        && matches!(response.response_decision, crate::core::http::response_classifier::ProxyResponseDecision::RejectedForDestination { .. })
                        && search.as_ref().is_some_and(|search| search.can_start() && crate::core::proxy_retry_allowed(
                            &method, search.replayable(), crate::core::ProxyRequestDelivery::ResponseReceived, true));
                    if retry {
                        search.as_ref().unwrap().end_attempt();
                        continue;
                    }
                    break Ok(response);
                }
                Err(error) => {
                    if let crate::core::ProxyError::RequestAttempt {
                        selection: Some(token),
                        delivery,
                        ..
                    } = &error
                    {
                        token
                            .record_failure(crate::core::ProxyDestinationFailureReason::Other)
                            .await;
                        if search.as_ref().is_some_and(|search| {
                            let buffered_replay = proxy_request.buffer_response_body
                                && *delivery == crate::core::ProxyRequestDelivery::ResponseReceived
                                && (search.replayable() || method == "GET" || method == "HEAD");
                            search.can_start()
                                && (buffered_replay
                                    || crate::core::proxy_retry_allowed(
                                        &method,
                                        search.replayable(),
                                        *delivery,
                                        false,
                                    ))
                        }) {
                            search.as_ref().unwrap().end_attempt();
                            continue;
                        }
                    }
                    break Err(error);
                }
            }
        }
    };
    let result = if let Some(search) = &search {
        search.scope(execute).await
    } else {
        execute.await
    };
    if let Some(search) = &search {
        search.log_progress(if result.is_ok() {
            "response received"
        } else {
            "request failed"
        });
    }
    let proxy_response = match result {
        Ok(response) => response,
        Err(error) => return Ok(stream_error(502, format!("Proxy error: {error}"))),
    };

    let status = proxy_response.status;
    let mut response_headers = proxy_response.headers;

    // Rewrite Location header for redirects
    if (300..400).contains(&status) {
        let redirect_opts_encoded = redirect_opts_encoded(
            status,
            &parsed.opts_encoded,
            redirect_opts.as_ref(),
            &redirect_actions,
        );
        if let Err(error) = rewrite_location_header(
            &mut response_headers,
            &target_url_str,
            &redirect_opts_encoded,
            &input.entry_point,
        ) {
            return Ok(stream_error(502, error));
        }
    }
    filter_response_non_transferable(&mut response_headers);

    // Derive content-type
    let content_type = remove_header_case_insensitive(&mut response_headers, "content-type")
        .unwrap_or_else(|| "application/octet-stream".to_string());

    // Handle HEAD: return no body
    let body = if method == "HEAD" {
        ResponseBody::Buffered(Vec::new())
    } else {
        match proxy_response.body {
            ProxiedResponseBody::Buffered(bytes) => ResponseBody::Buffered(bytes),
            ProxiedResponseBody::Streamed(stream) => ResponseBody::Streamed(stream),
        }
    };

    Ok(ControlerStreamOutput {
        status,
        body,
        content_type,
        headers: response_headers,
    })
}
