use std::sync::Arc;

use async_trait::async_trait;
use ghostwire::{Ghostwire, UserAgentOptions};
use http::HeaderValue;
use tokio::sync::Mutex;

use crate::{
    config::ArachneaHttpConfig,
    engine::{EngineRequest, EngineResponse, HttpEngine, SOLVER_USER_AGENT_HEADER},
    error::ArachneaHttpError,
};

/// Registry name for the Ghostwire engine.
pub const ENGINE_NAME: &str = "ghostwire";

/// Ghostwire engine adapter for Cloudflare challenge handling.
#[derive(Clone)]
pub struct GhostwireEngine {
    /// Ghostwire mutates internal state, so access is serialized through a mutex.
    client: Arc<Mutex<Ghostwire>>,
    /// User-agent configured for Ghostwire and mirrored into the fast path.
    user_agent: String,
}

impl GhostwireEngine {
    /// Creates a Ghostwire engine.
    ///
    /// # Parameters
    ///
    /// - `config`: Client configuration used to choose Ghostwire's user-agent.
    ///
    /// # Returns
    ///
    /// A configured Ghostwire adapter.
    ///
    /// # Errors
    ///
    /// Returns `GhostwireFailure` when Ghostwire cannot be constructed.
    pub fn new(config: &ArachneaHttpConfig) -> Result<Self, ArachneaHttpError> {
        let user_agent = config.user_agent_profile.user_agent().to_string();
        let client = Ghostwire::builder()
            .user_agent_opts(UserAgentOptions {
                custom: Some(user_agent.clone()),
                desktop: true,
                mobile: false,
                ..Default::default()
            })
            .build()
            .map_err(|err| ArachneaHttpError::GhostwireFailure(err.to_string()))?;
        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            user_agent,
        })
    }
}

#[async_trait]
impl HttpEngine for GhostwireEngine {
    /// Returns the Ghostwire engine name.
    ///
    /// # Returns
    ///
    /// The static engine name.
    fn name(&self) -> &'static str {
        ENGINE_NAME
    }

    /// Sends a request with Ghostwire.
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
    /// Returns `GhostwireFailure` when Ghostwire cannot execute or solve the
    /// request.
    async fn send(&self, request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError> {
        let request_url = request.url.clone();
        let mut client = self.client.lock().await;
        let opts = ghostwire::RequestOptions {
            headers: Some(request.headers),
            body_bytes: request.body,
            timeout: None,
            follow_redirects: None,
            ..Default::default()
        };
        let response = client
            .request(request.method, &request_url, opts)
            .await
            .map_err(|err| ArachneaHttpError::GhostwireFailure(err.to_string()))?;

        let status = response.status();
        let headers = response.headers().clone();
        let final_url = response.url();
        let url = if final_url.host_str() == Some("no.url.provided.local") {
            request_url
        } else {
            final_url.to_string()
        };
        let body = response
            .bytes()
            .await
            .map_err(|err| ArachneaHttpError::GhostwireFailure(err.to_string()))?;

        Ok(EngineResponse {
            url,
            status,
            headers,
            body,
        })
    }

    /// Refreshes Cloudflare state and records Ghostwire's configured user-agent.
    ///
    /// # Parameters
    ///
    /// - `request`: Normalized refresh request.
    ///
    /// # Returns
    ///
    /// A normalized response with cookies and solver user-agent metadata.
    ///
    /// # Errors
    ///
    /// Returns `GhostwireFailure` when Ghostwire cannot execute or solve the
    /// request, or `InvalidHeader` when metadata cannot be represented as a
    /// header.
    async fn refresh_cloudflare(
        &self,
        request: EngineRequest,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        let mut response = self.send(request).await?;
        response.headers.insert(
            SOLVER_USER_AGENT_HEADER,
            HeaderValue::from_str(&self.user_agent)
                .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
        );
        Ok(response)
    }
}
