use async_trait::async_trait;

#[cfg(all(test, feature = "arachnea-proxy"))]
#[path = "../../tests/engine/rquest.rs"]
mod proxy_tests;

use crate::{
    config::ArachneaHttpConfig,
    engine::{EngineRequest, EngineResponse, HttpEngine},
    error::ArachneaHttpError,
};

/// `rquest` engine adapter for the fast HTTP path.
#[derive(Clone)]
pub(crate) struct RquestEngine {
    /// Reused underlying rquest client.
    client: rquest::Client,
    /// Whether this client routes through a configured proxy transport.
    uses_proxy: bool,
    /// Ordinary total timeout; contextual headers and accepted bodies use it separately.
    request_timeout: std::time::Duration,
}

impl RquestEngine {
    /// Executes a correlated route and observes headers before body collection.
    ///
    /// # Arguments
    /// - `request`: Business request sent only through the pinned client.
    /// - `selection`: Exact route correlation for this attempt.
    ///
    /// # Errors
    /// Returns an attributed failure with conservative delivery evidence.
    ///
    /// # Returns
    /// Materialized response, unchanged business decision and technical permission
    /// to retain the transport if the redirect loop subsequently confirms following.
    #[cfg(feature = "arachnea-proxy")]
    pub(crate) async fn send_for_proxy(
        &self,
        request: EngineRequest,
        selection: Option<arachnea_proxy::core::ProxySelectionToken>,
        policy: Option<&arachnea_proxy::core::http::response_classifier::ProxyResponsePolicy>,
        requested_url: &str,
        redirect_urls: &[String],
    ) -> Result<
        (
            EngineResponse,
            arachnea_proxy::core::http::response_classifier::ProxyResponseDecision,
            bool,
        ),
        ArachneaHttpError,
    > {
        use arachnea_proxy::core::http::response_classifier::*;
        use futures::StreamExt;
        let started = tokio::time::Instant::now();
        let method = request.method.clone();
        let search = arachnea_proxy::core::ProxySearchContext::current();
        let header_timeout = if search.is_some()
            && selection
                .as_ref()
                .is_some_and(|token| token.reuses_destination())
        {
            self.request_timeout
                .min(crate::HTTP_RQUEST_PROXY_HEADER_TIMEOUT_MAX)
        } else {
            self.request_timeout
        };
        let mut builder = self.client.request(request.method, &request.url);
        if search.is_none() {
            builder = builder.timeout(self.request_timeout);
        }
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        use arachnea_proxy::core::ProxyRequestDelivery;
        let failure = |delivery, reason| ArachneaHttpError::ProxyAttempt {
            selection: selection.clone(),
            delivery,
            source: Box::new(ArachneaHttpError::Proxy(reason)),
        };
        let send = async {
            tokio::time::timeout(header_timeout, builder.send())
                .await
                .map_err(|_| {
                    tracing::debug!(
                        search_id = search.as_ref().map(|search| search.id()),
                        proxy = selection.as_ref().map(|token| token.endpoint()),
                        failure_kind = "timeout",
                        phase = "response_headers",
                        timeout_ms = header_timeout.as_millis(),
                        "localized proxy response headers timed out; delivery remains uncertain"
                    );
                    failure(
                        ProxyRequestDelivery::PossiblySent,
                        "origin response headers timed out".into(),
                    )
                })?
                .map_err(|error| {
                    let delivery = if error.is_connect() && redirect_urls.is_empty() {
                        ProxyRequestDelivery::NotSent
                    } else {
                        ProxyRequestDelivery::PossiblySent
                    };
                    // Only the connector classification proves absence of sending.
                    // Source text is inspected for diagnostic categories, never replay decisions.
                    let mut kind = if error.is_timeout() {
                        "timeout"
                    } else if error.is_connect() {
                        "connection"
                    } else {
                        "request"
                    };
                    let mut source = std::error::Error::source(&error);
                    for _ in 0..8 {
                        let Some(cause) = source else {
                            break;
                        };
                        let description = cause.to_string().to_ascii_lowercase();
                        if description.contains("certificate") {
                            kind = "tls_certificate";
                            break;
                        }
                        if description.contains("ssl") || description.contains("tls") {
                            kind = "tls_handshake";
                        } else if description.contains("tunnel") && kind != "tls_handshake" {
                            kind = "proxy_tunnel";
                        }
                        source = cause.source();
                    }
                    tracing::debug!(
                        search_id = search.as_ref().map(|search| search.id()),
                        proxy = selection.as_ref().map(|token| token.endpoint()),
                        failure_kind = kind,
                        ?delivery,
                        "localized proxy send failed"
                    );
                    failure(delivery, format!("{kind}: {}", error.without_url()))
                })
        };
        let response = if let Some(search) = &search {
            search
                .origin(send)
                .await
                .map_err(|error| failure(ProxyRequestDelivery::PossiblySent, error.to_string()))
                .and_then(|result| result)
        } else {
            send.await
        }?;
        let status = response.status();
        #[cfg(feature = "performance-diagnostics")]
        tracing::debug!(
            search_id = search.as_ref().map(|search| search.id()),
            destination = selection
                .as_ref()
                .map(|token| token.destination().to_string()),
            status = status.as_u16(),
            duration_ms = started.elapsed().as_millis(),
            "received localized proxy HTTP response headers"
        );
        let headers = response.headers().clone();
        let url = response.url().to_string();
        if let Some(token) = &selection {
            if url.starts_with("https:") {
                token
                    .record_phase(
                        arachnea_proxy::core::ProxyObservationPhase::DestinationTls,
                        true,
                        None,
                    )
                    .await;
            }
            token.record_http_response(status.as_u16()).await;
        }
        let stream: ProxyResponseStream = Box::pin(
            response
                .bytes_stream()
                .map(|result| result.map_err(std::io::Error::other)),
        );
        #[cfg(feature = "performance-diagnostics")]
        let inspection_started = tokio::time::Instant::now();
        let view = ProxyResponseView {
            method: method.as_str(),
            requested_url,
            effective_url: &url,
            redirect_urls,
            status: status.as_u16(),
            headers: &headers,
            selection: selection.as_ref(),
            body: &[],
            body_state: ProxyBodyState::Unavailable,
        };
        let historical_requirement = policy
            .map(|policy| policy.0.body_requirement())
            .unwrap_or(ProxyBodyRequirement::None);
        let requirement = policy
            .map(|policy| policy.0.body_requirement_for_response(&view))
            .unwrap_or(ProxyBodyRequirement::None);
        let inspection_timeout = search
            .as_ref()
            .map(|search| search.origin_remaining())
            .unwrap_or_else(|| self.request_timeout.saturating_sub(started.elapsed()));
        let deferred_prefix = match (historical_requirement, requirement) {
            (ProxyBodyRequirement::Prefix(bytes), ProxyBodyRequirement::None)
                if bytes > 0
                    && method != http::Method::HEAD
                    && status.as_u16() >= 200
                    && !matches!(status.as_u16(), 204 | 304) =>
            {
                Some((
                    bytes.min(MAX_PROXY_RESPONSE_PREFIX),
                    inspection_timeout.min(PROXY_RESPONSE_INSPECTION_TIMEOUT),
                ))
            }
            _ => None,
        };
        let inspection = inspect_proxy_response_with_requirement(
            policy,
            view,
            stream,
            inspection_timeout,
            requirement,
        );
        let (decision, stream, allow_redirect_transport) = if let Some(search) = &search {
            search.origin(inspection).await.map_err(|error| {
                if let Some(token) = &selection {
                    token.finish_discovery();
                }
                ArachneaHttpError::ProxyAttempt {
                    selection: selection.clone(),
                    delivery: arachnea_proxy::core::ProxyRequestDelivery::ResponseReceived,
                    source: Box::new(ArachneaHttpError::Proxy(error.to_string())),
                }
            })?
        } else {
            inspection.await
        };
        #[cfg(feature = "performance-diagnostics")]
        tracing::debug!(
            search_id = search.as_ref().map(|search| search.id()),
            destination = selection
                .as_ref()
                .map(|token| token.destination().to_string()),
            decision = match &decision {
                ProxyResponseDecision::Accepted => "accepted",
                ProxyResponseDecision::RejectedForDestination { .. } => "rejected_for_destination",
                ProxyResponseDecision::Inconclusive => "inconclusive",
            },
            duration_ms = inspection_started.elapsed().as_millis(),
            deferred_prefix_bytes = deferred_prefix.map(|(bytes, _)| bytes),
            deferred_prefix_ms = deferred_prefix.map(|(_, timeout)| timeout.as_millis()),
            "classified localized proxy HTTP response"
        );
        let accepted =
            matches!(decision, ProxyResponseDecision::Accepted) && !status.is_redirection();
        if accepted {
            if let Some(search) = &search {
                search.accept();
            }
            if let Some(token) = &selection {
                token.record_accepted().await;
            }
        }
        let retry_rejection = matches!(
            decision,
            ProxyResponseDecision::RejectedForDestination { .. }
        ) && search.as_ref().is_some_and(|search| {
            search.can_start()
                && arachnea_proxy::core::proxy_retry_allowed(
                    method.as_str(),
                    search.replayable(),
                    arachnea_proxy::core::ProxyRequestDelivery::ResponseReceived,
                    true,
                )
        });
        let transfer_timeout = if accepted {
            self.request_timeout
        } else {
            search
                .as_ref()
                .map(|s| s.origin_remaining().min(self.request_timeout))
                .unwrap_or(self.request_timeout)
        };
        #[cfg(feature = "performance-diagnostics")]
        let transfer_started = tokio::time::Instant::now();
        let body = Box::pin(collect_proxy_body(
            stream,
            retry_rejection,
            transfer_timeout,
            deferred_prefix,
            if accepted {
                None
            } else {
                search
                    .as_ref()
                    .map(|search| tokio::time::Instant::now() + search.origin_remaining())
            },
        ))
        .await
        .map_err(|error| {
            if let Some(token) = &selection {
                token.finish_discovery();
            }
            ArachneaHttpError::ProxyAttempt {
                selection: selection.clone(),
                delivery: arachnea_proxy::core::ProxyRequestDelivery::ResponseReceived,
                source: Box::new(ArachneaHttpError::Proxy(error.to_string())),
            }
        })?;
        #[cfg(feature = "performance-diagnostics")]
        tracing::debug!(
            search_id = search.as_ref().map(|search| search.id()),
            destination = selection
                .as_ref()
                .map(|token| token.destination().to_string()),
            body_bytes = body.len(),
            body_skipped = retry_rejection,
            duration_ms = transfer_started.elapsed().as_millis(),
            total_ms = started.elapsed().as_millis(),
            "collected localized proxy HTTP response body"
        );
        Ok((
            EngineResponse {
                url,
                status,
                headers,
                body: body.into(),
            },
            decision,
            allow_redirect_transport,
        ))
    }

    /// Creates a configured `rquest` engine.
    ///
    /// # Parameters
    ///
    /// - `config`: Client configuration.
    ///
    /// # Returns
    ///
    /// A configured engine adapter.
    ///
    /// # Errors
    ///
    /// Returns `Network` when the underlying client cannot be built.
    pub(crate) fn new(
        config: &ArachneaHttpConfig,
        proxy_url: Option<&str>,
        client_override: Option<rquest::Client>,
    ) -> Result<Self, ArachneaHttpError> {
        if let Some(client) = client_override {
            return Ok(Self {
                client,
                uses_proxy: true,
                request_timeout: config.request_timeout,
            });
        }

        let uses_proxy = proxy_url.is_some();
        let mut builder = rquest::Client::builder()
            .no_proxy()
            .timeout(config.request_timeout)
            .user_agent(config.user_agent_profile.user_agent())
            .redirect(rquest::redirect::Policy::none());

        #[cfg(feature = "arachnea-proxy")]
        if proxy_url.is_some() && arachnea_proxy::core::ProxySearchContext::current().is_some() {
            let search = arachnea_proxy::core::ProxySearchContext::current().unwrap();
            let limit = search.connection_timeout();
            builder = builder.connect_timeout(
                if limit == crate::HTTP_RQUEST_REUSED_ROUTE_CONNECT_ENVELOPE {
                    limit
                } else {
                    config.request_timeout.min(limit)
                },
            );
        }

        if let Some(proxy_url) = proxy_url {
            let proxy = rquest::Proxy::all(proxy_url)
                .map_err(|err| ArachneaHttpError::InvalidConfiguration(err.to_string()))?;
            builder = builder.proxy(proxy);
        }

        let client = builder
            .build()
            .map_err(|err| ArachneaHttpError::Network(err.to_string()))?;
        Ok(Self {
            client,
            uses_proxy,
            request_timeout: config.request_timeout,
        })
    }
}

#[async_trait]
impl HttpEngine for RquestEngine {
    /// Returns the rquest engine name.
    ///
    /// # Returns
    ///
    /// The static engine name.
    fn name(&self) -> &'static str {
        "rquest"
    }

    /// Sends a request with `rquest`.
    ///
    /// # Parameters
    ///
    /// - `request`: Normalized request to execute.
    ///
    /// # Returns
    ///
    /// A normalized response with body bytes collected.
    ///
    /// # Errors
    ///
    /// Returns `Proxy` for failures through a configured proxy transport and
    /// `Network` for direct request execution or body collection failures.
    async fn send(&self, request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError> {
        let mut builder = self
            .client
            .request(request.method, &request.url)
            .timeout(self.request_timeout);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }

        let response = builder.send().await.map_err(|err| {
            if self.uses_proxy {
                ArachneaHttpError::Proxy(err.to_string())
            } else {
                ArachneaHttpError::Network(err.to_string())
            }
        })?;
        let status = response.status();
        let headers = response.headers().clone();
        let url = response.url().to_string();
        let body = response.bytes().await.map_err(|err| {
            if self.uses_proxy {
                ArachneaHttpError::Proxy(err.to_string())
            } else {
                ArachneaHttpError::Network(err.to_string())
            }
        })?;

        Ok(EngineResponse {
            url,
            status,
            headers,
            body,
        })
    }
}

/// Collects once, preserving a skipped inspection's bounded reception window.
/// The ordinary transfer timeout starts after the historical prefix byte/time
/// boundary. EOF finishes immediately; failures and cancellation remain local.
///
/// # Arguments
/// - `stream`: Decoded response body, including any replayed inspection bytes.
/// - `skip`: Whether a retryable rejection must skip collection entirely.
/// - `transfer_timeout`: Ordinary allowance after the initial reception boundary.
/// - `deferred_prefix`: Historical byte/time bounds when preliminary inspection was skipped.
/// - `search_deadline`: Additional absolute cap for a body that was not accepted.
///
/// # Errors
/// Propagates body errors or reports expiration of the applicable transfer deadline.
#[cfg(feature = "arachnea-proxy")]
async fn collect_proxy_body(
    mut stream: arachnea_proxy::core::http::response_classifier::ProxyResponseStream,
    skip: bool,
    transfer_timeout: std::time::Duration,
    deferred_prefix: Option<(usize, std::time::Duration)>,
    search_deadline: Option<tokio::time::Instant>,
) -> std::io::Result<Vec<u8>> {
    use futures::StreamExt;
    let mut body = Vec::new();
    if skip {
        return Ok(body);
    }
    if let Some((bytes, allowance)) = deferred_prefix {
        let deadline = tokio::time::Instant::now() + allowance;
        let deadline = search_deadline.map_or(deadline, |search| deadline.min(search));
        while body.len() < bytes {
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            match tokio::time::timeout_at(deadline, stream.next()).await {
                Err(_) => break,
                Ok(None) => return Ok(body),
                Ok(Some(chunk)) => body.extend_from_slice(&chunk?),
            }
        }
    }
    let transfer_timeout = search_deadline.map_or(transfer_timeout, |deadline| {
        transfer_timeout.min(deadline.saturating_duration_since(tokio::time::Instant::now()))
    });
    tokio::time::timeout(transfer_timeout, async {
        while let Some(chunk) = stream.next().await {
            body.extend_from_slice(&chunk?);
        }
        Ok::<_, std::io::Error>(body)
    })
    .await
    .map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "response body transfer timed out",
        )
    })?
}
