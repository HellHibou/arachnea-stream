//! Controller stream request handling through the proxy HTTP client.

use std::collections::HashMap;
use std::sync::Arc;

use tracing::{instrument::WithSubscriber, Instrument};

use crate::core::http::{ProxiedResponseBody, SimpleHttpClient};
use crate::core::ArachneaProxyCore;
use arachnea_core::controler::{ControlerStreamInput, ControlerStreamOutput, ResponseBody};

use super::headers::{filter_response_non_transferable, remove_header_case_insensitive};
use super::redirects::{redirect_opts_encoded, rewrite_location_header};

/// Builds a buffered plain-text controller response for a proxy failure.
///
/// # Arguments
/// * `status` - HTTP status returned to the controller client.
/// * `message` - Error text returned in the buffered plain-text response.
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
/// is read from `input.entry_point`. An owned child task keeps media discovery
/// off the controller polling stack while propagating the ambient search and tracing context.
///
/// # Arguments
/// - `proxy_core`: Routing core shared with the application.
/// - `input`: Incoming controller stream request.
/// - `fallback_direct`: Default if no country proxy is available. The boolean
///   URL option overrides the request header, which overrides this default.
///
/// # Errors
/// Returns controller stream errors; upstream or child-task failures produce HTTP 502.
/// Dropping the request aborts its owned child. Returned streams outlive that task.
pub async fn handle_proxy_http(
    proxy_core: Arc<ArachneaProxyCore>,
    input: ControlerStreamInput,
    fallback_direct: bool,
) -> Result<ControlerStreamOutput, String> {
    let inherited_search = crate::core::ProxySearchContext::current();
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(
        async move {
            let request = Box::pin(handle_proxy_http_inner(proxy_core, input, fallback_direct));
            if let Some(search) = inherited_search {
                search.scope(request).await
            } else {
                request.await
            }
        }
        .instrument(tracing::Span::current())
        .with_current_subscriber(),
    );
    match tasks.join_next().await {
        Some(Ok(result)) => result,
        failure => {
            tracing::error!(
                panicked = failure.as_ref().is_some_and(|result| {
                    result.as_ref().err().is_some_and(|error| error.is_panic())
                }),
                "proxy HTTP request task failed"
            );
            Ok(stream_error(502, "Proxy request task failed"))
        }
    }
}

/// Runs the media request off the controller polling stack, keeping its original context.
///
/// # Arguments
/// - `proxy_core`: Shared routing core for this request.
/// - `input`: Owned controller request, including its streaming policy.
/// - `fallback_direct`: Existing default for direct fallback.
async fn handle_proxy_http_inner(
    proxy_core: Arc<ArachneaProxyCore>,
    input: ControlerStreamInput,
    fallback_direct: bool,
) -> Result<ControlerStreamOutput, String> {
    let proxy_core = Arc::new((*proxy_core).clone().with_fallback_direct(fallback_direct));
    let prepared = match super::prepare_proxy_http_source(&proxy_core, &input) {
        Ok(prepared) => prepared,
        Err((status, message)) => return Ok(stream_error(status, message)),
    };
    let method = prepared.request.method.clone();
    let target_url_str = prepared.request.url.clone();
    let opts_encoded = prepared.opts_encoded.clone();
    let redirect_opts = prepared.opts;
    let redirect_actions = prepared.redirect_actions;
    let request_proxy_core = if prepared
        .request
        .client_context
        .get_string("countries")
        .is_some_and(|value| !value.is_empty())
        || prepared
            .request
            .client_context
            .get_string("country")
            .is_some_and(|value| !value.is_empty())
    {
        proxy_core.with_dynamic_proxy_observation_scope()
    } else {
        (*proxy_core).clone()
    };
    let client = SimpleHttpClient::new(request_proxy_core);
    let proxy_request = prepared.request;

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
        search.scope(Box::pin(execute)).await
    } else {
        Box::pin(execute).await
    };
    #[cfg(feature = "performance-diagnostics")]
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
            &opts_encoded,
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
