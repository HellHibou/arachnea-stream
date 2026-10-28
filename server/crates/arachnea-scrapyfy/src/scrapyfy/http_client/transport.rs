//! Lazy HTTP transport construction and routing-version cache invalidation.

use anyhow::Result;
use arachnea_http::{ArachneaHttpClient, ArachneaHttpConfig};
use std::sync::Arc;

use super::HttpClient;

#[derive(Clone)]
/// Cached transport tagged with the routing and local-country versions used to construct it.
pub(super) struct CachedHttpClient {
    proxy_version: u64,
    local_country_version: u64,
    client: Arc<ArachneaHttpClient>,
}

impl HttpClient {
    /// Lazily initializes and returns the underlying [`ArachneaHttpClient`].
    pub(super) async fn http_client(&self) -> Result<Arc<ArachneaHttpClient>> {
        #[cfg(feature = "arachnea-proxy")]
        if let Some(search) = arachnea_proxy::core::ProxySearchContext::current() {
            return search.technical(Box::pin(self.http_client_inner())).await?;
        }
        Box::pin(self.http_client_inner()).await
    }

    /// Reuses the cached transport while routing versions match, otherwise constructs and caches a replacement.
    ///
    /// # Errors
    ///
    /// Returns errors from invalid HTTP/proxy policies, unavailable engine initialization or client construction.
    async fn http_client_inner(&self) -> Result<Arc<ArachneaHttpClient>> {
        let proxy_state = self.proxy_handle.snapshot();
        let local_country_state = self.local_country.snapshot();

        {
            let guard = self.client.read().await;
            if let Some(cached) = guard.as_ref() {
                if cached.proxy_version == proxy_state.version
                    && cached.local_country_version == local_country_state.version
                {
                    return Ok(cached.client.clone());
                }
            }
        }

        let proxy_countries = self.http_config.proxy_countries_hint();
        let request_timeout = crate::SCRAPER_HTTP_REQUEST_TIMEOUT;
        let mut builder = ArachneaHttpConfig::builder()
            .default_request_mode(self.http_config.request_mode())
            .user_agent_profile(self.http_config.browser_profile())
            .request_timeout(request_timeout)
            .proxy_retry_replayable(self.http_config.proxy_retry_replayable.unwrap_or(false))
            .max_redirects(self.http_config.max_redirects);
        #[cfg(feature = "arachnea-proxy")]
        {
            use arachnea_proxy::core::http::response_classifier::resolve_response_policy;
            let policy = match &self.response_classifier {
                Some(policy) => policy.clone(),
                None => resolve_response_policy(
                    self.http_config.proxy_response_classifier.as_ref(),
                    &self.http_config.proxy_rejection_statuses,
                )?,
            };
            builder = builder
                .proxy_response_classifier(policy)
                .proxy_runtime(self.proxy_handle.proxy_runtime())
                .proxy_transport_pool(self.proxy_handle.transports.clone());
        }
        let proxy_transport_configured = proxy_state.proxy.is_some();
        if let Some(proxy) = proxy_state.proxy {
            builder = builder.proxy(proxy);
        }
        if !proxy_countries.is_empty() {
            tracing::debug!(target: "arachnea_scrapyfy::scrapyfy::http_client",
                requested_proxy_countries = ?proxy_countries,
                proxy_transport_configured,
                request_timeout_seconds = request_timeout.as_secs(),
                "forwarding country hints to HTTP proxy transport"
            );
            builder = builder.proxy_parameter(
                "countries",
                serde_json::to_string(&proxy_countries).expect("country list serializes"),
            );
        }
        if let Some(proxy_affinity) = self
            .http_config
            .proxy_affinity
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            builder = builder.proxy_parameter("proxy_affinity", proxy_affinity.to_string());
        }
        if !self.http_config.proxy_domain_group.is_empty() {
            builder = builder.proxy_parameter(
                "proxy_domain_group",
                serde_json::to_string(&self.http_config.proxy_domain_group)
                    .expect("domain group serializes"),
            );
        }
        let config = builder.build()?;
        let client = Arc::new(
            ArachneaHttpClient::new_with_cookie_cache_browser_session_manager_and_session_store(
                config,
                self.cookie_cache.clone(),
                self.browser_session_manager.clone(),
                self.session_store.clone(),
            )
            .await?,
        );

        let mut guard = self.client.write().await;
        *guard = Some(CachedHttpClient {
            proxy_version: proxy_state.version,
            local_country_version: local_country_state.version,
            client: client.clone(),
        });
        Ok(client)
    }
}
