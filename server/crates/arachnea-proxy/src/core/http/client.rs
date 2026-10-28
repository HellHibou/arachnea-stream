//! Proxy selection, connection setup, response classification and redirect orchestration.

mod request;
mod response_body;
mod response_head;
mod types;

use response_body::{
    process_response_body, read_buffered_response_body, select_body_reader,
    transfer_encoding_is_chunked,
};
#[cfg(test)]
use response_head::read_headers_only;
use response_head::{
    header_value_case_insensitive, parse_status, remove_header_case_insensitive,
    send_request_and_read_head,
};
pub use types::{ProxiedHttpRequest, ProxiedHttpResponse, ProxiedResponseBody, SimpleHttpResponse};

use crate::core::{
    ArachneaProxyCore, ConnectRequest, Destination, HttpRequestTargetForm, ProxyError, ProxyStream,
    Result,
};
use bytes::Bytes;
use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;

#[cfg(feature = "performance-diagnostics")]
use super::performance_diagnostics::{classified_response, MediaRequestDiagnostics};

/// Minimal HTTP client that uses `ArachneaProxyCore` for connections.
#[derive(Clone)]
pub struct SimpleHttpClient {
    core: ArachneaProxyCore,
    tolerate_dechunked_bodies: bool,
}

impl SimpleHttpClient {
    /// Creates a client backed by the supplied core.
    ///
    /// # Arguments
    ///
    /// - `core`: Proxy core used for outbound HTTP connections.
    ///
    /// # Returns
    ///
    /// Simple HTTP client.
    pub fn new(core: ArachneaProxyCore) -> Self {
        Self {
            core,
            tolerate_dechunked_bodies: true,
        }
    }

    /// Configures tolerance for proxies that remove chunk framing while leaving
    /// the `Transfer-Encoding: chunked` response header in place.
    ///
    /// When enabled (the default), buffered responses fall back to their raw
    /// body when the first line is not a chunk size. Once framing is detected,
    /// truncated or malformed chunks remain errors. Streaming responses remain strict.
    ///
    /// # Arguments
    /// * `enabled` - Whether to tolerate an already-dechunked body despite a chunked transfer header.
    ///
    pub fn with_dechunked_body_tolerance(mut self, enabled: bool) -> Self {
        self.tolerate_dechunked_bodies = enabled;
        self
    }

    /// Performs a simple HTTP/1.1 GET request.
    ///
    /// # Arguments
    ///
    /// - `host`: Target hostname.
    /// - `port`: Target port.
    /// - `path`: Request path, normalized to `/` when not absolute.
    ///
    /// # Returns
    ///
    /// Minimal HTTP response containing status and raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when connection, request writing, response reading, or
    /// status parsing fails.
    pub async fn get(&self, host: &str, port: u16, path: &str) -> Result<SimpleHttpResponse> {
        tracing::debug!(host = %host, port = %port, path = %path, "simple http client sending get request");
        let mut stream = self
            .core
            .connect(
                Destination::host_port(host, port)
                    .with_protocol(crate::core::ApplicationProtocol::Http),
            )
            .await?;
        let path = if path.starts_with('/') { path } else { "/" };
        let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await?;
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).await?;
        let status = parse_status(&bytes)?;
        Ok(SimpleHttpResponse { status, bytes })
    }

    /// Performs a full proxied HTTP request through the Arachnea proxy core.
    ///
    /// When `request.follow_redirects` is set, redirect hops are resolved after
    /// reading their headers and before their body is consumed. This keeps media
    /// redirects usable by custom-scheme desktop clients without preloading the
    /// final response body.
    ///
    /// # Arguments
    ///
    /// - `request`: Full proxied HTTP request specification.
    ///
    /// # Returns
    ///
    /// Proxied HTTP response with parsed status, headers, and body.
    ///
    /// # Errors
    ///
    /// Returns an error when URL parsing, connection, or HTTP I/O fails.
    /// Buffered transfer failures retain the selected route and response-received
    /// delivery evidence so callers can apply destination health and replay policy.
    pub async fn request_proxied(
        &self,
        request: ProxiedHttpRequest,
    ) -> Result<ProxiedHttpResponse> {
        if crate::core::ProxySearchContext::current().is_none()
            && request
                .client_context
                .get_string("countries")
                .is_some_and(|countries| !countries.is_empty() && countries != "[]")
        {
            let search = crate::core::ProxySearchContext::default();
            return search.scope(self.request_proxied_scoped(request)).await;
        }
        self.request_proxied_scoped(request).await
    }

    /// Executes an outbound request and follows its configured redirect budget.
    ///
    /// # Arguments
    /// * `request` - Outbound request, including its destination and routing options.
    async fn request_proxied_scoped(
        &self,
        mut request: ProxiedHttpRequest,
    ) -> Result<ProxiedHttpResponse> {
        let redirect_limit = request.follow_redirects;
        let requested_url = request.url.clone();
        let mut redirect_urls = Vec::new();
        for redirect_index in 0..redirect_limit.unwrap_or_default() {
            let response = self
                .request_proxied_once(request.clone(), &requested_url, &redirect_urls)
                .await?;
            if matches!(
                response.response_decision,
                super::response_classifier::ProxyResponseDecision::RejectedForDestination { .. }
            ) {
                return Ok(response);
            }
            if !(300..400).contains(&response.status) {
                return Ok(response);
            }
            let Some(location) = header_value_case_insensitive(&response.headers, "location")
            else {
                return Ok(response);
            };
            let next_url = url::Url::parse(&request.url)
                .and_then(|base| base.join(location))
                .map_err(|error| {
                    ProxyError::Protocol(format!(
                        "failed to resolve redirect Location '{}': {}",
                        location, error
                    ))
                })?
                .to_string();
            if let Some(token) = &response.selection_token {
                token.record_redirect(response.status).await;
            }
            tracing::debug!(
                from = %request.url,
                to = %next_url,
                redirect = redirect_index + 1,
                "following proxied HTTP redirect"
            );
            request.url = next_url.clone();
            redirect_urls.push(next_url.clone());
            request.context.target_url = next_url;
        }
        self.request_proxied_once(request, &requested_url, &redirect_urls)
            .await
    }

    /// Executes one redirect hop under the current exact-origin search scope.
    ///
    /// # Arguments
    /// * `request` - Outbound request, including its destination and routing options.
    /// * `requested_url` - Original URL retained in response metadata across redirect hops.
    /// * `redirect_urls` - Redirect targets already followed for this logical request.
    async fn request_proxied_once(
        &self,
        request: ProxiedHttpRequest,
        requested_url: &str,
        redirect_urls: &[String],
    ) -> Result<ProxiedHttpResponse> {
        if let Some(search) = crate::core::ProxySearchContext::current() {
            return search
                .for_url(&request.url)
                .scope(search.origin(Box::pin(self.request_proxied_once_inner(
                    request,
                    requested_url,
                    redirect_urls,
                ))))
                .await?;
        }
        Box::pin(self.request_proxied_once_inner(request, requested_url, redirect_urls)).await
    }

    /// Routes one HTTP attempt, classifies its response and applies configured body transformations.
    ///
    /// # Arguments
    /// * `request` - Outbound request, including its destination and routing options.
    /// * `requested_url` - Original URL retained in response metadata across redirect hops.
    /// * `redirect_urls` - Redirect targets already followed for this logical request.
    async fn request_proxied_once_inner(
        &self,
        request: ProxiedHttpRequest,
        requested_url: &str,
        redirect_urls: &[String],
    ) -> Result<ProxiedHttpResponse> {
        if !request.buffer_response_body && !request.post_actions.is_empty() {
            return Err(ProxyError::Protocol(
                "post-response actions require a buffered response body".to_string(),
            ));
        }

        let url = url::Url::parse(&request.url).map_err(|e| {
            ProxyError::Protocol(format!("invalid target URL '{}': {}", request.url, e))
        })?;

        let host = url.host_str().ok_or_else(|| {
            ProxyError::Protocol(format!("target URL '{}' has no host", request.url))
        })?;
        let port = url.port_or_known_default().ok_or_else(|| {
            ProxyError::Protocol(format!("target URL '{}' has no known port", request.url))
        })?;

        let path = url.path();
        let query = url.query();
        let request_target = match query {
            Some(q) => format!("{}?{}", path, q),
            None => path.to_string(),
        };

        // Determine application protocol
        let protocol = match url.scheme() {
            "https" => crate::core::ApplicationProtocol::Https,
            "http" => crate::core::ApplicationProtocol::Http,
            scheme => {
                return Err(ProxyError::Protocol(format!(
                    "unsupported URL scheme '{}'",
                    scheme
                )));
            }
        };
        let is_https = protocol == crate::core::ApplicationProtocol::Https;

        #[cfg(feature = "performance-diagnostics")]
        let mut diagnostics = MediaRequestDiagnostics::new(
            url.origin().ascii_serialization(),
            &request.method,
            request
                .headers
                .keys()
                .any(|name| name.eq_ignore_ascii_case("range")),
            request.buffer_response_body,
        );
        let destination = Destination::host_port(host, port).with_protocol(protocol);
        let selected = self
            .core
            .select_for_request(
                ConnectRequest::new(destination.clone())
                    .with_client_context(request.client_context.clone()),
            )
            .await?;
        let reuses_destination = selected
            .selection_token
            .as_ref()
            .is_some_and(|token| token.reuses_destination());
        #[cfg(feature = "performance-diagnostics")]
        diagnostics.next_phase("connection");
        let connection_deadline =
            tokio::time::Instant::now() + crate::PROXY_MEDIA_REUSED_CONNECTION_TIMEOUT;
        let connect = selected.core.connect_http_request(
            ConnectRequest::new(destination).with_client_context(request.client_context.clone()),
        );
        let stream = if reuses_destination {
            tokio::time::timeout_at(connection_deadline, connect)
                .await
                .unwrap_or(Err(ProxyError::Timeout(
                    "known proxy connection establishment",
                )))
        } else {
            connect.await
        }
        .map_err(|error| ProxyError::RequestAttempt {
            selection: selected.selection_token.clone(),
            delivery: crate::core::ProxyRequestDelivery::NotSent,
            source: Box::new(error),
        })?;

        #[cfg(feature = "performance-diagnostics")]
        diagnostics.next_phase("request_preparation");
        let request::PreparedRequest {
            bytes: request_to_send,
            send_body,
        } = request::prepare(
            &request,
            host,
            port,
            &request_target,
            &stream.target_form,
            stream.proxy_authorization.as_deref(),
        );

        let metadata = stream.stream.metadata().clone();

        let mut unified_stream =
            if is_https && stream.target_form == HttpRequestTargetForm::OriginForm {
                #[cfg(feature = "performance-diagnostics")]
                diagnostics.next_phase("destination_tls");
                let handshake = crate::core::transport::tls::client_tls(
                    stream.stream,
                    host,
                    if reuses_destination {
                        crate::PROXY_MEDIA_REUSED_ORIGIN_TLS_TIMEOUT
                    } else {
                        crate::PROXY_MEDIA_NEW_ORIGIN_TLS_TIMEOUT
                    },
                    request.verify_tls,
                );
                let tls_stream = if reuses_destination {
                    tokio::time::timeout_at(connection_deadline, handshake)
                        .await
                        .unwrap_or(Err(ProxyError::Timeout(
                            "known proxy connection establishment",
                        )))
                } else {
                    handshake.await
                }
                .map_err(|error| ProxyError::RequestAttempt {
                    selection: selected.selection_token.clone(),
                    delivery: crate::core::ProxyRequestDelivery::NotSent,
                    source: Box::new(error),
                })?;
                if let Some(token) = selected
                    .selection_token
                    .as_ref()
                    .filter(|_| request.verify_tls)
                {
                    token
                        .record_phase(
                            crate::core::ProxyObservationPhase::DestinationTls,
                            true,
                            None,
                        )
                        .await;
                }
                ProxyStream::new(tls_stream, metadata)
            } else {
                stream.stream
            };

        #[cfg(feature = "performance-diagnostics")]
        diagnostics.next_phase("send_and_headers");
        let head = send_request_and_read_head(
            &mut unified_stream,
            &request_to_send,
            &request.body,
            send_body,
        );
        let (status_code, mut headers) = if crate::core::ProxySearchContext::current().is_some() {
            tokio::time::timeout(
                if reuses_destination {
                    crate::PROXY_MEDIA_REUSED_ORIGIN_HEADER_TIMEOUT
                } else {
                    crate::PROXY_MEDIA_NEW_ORIGIN_HEADER_TIMEOUT
                },
                head,
            )
            .await
            .unwrap_or_else(|_| Err(ProxyError::Timeout("media response headers")))
        } else {
            head.await
        }
        .map_err(|error| ProxyError::RequestAttempt {
            selection: selected.selection_token.clone(),
            delivery: crate::core::ProxyRequestDelivery::PossiblySent,
            source: Box::new(error),
        })?;

        #[cfg(feature = "performance-diagnostics")]
        diagnostics.next_phase("response_setup");
        #[cfg(feature = "performance-diagnostics")]
        diagnostics.received_headers(status_code);
        if let Some(token) = &selected.selection_token {
            token.record_http_response(status_code).await;
        }

        use super::response_classifier::*;
        let classifier_headers: http::HeaderMap = headers
            .iter()
            .filter_map(|(name, value)| {
                Some((
                    http::header::HeaderName::from_bytes(name.as_bytes()).ok()?,
                    http::HeaderValue::from_str(value).ok()?,
                ))
            })
            .collect();
        let stream: ProxyResponseStream =
            if request.headers_only || matches!(status_code, 204 | 304) {
                Box::pin(futures::stream::empty())
            } else if request.buffer_response_body
                && self.tolerate_dechunked_bodies
                && !matches!(
                    request
                        .response_classifier
                        .as_ref()
                        .map(|policy| policy.0.body_requirement()),
                    Some(ProxyBodyRequirement::Prefix(_))
                )
            {
                let headers = headers.clone();
                let tolerate = self.tolerate_dechunked_bodies;
                Box::pin(futures::stream::once(async move {
                    read_buffered_response_body(
                        unified_stream,
                        &headers,
                        transfer_encoding_is_chunked(&headers),
                        tolerate,
                    )
                    .await
                    .map(Bytes::from)
                    .map_err(std::io::Error::other)
                }))
            } else {
                Box::pin(ReaderStream::new(select_body_reader(
                    unified_stream,
                    &headers,
                )))
            };
        #[cfg(feature = "performance-diagnostics")]
        diagnostics.next_phase("body");
        #[cfg(feature = "performance-diagnostics")]
        let media_request_id = diagnostics.id();
        #[cfg(feature = "performance-diagnostics")]
        let stream: ProxyResponseStream = diagnostics.observe(stream);
        #[cfg(feature = "performance-diagnostics")]
        let classification_started = std::time::Instant::now();
        let (response_decision, mut inspected_stream) = inspect_proxy_response(
            request.response_classifier.as_ref(),
            ProxyResponseView {
                method: &request.method,
                requested_url,
                effective_url: &request.url,
                redirect_urls,
                status: status_code,
                headers: &classifier_headers,
                selection: selected.selection_token.as_ref(),
                body: &[],
                body_state: ProxyBodyState::Unavailable,
            },
            stream,
            crate::core::ProxySearchContext::current()
                .map(|search| {
                    search
                        .origin_remaining()
                        .min(PROXY_RESPONSE_INSPECTION_TIMEOUT)
                })
                .unwrap_or(PROXY_RESPONSE_INSPECTION_TIMEOUT),
        )
        .await;
        #[cfg(feature = "performance-diagnostics")]
        classified_response(media_request_id, &response_decision, classification_started);
        if matches!(response_decision, ProxyResponseDecision::Accepted)
            && !(300..400).contains(&status_code)
            && (!request.buffer_response_body || request.headers_only)
        {
            if let Some(search) = crate::core::ProxySearchContext::current() {
                search.accept();
            }
            if let Some(token) = &selected.selection_token {
                token.record_accepted().await;
            }
        }
        let retry_rejection =
            matches!(
                response_decision,
                ProxyResponseDecision::RejectedForDestination { .. }
            ) && crate::core::ProxySearchContext::current().is_some_and(|search| {
                search.can_start()
                    && crate::core::proxy_retry_allowed(
                        &request.method,
                        search.replayable(),
                        crate::core::ProxyRequestDelivery::ResponseReceived,
                        true,
                    )
            });
        let body = if retry_rejection || request.headers_only || (300..400).contains(&status_code) {
            ProxiedResponseBody::Buffered(Vec::new())
        } else {
            let is_chunked = transfer_encoding_is_chunked(&headers);
            if request.buffer_response_body {
                let mut body = Vec::new();
                let collect = async {
                    while let Some(chunk) = inspected_stream.next().await {
                        body.extend_from_slice(&chunk?);
                    }
                    Ok::<_, std::io::Error>(())
                };
                let collected = if crate::core::ProxySearchContext::current().is_some() {
                    tokio::time::timeout(crate::PROXY_MEDIA_BUFFERED_BODY_TIMEOUT, collect)
                        .await
                        .map_err(|_| ProxyError::Timeout("media response body transfer"))
                        .and_then(|result| result.map_err(ProxyError::from))
                } else {
                    collect.await.map_err(ProxyError::from)
                };
                collected.map_err(|error| ProxyError::RequestAttempt {
                    selection: selected.selection_token.clone(),
                    delivery: crate::core::ProxyRequestDelivery::ResponseReceived,
                    source: Box::new(error),
                })?;
                if matches!(response_decision, ProxyResponseDecision::Accepted) {
                    if let Some(search) = crate::core::ProxySearchContext::current() {
                        search.accept();
                    }
                    if let Some(token) = &selected.selection_token {
                        token.record_accepted().await;
                    }
                }
                if is_chunked {
                    remove_header_case_insensitive(&mut headers, "transfer-encoding");
                    remove_header_case_insensitive(&mut headers, "content-length");
                }
                ProxiedResponseBody::Buffered(process_response_body(
                    body,
                    &mut headers,
                    status_code,
                    &request.post_actions,
                    &request.context,
                )?)
            } else {
                if is_chunked {
                    remove_header_case_insensitive(&mut headers, "transfer-encoding");
                    remove_header_case_insensitive(&mut headers, "content-length");
                }
                ProxiedResponseBody::Streamed(inspected_stream)
            }
        };

        Ok(ProxiedHttpResponse {
            response_decision,
            selection_token: selected.selection_token,
            status: status_code,
            headers,
            body,
        })
    }
}

#[cfg(test)]
#[path = "../../../tests/core/http/client.rs"]
mod tests;
