use anyhow::{Context, Result};
use arachnea_core::persistence::TypedEntityStore;
use arachnea_http::chaser_session::{memory_session_store, CachedChaserSession};
use arachnea_http::{
    global_cookie_cache, header_map_from_strings, ArachneaHttpClient, ArachneaHttpConfig,
    ArachneaResponse, BrowserProfile, BrowserSessionConfig, BrowserSessionManager, CookieEntry,
    HttpProxyConfig, HttpRequestMode, PageClickRequest, PageFetchRequest, PageNavigationRequest,
    SharedCookieCache,
};
#[cfg(feature = "arachnea-proxy")]
use arachnea_proxy::core::{ArachneaProxyCore, UsageProfile};
use http::{HeaderMap, Method};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use tokio::sync::RwLock as AsyncRwLock;
use tracing::{error, trace};
use url::Url;

use crate::scrapyfy::{query_helpers, SharedLocalCountry};

/// Optional mock router used in tests to intercept outgoing HTTP calls.
pub type RouterFn = Box<dyn Fn(&HttpClient, &str) -> Result<String> + Send + Sync + 'static>;

/// Global optional mock router installed by [`set_router`] for testing.
static HTTP_CLIENT_ROUTER: OnceLock<Mutex<Option<RouterFn>>> = OnceLock::new();

/// Compiled regex used to extract the `__NEXT_DATA__` JSON payload from HTML pages.
static NEXT_DATA_REGEX: OnceLock<Regex> = OnceLock::new();

use crate::HTTP_INVALID_JSON_LOG_PREVIEW_LIMIT as INVALID_JSON_LOG_PREVIEW_LIMIT;

/// Mutable proxy transport shared by one scraper instance and its derived HTTP clients.
#[derive(Clone, Default)]
pub struct SharedProxyConfigHandle {
    inner: Arc<RwLock<ProxyConfigState>>,
    #[cfg(feature = "arachnea-proxy")]
    runtime: Arc<arachnea_proxy::server::LoopbackProxyRuntime>,
    #[cfg(feature = "arachnea-proxy")]
    transports: Arc<arachnea_http::client::ProxyTransportPool>,
}

#[derive(Clone, Default)]
/// Versioned proxy configuration used to invalidate derived transports and manage the default route.
struct ProxyConfigState {
    version: u64,
    proxy: Option<HttpProxyConfig>,
    #[cfg(feature = "arachnea-proxy")]
    default_route: Option<Arc<arachnea_proxy::server::LoopbackRoute>>,
}

#[derive(Clone)]
/// Cached transport tagged with the routing and local-country versions used to construct it.
struct CachedHttpClient {
    proxy_version: u64,
    local_country_version: u64,
    client: Arc<ArachneaHttpClient>,
}

impl SharedProxyConfigHandle {
    /// Creates a handle without an active proxy override.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an independent routing configuration sharing an application listener.
    ///
    /// # Arguments
    ///
    /// - `runtime`: Shared loopback listener, including provider bootstrap transports.
    #[cfg(feature = "arachnea-proxy")]
    pub fn with_runtime(runtime: Arc<arachnea_proxy::server::LoopbackProxyRuntime>) -> Self {
        Self {
            inner: Arc::default(),
            runtime,
            transports: Arc::default(),
        }
    }

    /// Creates an independent routing handle sharing this family's listener and idle transports.
    #[cfg(feature = "arachnea-proxy")]
    pub fn with_independent_config(&self) -> Self {
        Self {
            inner: Arc::default(),
            runtime: self.runtime.clone(),
            transports: self.transports.clone(),
        }
    }

    /// Returns the listener shared by clients using this handle.
    #[cfg(feature = "arachnea-proxy")]
    pub fn proxy_runtime(&self) -> Arc<arachnea_proxy::server::LoopbackProxyRuntime> {
        self.runtime.clone()
    }

    /// Publishes the active core for external clients, preserving existing connections.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no active core or the registry is unavailable.
    #[cfg(feature = "arachnea-proxy")]
    pub fn publish_default_route(&self) -> Result<()> {
        let mut state = self
            .inner
            .write()
            .map_err(|_| anyhow::anyhow!("proxy configuration lock poisoned"))?;
        let Some(HttpProxyConfig::Arachnea(core)) = &state.proxy else {
            anyhow::bail!("cannot publish loopback server without an Arachnea proxy core");
        };
        let route = self.runtime.register(
            core.clone(),
            arachnea_proxy::core::ClientContext::new(),
            true,
        )?;
        self.runtime.set_default_route(&route)?;
        state.default_route = Some(route);
        Ok(())
    }

    /// Publishes the current default route and binds the shared server before external use.
    ///
    /// # Errors
    ///
    /// Returns a contextual error if the route cannot be published or the port cannot bind.
    #[cfg(feature = "arachnea-proxy")]
    pub async fn start_proxy_server(&self) -> Result<std::net::SocketAddr> {
        self.publish_default_route()?;
        self.runtime
            .start()
            .await
            .context("Failed to bind shared loopback proxy server")
    }

    /// Replaces the proxy configuration and increments the version.
    ///
    /// # Arguments
    ///
    /// * `proxy` - Proxy transport configuration replacing the current shared override.
    pub fn set_proxy(&self, proxy: HttpProxyConfig) {
        if let Ok(mut guard) = self.inner.write() {
            guard.version = guard.version.saturating_add(1);
            #[cfg(feature = "arachnea-proxy")]
            if guard.default_route.is_some() {
                if let HttpProxyConfig::Arachnea(core) = &proxy {
                    match self
                        .runtime
                        .register(
                            core.clone(),
                            arachnea_proxy::core::ClientContext::new(),
                            true,
                        )
                        .and_then(|route| {
                            self.runtime.set_default_route(&route)?;
                            Ok(route)
                        }) {
                        Ok(route) => guard.default_route = Some(route),
                        Err(error) => {
                            if let Some(route) = guard.default_route.take() {
                                let _ = self.runtime.clear_default_route(&route);
                            }
                            tracing::error!(%error, "failed to replace external loopback route");
                        }
                    }
                } else if let Some(route) = guard.default_route.take() {
                    let _ = self.runtime.clear_default_route(&route);
                }
            }
            guard.proxy = Some(proxy);
        }
    }

    /// Removes the proxy override and increments the version.
    pub fn clear_proxy(&self) {
        if let Ok(mut guard) = self.inner.write() {
            guard.version = guard.version.saturating_add(1);
            guard.proxy = None;
            #[cfg(feature = "arachnea-proxy")]
            {
                if let Some(route) = guard.default_route.take() {
                    let _ = self.runtime.clear_default_route(&route);
                }
            }
        }
    }

    /// Installs the default in-process system relay proxy for this handle.
    #[cfg(feature = "arachnea-proxy")]
    pub fn enable_system_proxy(&self) -> Result<()> {
        let proxy_core = ArachneaProxyCore::new(UsageProfile::SystemRelay.config())?;
        self.set_proxy(HttpProxyConfig::Arachnea(proxy_core));
        Ok(())
    }

    /// Returns the current proxy configuration snapshot and version.
    fn snapshot(&self) -> ProxyConfigState {
        self.inner
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

/// Logs a bounded preview of an invalid JSON response for parser diagnostics.
///
/// # Arguments
///
/// * `url` - Absolute request URL used by the HTTP transport.
/// * `payload_kind` - Diagnostic label identifying whether the payload is JSON text or an embedded response.
/// * `payload` - Invalid response text whose bounded preview is included in the diagnostic.
/// * `error` - Underlying error wrapped with the administration operation status.
fn log_invalid_json_payload(
    url: &str,
    payload_kind: &str,
    payload: &str,
    error: &serde_json::Error,
) {
    let truncated = payload.chars().count() > INVALID_JSON_LOG_PREVIEW_LIMIT;
    let payload_preview = if truncated {
        payload
            .chars()
            .take(INVALID_JSON_LOG_PREVIEW_LIMIT)
            .collect::<String>()
    } else {
        payload.to_string()
    };

    error!(
        url = %url,
        payload_kind = payload_kind,
        parse_error = %error,
        payload_truncated = truncated,
        payload = %payload_preview,
        "Invalid JSON payload received"
    );
}

/// Installs a temporary HTTP router override used by integration/unit tests.
///
/// # Arguments
///
/// * `f` - Callback returning a mocked payload for the requested URL.
pub fn set_router<F>(f: F)
where
    F: Fn(&HttpClient, &str) -> Result<String> + Send + Sync + 'static,
{
    let mut router = HTTP_CLIENT_ROUTER
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap();
    *router = Some(Box::new(f));
}

/// Removes the currently registered mock HTTP router.
pub fn remove_router() {
    let mut router = HTTP_CLIENT_ROUTER
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap();
    *router = None;
}

/// Thin scraper HTTP wrapper around `arachnea-http`.
#[derive(Clone)]
pub struct HttpClient {
    #[cfg(feature = "arachnea-proxy")]
    response_classifier:
        Option<arachnea_proxy::core::http::response_classifier::ProxyResponsePolicy>,
    http_config: ScraperHttpConfig,
    proxy_handle: SharedProxyConfigHandle,
    local_country: SharedLocalCountry,
    session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    cookie_cache: Arc<AsyncRwLock<SharedCookieCache>>,
    browser_session_manager: Arc<BrowserSessionManager>,
    client: Arc<AsyncRwLock<Option<CachedHttpClient>>>,
}

impl HttpClient {
    /// Builds a client with default scraper HTTP configuration.
    ///
    /// # Arguments
    ///
    /// * `_base_url` - Source URL retained for API compatibility; ignored by this constructor.
    pub fn new(_base_url: &str) -> Self {
        Self::with_http_config(ScraperHttpConfig::default())
    }

    /// Builds a client with explicit scraper HTTP configuration.
    ///
    /// # Arguments
    ///
    /// * `http_config` - Effective scraper HTTP options used for client construction or configuration.
    pub fn with_http_config(http_config: ScraperHttpConfig) -> Self {
        Self::with_http_config_and_proxy_handle(http_config, SharedProxyConfigHandle::new())
    }

    /// Builds a client with explicit scraper HTTP configuration and a shared proxy handle.
    ///
    /// # Arguments
    ///
    /// * `http_config` - Effective scraper HTTP options used for client construction or configuration.
    /// * `proxy_handle` - Shared, versioned routing configuration observed by derived HTTP clients.
    pub fn with_http_config_and_proxy_handle(
        http_config: ScraperHttpConfig,
        proxy_handle: SharedProxyConfigHandle,
    ) -> Self {
        Self::with_http_config_proxy_handle_and_local_country(
            http_config,
            proxy_handle,
            SharedLocalCountry::new(),
        )
    }

    /// Builds a client with explicit scraper HTTP configuration, a shared proxy
    /// handle, and shared local-country state.
    ///
    /// # Arguments
    ///
    /// * `http_config` - Effective scraper HTTP options used for client construction or configuration.
    /// * `proxy_handle` - Shared, versioned routing configuration observed by derived HTTP clients.
    /// * `local_country` - Shared explicit or detected outbound-country state used for local-country proxy bypass.
    pub fn with_http_config_proxy_handle_and_local_country(
        http_config: ScraperHttpConfig,
        proxy_handle: SharedProxyConfigHandle,
        local_country: SharedLocalCountry,
    ) -> Self {
        Self::with_http_config_proxy_handle_and_local_country_and_session_store(
            http_config,
            proxy_handle,
            local_country,
            memory_session_store().expect("in-memory Cloudflare session store is valid"),
        )
    }

    /// Builds a client with explicit scraper HTTP configuration, a shared proxy
    /// handle, shared local-country state, and a shared persistence store.
    ///
    /// # Arguments
    ///
    /// * `http_config` - Effective scraper HTTP options used for client construction or configuration.
    /// * `proxy_handle` - Shared, versioned routing configuration observed by derived HTTP clients.
    /// * `local_country` - Shared explicit or detected outbound-country state used for local-country proxy bypass.
    /// * `session_store` - Typed store for reusable chaser-cf sessions shared by this client family.
    pub fn with_http_config_proxy_handle_and_local_country_and_session_store(
        http_config: ScraperHttpConfig,
        proxy_handle: SharedProxyConfigHandle,
        local_country: SharedLocalCountry,
        session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    ) -> Self {
        Self::with_http_config_proxy_handle_and_cookie_cache(
            http_config,
            proxy_handle,
            local_country,
            session_store,
            global_cookie_cache(),
        )
    }

    /// Builds a client family with explicit routing, local-country, session-store and cookie-cache handles.
    ///
    /// # Arguments
    ///
    /// * `http_config` - Effective scraper HTTP options used for client construction or configuration.
    /// * `proxy_handle` - Shared, versioned routing configuration observed by derived HTTP clients.
    /// * `local_country` - Shared explicit or detected outbound-country state used for local-country proxy bypass.
    /// * `session_store` - Typed store for reusable chaser-cf sessions shared by this client family.
    /// * `cookie_cache` - Cookie cache shared by derived clients unless isolation is requested.
    fn with_http_config_proxy_handle_and_cookie_cache(
        http_config: ScraperHttpConfig,
        proxy_handle: SharedProxyConfigHandle,
        local_country: SharedLocalCountry,
        session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
        cookie_cache: Arc<AsyncRwLock<SharedCookieCache>>,
    ) -> Self {
        Self {
            #[cfg(feature = "arachnea-proxy")]
            response_classifier: None,
            http_config,
            proxy_handle,
            local_country,
            session_store,
            cookie_cache,
            browser_session_manager: Arc::new(BrowserSessionManager::new(
                BrowserSessionConfig::default(),
            )),
            client: Arc::new(AsyncRwLock::new(None)),
        }
    }

    /// Returns a clone using a fresh cookie cache while preserving HTTP and proxy config.
    pub fn with_isolated_cookies(&self) -> Self {
        let client = Self::with_http_config_proxy_handle_and_cookie_cache(
            self.http_config.clone(),
            self.proxy_handle.clone(),
            self.local_country.clone(),
            self.session_store.clone(),
            Arc::new(AsyncRwLock::new(SharedCookieCache::default())),
        );
        #[cfg(feature = "arachnea-proxy")]
        let client = Self {
            response_classifier: self.response_classifier.clone(),
            ..client
        };
        client
    }

    /// Returns the shared mutable proxy handle used by this client family.
    pub fn proxy_handle(&self) -> SharedProxyConfigHandle {
        self.proxy_handle.clone()
    }

    /// Returns a clone using a different HTTP configuration.
    ///
    /// # Arguments
    ///
    /// * `http_config` - Effective scraper HTTP options used for client construction or configuration.
    pub fn configured(&self, http_config: ScraperHttpConfig) -> Self {
        if http_config == self.http_config {
            return self.clone();
        }

        // Browser execution settings do not alter the underlying HTTP transport.
        // Reuse its client and engine when mode, browser fingerprint, and proxy
        // routing remain unchanged so retained page sessions outlive sub-queries.
        #[cfg(not(feature = "arachnea-proxy"))]
        let same_policy = true;
        #[cfg(feature = "arachnea-proxy")]
        let same_policy =
            http_config.proxy_response_classifier == self.http_config.proxy_response_classifier;
        if same_policy
            && http_config.proxy_retry_replayable == self.http_config.proxy_retry_replayable
            && http_config.request_mode() == self.http_config.request_mode()
            && http_config.browser_profile() == self.http_config.browser_profile()
            && http_config.proxy_countries_hint() == self.http_config.proxy_countries_hint()
            && http_config.proxy_domain_group == self.http_config.proxy_domain_group
            && http_config.proxy_affinity == self.http_config.proxy_affinity
            && http_config.proxy_rejection_statuses == self.http_config.proxy_rejection_statuses
            && http_config.max_redirects == self.http_config.max_redirects
        {
            return Self {
                #[cfg(feature = "arachnea-proxy")]
                response_classifier: self.response_classifier.clone(),
                http_config,
                proxy_handle: self.proxy_handle.clone(),
                local_country: self.local_country.clone(),
                session_store: self.session_store.clone(),
                cookie_cache: self.cookie_cache.clone(),
                browser_session_manager: self.browser_session_manager.clone(),
                client: self.client.clone(),
            };
        }

        let client = Self::with_http_config_proxy_handle_and_cookie_cache(
            http_config,
            self.proxy_handle.clone(),
            self.local_country.clone(),
            self.session_store.clone(),
            self.cookie_cache.clone(),
        );
        #[cfg(feature = "arachnea-proxy")]
        let client = Self {
            response_classifier: self.response_classifier.clone(),
            ..client
        };
        client
    }

    /// Installs a code policy overriding YAML and legacy statuses, with a fresh cache.
    ///
    /// # Arguments
    ///
    /// - `policy`: Nonblocking response classifier shared with derived clients.
    #[cfg(feature = "arachnea-proxy")]
    pub fn with_proxy_response_classifier(
        mut self,
        policy: arachnea_proxy::core::http::response_classifier::ProxyResponsePolicy,
    ) -> Self {
        self.response_classifier = Some(policy);
        self.client = Arc::new(AsyncRwLock::new(None));
        self
    }

    /// Lazily initializes and returns the underlying [`ArachneaHttpClient`].
    async fn http_client(&self) -> Result<Arc<ArachneaHttpClient>> {
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
            tracing::debug!(
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

    /// Sends a textual request through the HTTP facade, which owns correlated proxy retries.
    /// Text bodies are attached only for POST; an omitted POST body uses the URL query string.
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP method used for this request.
    /// * `url` - Absolute request URL used by the HTTP transport.
    /// * `request_headers` - Request header names and values resolved for this fetch.
    /// * `request_body` - Optional request payload; `None` leaves the body unspecified.
    ///
    /// # Errors
    ///
    /// Returns client-construction, header-conversion or HTTP-send errors with the target URL as context.
    async fn send_text_once(
        &self,
        method: Method,
        url: &str,
        request_headers: &HashMap<String, String>,
        request_body: Option<&str>,
    ) -> Result<ArachneaResponse> {
        let is_post = method == Method::POST;
        let http_client = self.http_client().await?;
        let mut request_builder =
            http_client.request_with_mode(method, url, self.http_config.request_mode());

        if !request_headers.is_empty() {
            request_builder =
                request_builder.headers(header_map_from_strings(request_headers.clone())?);
        }

        if is_post {
            let body = request_body.map(str::to_string).unwrap_or_else(|| {
                url.split_once('?')
                    .map(|(_, value)| value.to_string())
                    .unwrap_or_default()
            });
            request_builder = request_builder.body(body);
        }

        request_builder
            .send()
            .await
            .with_context(|| format!("Fetch fail {}", url))
    }

    /// Sends a binary request through the HTTP facade, which owns correlated proxy retries.
    /// A supplied body is attached for any method; this wrapper adds no retry loop.
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP method used for this request.
    /// * `url` - Absolute request URL used by the HTTP transport.
    /// * `request_headers` - Request header names and values resolved for this fetch.
    /// * `request_body` - Optional request payload; `None` leaves the body unspecified.
    ///
    /// # Errors
    ///
    /// Returns client-construction, header-conversion or HTTP-send errors with the target URL as context.
    async fn send_bytes_once(
        &self,
        method: Method,
        url: &str,
        request_headers: &HashMap<String, String>,
        request_body: Option<Vec<u8>>,
    ) -> Result<ArachneaResponse> {
        let http_client = self.http_client().await?;
        let mut request_builder =
            http_client.request_with_mode(method, url, self.http_config.request_mode());

        if !request_headers.is_empty() {
            request_builder =
                request_builder.headers(header_map_from_strings(request_headers.clone())?);
        }

        if let Some(body) = request_body {
            request_builder = request_builder.body(body);
        }

        request_builder
            .send()
            .await
            .with_context(|| format!("Fetch fail {}", url))
    }

    /// Fetches the raw response body for one request.
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP method used for this request.
    /// * `url` - Absolute URL to request.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or if the response body cannot be read.
    pub async fn query_http(&self, method: Method, url: &str) -> Result<String> {
        let headers = HashMap::new();
        self.query_http_for_request(method, url, &headers, None)
            .await
    }

    /// Fetches the raw response body for one request with optional extra headers and body.
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP verb used for the request.
    /// * `url` - Absolute URL to request.
    /// * `request_headers` - Additional headers applied to this request only.
    /// * `request_body` - Explicit POST text body; omitted POST bodies use the URL query, and other methods ignore it.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, if the response body cannot be read,
    /// or if one custom header is invalid.
    pub async fn query_http_for_request(
        &self,
        method: Method,
        url: &str,
        request_headers: &HashMap<String, String>,
        request_body: Option<&str>,
    ) -> Result<String> {
        let response = self
            .send_for_request(method, url, request_headers, request_body)
            .await?;
        let text = response.text().await?;
        trace!("{}", text);
        Ok(text)
    }

    /// Resolves the final response URL after redirects for one HTTP request definition.
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP verb used for the request.
    /// * `url` - Absolute URL to request.
    /// * `request_headers` - Additional headers applied to this request only.
    /// * `request_body` - Explicit POST text body; omitted POST bodies use the URL query, and other methods ignore it.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or if one custom header is invalid.
    pub async fn resolve_final_url_for_request(
        &self,
        method: Method,
        url: &str,
        request_headers: &HashMap<String, String>,
        request_body: Option<&str>,
    ) -> Result<String> {
        if url.starts_with("file://") {
            return Ok(url.to_string());
        }

        let response = self
            .send_for_request(method, url, request_headers, request_body)
            .await?;
        Ok(response.url().to_string())
    }

    /// Downloads and parses a JSON document.
    ///
    /// # Arguments
    ///
    /// * `url` - Absolute URL of the JSON payload to download.
    ///
    /// # Errors
    ///
    /// Returns an error if the payload cannot be downloaded or parsed.
    pub async fn get_json(&self, url: &str) -> Result<Value> {
        let headers = HashMap::new();
        self.get_json_with_headers(url, &headers).await
    }

    /// Downloads and parses a JSON document with optional extra headers.
    ///
    /// # Arguments
    ///
    /// * `url` - Absolute URL of the JSON payload to download.
    /// * `request_headers` - Additional headers applied to this request only.
    ///
    /// # Errors
    ///
    /// Returns an error if the payload cannot be downloaded, if one header is invalid,
    /// or if the response cannot be parsed as JSON.
    pub async fn get_json_with_headers(
        &self,
        url: &str,
        request_headers: &HashMap<String, String>,
    ) -> Result<Value> {
        self.get_json_for_request(Method::GET, url, request_headers, None)
            .await
    }

    /// Downloads and parses a JSON document for one HTTP request definition.
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP verb used for the request.
    /// * `url` - Absolute URL of the JSON payload to download.
    /// * `request_headers` - Additional headers applied to this request only.
    /// * `request_body` - Explicit POST text body; omitted POST bodies use the URL query, and other methods ignore it.
    ///
    /// # Errors
    ///
    /// Returns an error if the payload cannot be downloaded, if one header is invalid,
    /// or if the response cannot be parsed as JSON.
    pub async fn get_json_for_request(
        &self,
        method: Method,
        url: &str,
        request_headers: &HashMap<String, String>,
        request_body: Option<&str>,
    ) -> Result<Value> {
        if url.starts_with("file://") {
            let payload = std::fs::read_to_string(url.replacen("file://", "", 1))?;
            return serde_json::from_str(&payload).map_err(|error| {
                log_invalid_json_payload(url, "response", &payload, &error);
                anyhow::Error::new(error)
                    .context(format!("Invalid JSON payload returned by {}", url))
            });
        }

        let response = self
            .send_for_request(method, url, request_headers, request_body)
            .await?;
        let payload = response.text().await?;

        /////////////////////////////////////////////////////////////////////////////
        // Log response
        // println!("{}", payload);
        /////////////////////////////////////////////////////////////////////////////

        serde_json::from_str(&payload).map_err(|error| {
            log_invalid_json_payload(url, "response", &payload, &error);
            anyhow::Error::new(error).context(format!("Invalid JSON payload returned by {}", url))
        })
    }

    /// Downloads an HTML page and parses the JSON payload embedded in `__NEXT_DATA__`.
    ///
    /// # Arguments
    ///
    /// * `url` - Absolute URL of the HTML page containing a Next.js payload.
    ///
    /// # Errors
    ///
    /// Returns an error if the page cannot be downloaded, if the script tag is missing,
    /// or if the extracted JSON payload cannot be parsed.
    pub async fn get_next_data_json(&self, url: &str) -> Result<Value> {
        let headers = HashMap::new();
        self.get_next_data_json_with_headers(url, &headers).await
    }

    /// Downloads an HTML page and parses the JSON payload embedded in `__NEXT_DATA__`
    /// with optional extra headers.
    ///
    /// # Arguments
    ///
    /// * `url` - Absolute URL of the HTML page containing a Next.js payload.
    /// * `request_headers` - Additional headers applied to this request only.
    ///
    /// # Errors
    ///
    /// Returns an error if the page cannot be downloaded, if one header is invalid,
    /// if the script tag is missing, or if the extracted JSON payload cannot be parsed.
    pub async fn get_next_data_json_with_headers(
        &self,
        url: &str,
        request_headers: &HashMap<String, String>,
    ) -> Result<Value> {
        self.get_next_data_json_for_request(Method::GET, url, request_headers, None)
            .await
    }

    /// Downloads an HTML page and parses the JSON payload embedded in `__NEXT_DATA__`
    /// for one HTTP request definition.
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP verb used for the request.
    /// * `url` - Absolute URL of the HTML page containing a Next.js payload.
    /// * `request_headers` - Additional headers applied to this request only.
    /// * `request_body` - Explicit POST text body; omitted POST bodies use the URL query, and other methods ignore it.
    ///
    /// # Errors
    ///
    /// Returns an error if the page cannot be downloaded, if one header is invalid,
    /// if the script tag is missing, or if the extracted JSON payload cannot be parsed.
    pub async fn get_next_data_json_for_request(
        &self,
        method: Method,
        url: &str,
        request_headers: &HashMap<String, String>,
        request_body: Option<&str>,
    ) -> Result<Value> {
        let payload = self
            .query_http_for_request(method, url, request_headers, request_body)
            .await?;
        Self::extract_next_data_json(url, &payload)
    }

    /// Extracts and parses the `__NEXT_DATA__` JSON payload from an HTML page.
    ///
    /// # Arguments
    ///
    /// * `url` - Absolute URL the payload was downloaded from, used in errors.
    /// * `payload` - Raw HTML page content.
    ///
    /// # Errors
    ///
    /// Returns an error if the script tag is missing or the JSON is invalid.
    pub fn extract_next_data_json(url: &str, payload: &str) -> Result<Value> {
        let regex = NEXT_DATA_REGEX.get_or_init(|| {
            Regex::new(r#"(?s)<script[^>]*\bid=["']__NEXT_DATA__["'][^>]*>(.*?)</script>"#)
                .expect("Invalid __NEXT_DATA__ regex")
        });
        let captures = regex
            .captures(payload)
            .with_context(|| format!("Missing __NEXT_DATA__ payload in {}", url))?;
        let json_payload = captures
            .get(1)
            .map(|capture| capture.as_str())
            .with_context(|| format!("Invalid __NEXT_DATA__ capture in {}", url))?;

        serde_json::from_str(json_payload).map_err(|error| {
            log_invalid_json_payload(url, "__NEXT_DATA__", json_payload, &error);
            anyhow::Error::new(error).context(format!(
                "Invalid __NEXT_DATA__ JSON payload returned by {}",
                url
            ))
        })
    }

    /// Sends an HTTP request using the configured mode, headers, and optional body.
    ///
    /// When a mock router is installed, the request is intercepted. `file://` URLs
    /// are read from disk.
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP verb used for the request.
    /// * `url` - Absolute URL to request.
    /// * `request_headers` - Additional headers applied to this request only.
    /// * `request_body` - Explicit POST text body; omitted POST bodies use the URL query, and other methods ignore it.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, if the response body cannot be read,
    /// or if one custom header is invalid.
    pub async fn send_for_request(
        &self,
        method: Method,
        url: &str,
        request_headers: &HashMap<String, String>,
        request_body: Option<&str>,
    ) -> Result<ArachneaResponse> {
        if let Some(router) = HTTP_CLIENT_ROUTER
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap()
            .as_ref()
        {
            let payload = router(self, url)?;
            return Ok(ArachneaResponse::from_text(url, payload));
        }

        if url.starts_with("file://") {
            return Ok(ArachneaResponse::from_text(
                url,
                std::fs::read_to_string(url.replacen("file://", "", 1))?,
            ));
        }

        #[cfg(feature = "arachnea-proxy")]
        if !self.http_config.proxy_countries_hint().is_empty() {
            let search = arachnea_proxy::core::ProxySearchContext::current().unwrap_or_else(|| {
                arachnea_proxy::core::ProxySearchContext::default()
                    .with_replayable(self.http_config.proxy_retry_replayable.unwrap_or(false))
            });
            return search
                .scope(Box::pin(self.send_text_once(
                    method,
                    url,
                    request_headers,
                    request_body,
                )))
                .await;
        }
        let response =
            Box::pin(self.send_text_once(method, url, request_headers, request_body)).await?;
        Ok(response)
    }

    /// Sends an HTTP request with an optional binary body.
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP verb used for the request.
    /// * `url` - Absolute URL to request.
    /// * `request_headers` - Additional headers applied to this request only.
    /// * `request_body` - Optional raw request body.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, if the response body cannot be read,
    /// or if one custom header is invalid.
    pub async fn send_bytes_for_request(
        &self,
        method: Method,
        url: &str,
        request_headers: &HashMap<String, String>,
        request_body: Option<Vec<u8>>,
    ) -> Result<ArachneaResponse> {
        if let Some(router) = HTTP_CLIENT_ROUTER
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap()
            .as_ref()
        {
            let payload = router(self, url)?;
            return Ok(ArachneaResponse::from_text(url, payload));
        }

        if url.starts_with("file://") {
            return Ok(ArachneaResponse::from_text(
                url,
                std::fs::read_to_string(url.replacen("file://", "", 1))?,
            ));
        }

        #[cfg(feature = "arachnea-proxy")]
        if !self.http_config.proxy_countries_hint().is_empty() {
            let search = arachnea_proxy::core::ProxySearchContext::current().unwrap_or_else(|| {
                arachnea_proxy::core::ProxySearchContext::default()
                    .with_replayable(self.http_config.proxy_retry_replayable.unwrap_or(false))
            });
            return search
                .scope(Box::pin(self.send_bytes_once(
                    method,
                    url,
                    request_headers,
                    request_body,
                )))
                .await;
        }
        let response =
            Box::pin(self.send_bytes_once(method, url, request_headers, request_body)).await?;
        Ok(response)
    }
}

mod configuration;
pub use configuration::{
    ScraperBrowserClickConfig, ScraperBrowserContext, ScraperBrowserTokenCacheScope,
    ScraperBrowserTokenConfig, ScraperBrowserTokenRetry, ScraperBrowserTokenSource,
    ScraperHttpConfig, ScraperHttpExecution, ScraperHttpMode, ScraperHttpUserAgentProfile,
};

mod browser_sessions;

mod cookies;
