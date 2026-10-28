//! Explicit Cloudflare refresh strategy and engine selection.

use super::*;

impl ArachneaHttpClient {
    /// Forces a Cloudflare refresh for an origin.
    ///
    /// # Arguments
    ///
    /// - `origin`: Origin or URL to refresh.
    ///
    /// # Errors
    ///
    /// Returns Cloudflare solver or URL parsing failures.
    pub async fn refresh_cloudflare(&self, origin: &str) -> Result<(), ArachneaHttpError> {
        self.refresh_cloudflare_for_url(&origin_url(origin)?, None)
            .await
    }

    /// Forces a fresh Cloudflare refresh using the smart solver when available,
    /// otherwise the browser solver.
    ///
    /// # Arguments
    ///
    /// - `url`: Absolute URL used to obtain or refresh Cloudflare cookies.
    /// - `extra_headers`: Optional headers forwarded to the solver request.
    ///
    /// # Errors
    ///
    /// Returns `CloudflareSolverUnavailable` when no solver is configured, or
    /// URL parsing and solver failures.
    pub(super) async fn refresh_cloudflare_for_url(
        &self,
        url: &str,
        extra_headers: Option<&HeaderMap>,
    ) -> Result<(), ArachneaHttpError> {
        if self.smart_cloudflare_engine.is_some() {
            return self
                .refresh_cloudflare_for_url_with_strategy(
                    url,
                    CloudflareRefreshStrategy::Smart,
                    true,
                    extra_headers,
                )
                .await;
        }
        self.refresh_cloudflare_for_url_with_strategy(
            url,
            CloudflareRefreshStrategy::Browser,
            true,
            extra_headers,
        )
        .await
    }

    /// Forces a Cloudflare refresh using a selected solver path.
    ///
    /// # Arguments
    ///
    /// - `url`: Absolute URL used to obtain or refresh Cloudflare cookies.
    /// - `strategy`: Solver class selected by the request mode.
    /// - `fresh`: Whether engine-specific caches must be bypassed.
    /// - `extra_headers`: Optional request headers that must also be visible to
    ///   the solver request, for example `Referer`.
    ///
    /// # Errors
    ///
    /// Returns Cloudflare solver or URL parsing failures.
    pub(super) async fn refresh_cloudflare_for_url_with_strategy(
        &self,
        url: &str,
        strategy: CloudflareRefreshStrategy,
        fresh: bool,
        extra_headers: Option<&HeaderMap>,
    ) -> Result<(), ArachneaHttpError> {
        Url::parse(url).map_err(|err| ArachneaHttpError::InvalidUrl(err.to_string()))?;
        let engine = self
            .cloudflare_engine_for_strategy(strategy)
            .ok_or(ArachneaHttpError::CloudflareSolverUnavailable)?;
        debug!(target: "arachnea_http::client",
            origin = %url,
            engine = engine.name(),
            strategy = strategy.name(),
            fresh,
            "refreshing Cloudflare cookies"
        );
        let user_agent = self.cloudflare_user_agent_for_url(url).await?;
        let mut headers = self.base_headers(None, user_agent.as_deref())?;
        if let Some(extra_headers) = extra_headers {
            headers.extend(extra_headers.clone());
        }
        let request = EngineRequest {
            method: Method::GET,
            url: url.to_string(),
            headers,
            body: None,
        };
        #[cfg(feature = "arachnea-proxy")]
        if arachnea_proxy::core::ProxySearchContext::current().is_some() {
            return Err(ArachneaHttpError::UnsupportedEngineOperation {
                engine: engine.name(),
                operation: "correlated localized proxy search",
            });
        }
        let response = if fresh {
            engine.refresh_cloudflare_fresh(request).await?
        } else {
            engine.refresh_cloudflare(request).await?
        };
        self.store_cloudflare_solver_metadata(&response.url, &response.headers)
            .await?;
        self.store_response_cookies(&response.url, &response.headers)
            .await?;
        Ok(())
    }

    /// Returns the configured engine for a Cloudflare refresh strategy.
    ///
    /// # Arguments
    ///
    /// - `strategy`: Solver class selected by the request mode.
    ///
    /// # Returns
    ///
    /// The configured engine for the strategy, when available.
    pub(super) fn cloudflare_engine_for_strategy(
        &self,
        strategy: CloudflareRefreshStrategy,
    ) -> Option<&DynHttpEngine> {
        match strategy {
            CloudflareRefreshStrategy::Smart => self.smart_cloudflare_engine.as_ref(),
            CloudflareRefreshStrategy::Browser => self.browser_cloudflare_engine.as_ref(),
        }
    }
}

#[cfg(test)]
#[path = "../../tests/client/cloudflare.rs"]
mod tests;
