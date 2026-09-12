use async_trait::async_trait;

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
}

impl RquestEngine {
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
        Ok(Self { client, uses_proxy })
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
        let mut builder = self.client.request(request.method, &request.url);
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
        let url = response.uri().to_string();
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
