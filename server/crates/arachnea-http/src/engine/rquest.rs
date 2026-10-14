use async_trait::async_trait;

#[cfg(all(test, feature = "arachnea-proxy"))]
#[path = "rquest_proxy_tests.rs"]
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
    /// Total request timeout, including response body collection.
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
    #[cfg(feature = "arachnea-proxy")]
    pub(crate) async fn send_for_proxy(
        &self,
        request: EngineRequest,
        selection: Option<arachnea_proxy::core::ProxySelectionToken>,
    ) -> Result<EngineResponse, ArachneaHttpError> {
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
        let response = builder
            .send()
            .await
            .map_err(|error| ArachneaHttpError::ProxyAttempt {
                selection: selection.clone(),
                delivery: arachnea_proxy::core::ProxyRequestDelivery::PossiblySent,
                source: Box::new(ArachneaHttpError::Proxy(error.to_string())),
            })?;
        let status = response.status();
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
        let body = response
            .bytes()
            .await
            .map_err(|error| ArachneaHttpError::ProxyAttempt {
                selection,
                delivery: arachnea_proxy::core::ProxyRequestDelivery::ResponseReceived,
                source: Box::new(ArachneaHttpError::Proxy(error.to_string())),
            })?;
        Ok(EngineResponse {
            url,
            status,
            headers,
            body,
        })
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
