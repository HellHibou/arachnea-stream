use anyhow::{Context, Result};
use arachnea_core::persistence::{MemoryPersistenceStore, PersistenceStore};
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
use std::time::Duration;
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

/// Maximum number of characters included in invalid JSON payload logs.
const INVALID_JSON_LOG_PREVIEW_LIMIT: usize = 8_192;

/// Mutable proxy transport shared by one scraper instance and its derived HTTP clients.
#[derive(Clone, Default)]
pub struct SharedProxyConfigHandle {
    inner: Arc<RwLock<ProxyConfigState>>,
}

#[derive(Clone, Default)]
struct ProxyConfigState {
    version: u64,
    proxy: Option<HttpProxyConfig>,
}

#[derive(Clone)]
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

    /// Replaces the proxy configuration and increments the version.
    pub fn set_proxy(&self, proxy: HttpProxyConfig) {
        if let Ok(mut guard) = self.inner.write() {
            guard.version = guard.version.saturating_add(1);
            guard.proxy = Some(proxy);
        }
    }

    /// Removes the proxy override and increments the version.
    pub fn clear_proxy(&self) {
        if let Ok(mut guard) = self.inner.write() {
            guard.version = guard.version.saturating_add(1);
            guard.proxy = None;
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

/// YAML-selectable request mode for scraper HTTP calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperHttpMode {
    /// Automatically select the best request mode based on the target.
    Auto,
    /// Use direct HTTP requests without any special handling.
    Direct,
    /// Use Cloudflare smart mode for handling Cloudflare-protected sites.
    CloudflareSmart,
    /// Use browser-like requests for Cloudflare-protected sites.
    CloudflareBrowser,
}

/// Execution transport for a scraper query or sub-query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperHttpExecution {
    /// Use the normal HTTP client request path.
    Direct,
    /// Navigate a retained browser page and return its stable HTML source.
    PageNavigate,
    /// Navigate a retained browser page, click an element, then return rendered HTML.
    PageClick,
    /// Navigate a retained browser page, then execute JavaScript `fetch()` in it.
    PageFetch,
}

/// Scope used to reuse browser page sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperBrowserContext {
    /// Reuse a session only when origin, browser profile, and proxy route match.
    Origin,
    /// Open a dedicated page for one `page_click` sub-query execution.
    Isolated,
}

/// Browser-side source from which one request token is captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperBrowserTokenSource {
    /// Application token passed to `turnstile.render(... callback(token))`.
    TurnstileCallback,
}

/// Maximum browser-token retry policy for one sub-query execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperBrowserTokenRetry {
    /// Never retry after a classified token rejection.
    Never,
    /// Re-navigate once, obtain a fresh token, and submit one more request.
    Once,
}

/// Optional reuse scope for a browser callback token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperBrowserTokenCacheScope {
    /// Reuse the token only in the matching origin/profile/proxy browser session.
    Domain,
}

/// Token substitution and rejection signals for a browser page fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScraperBrowserTokenConfig {
    /// Source used to capture the token in the browser page.
    pub source: ScraperBrowserTokenSource,
    /// Literal request-body placeholder replaced with the captured token.
    pub placeholder: String,
    /// Optional opt-in reuse scope for the captured token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_scope: Option<ScraperBrowserTokenCacheScope>,
    /// Bounded retry policy when the target rejects the captured token.
    #[serde(default = "default_browser_token_retry")]
    pub retry_on_rejection: ScraperBrowserTokenRetry,
    /// HTTP status codes classified as token rejection after token submission.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejection_statuses: Vec<u16>,
    /// Response text fragments classified as token rejection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejection_body_markers: Vec<String>,
}

fn default_browser_token_retry() -> ScraperBrowserTokenRetry {
    ScraperBrowserTokenRetry::Never
}

/// Browser-page click and DOM wait configuration for a scraper sub-query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScraperBrowserClickConfig {
    /// CSS selector for the page element that triggers the site workflow.
    pub selector: String,
    /// CSS selector that must appear before the rendered page is extracted.
    pub wait_for_selector: String,
}

impl From<ScraperHttpMode> for HttpRequestMode {
    /// Converts the scraper-level mode into the [`HttpRequestMode`] expected by `arachnea-http`.
    fn from(mode: ScraperHttpMode) -> Self {
        match mode {
            ScraperHttpMode::Auto => Self::Auto,
            ScraperHttpMode::Direct => Self::Direct,
            ScraperHttpMode::CloudflareSmart => Self::CloudflareSmart,
            ScraperHttpMode::CloudflareBrowser => Self::CloudflareBrowser,
        }
    }
}

/// YAML-selectable browser profile used by `arachnea-http`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperHttpUserAgentProfile {
    /// Use Chrome browser profile.
    Chrome,
    /// Use stable Chrome browser profile.
    ChromeStable,
    /// Use Firefox browser profile.
    Firefox,
    /// Use stable Firefox browser profile.
    FirefoxStable,
}

impl From<ScraperHttpUserAgentProfile> for BrowserProfile {
    /// Converts the scraper-level profile into the [`BrowserProfile`] expected by `arachnea-http`.
    fn from(profile: ScraperHttpUserAgentProfile) -> Self {
        match profile {
            ScraperHttpUserAgentProfile::Chrome | ScraperHttpUserAgentProfile::ChromeStable => {
                Self::ChromeStable
            }
            ScraperHttpUserAgentProfile::Firefox | ScraperHttpUserAgentProfile::FirefoxStable => {
                Self::FirefoxStable
            }
        }
    }
}

/// Scraper YAML HTTP options inherited from collection to query to sub-query.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScraperHttpConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// HTTP request mode override for this scraper call.
    pub mode: Option<ScraperHttpMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Browser profile used to generate the User-Agent header.
    pub user_agent_profile: Option<ScraperHttpUserAgentProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Custom User-Agent string override.
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Proxy country hint (ISO country code) for geo-targeted requests.
    pub proxy_country: Option<String>,
    /// Maximum number of redirects to follow before returning an error.
    pub max_redirects: Option<usize>,
    /// Optional browser execution mode for a sub-query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<ScraperHttpExecution>,
    /// Browser session reuse scope required by browser page execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_context: Option<ScraperBrowserContext>,
    /// Source page URL template navigated before an in-page fetch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_url: Option<String>,
    /// Optional browser token configuration for the in-page request body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_token: Option<ScraperBrowserTokenConfig>,
    /// Optional click and DOM wait configuration for `execution: page_click`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_click: Option<ScraperBrowserClickConfig>,
}

impl ScraperHttpConfig {
    /// Returns whether this configuration has no explicit options.
    pub fn is_empty(&self) -> bool {
        self.mode.is_none()
            && self.user_agent_profile.is_none()
            && self.user_agent.is_none()
            && self.proxy_country.is_none()
            && self.max_redirects.is_none()
            && self.execution.is_none()
            && self.browser_context.is_none()
            && self.page_url.is_none()
            && self.browser_token.is_none()
            && self.browser_click.is_none()
    }

    /// Applies a child override on top of this config.
    pub fn merge(&self, child: &Self) -> Self {
        Self {
            mode: child.mode.or(self.mode),
            user_agent_profile: child.user_agent_profile.or(self.user_agent_profile),
            user_agent: child
                .user_agent
                .as_ref()
                .or(self.user_agent.as_ref())
                .cloned(),
            proxy_country: child
                .proxy_country
                .as_ref()
                .or(self.proxy_country.as_ref())
                .map(|value| normalize_proxy_country(value)),
            max_redirects: child.max_redirects.or(self.max_redirects),
            execution: child.execution.or(self.execution),
            browser_context: child.browser_context.or(self.browser_context),
            page_url: child.page_url.as_ref().or(self.page_url.as_ref()).cloned(),
            browser_token: child
                .browser_token
                .as_ref()
                .or(self.browser_token.as_ref())
                .cloned(),
            browser_click: child
                .browser_click
                .as_ref()
                .or(self.browser_click.as_ref())
                .cloned(),
        }
    }

    /// Routes requests through a proxy country when a proxy core supports it.
    ///
    /// The country code is normalized with trim + uppercase before storage.
    pub fn proxy_country(mut self, country: impl AsRef<str>) -> Self {
        let country = normalize_proxy_country(country.as_ref());
        self.proxy_country = (!country.is_empty()).then_some(country);
        self
    }

    /// Returns the [`HttpRequestMode`] derived from this configuration.
    fn request_mode(&self) -> HttpRequestMode {
        self.mode.unwrap_or(ScraperHttpMode::Auto).into()
    }

    /// Returns the [`BrowserProfile`] derived from user-agent or user-agent profile.
    fn browser_profile(&self) -> BrowserProfile {
        self.user_agent
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .map(|value| BrowserProfile::Custom(value.trim().to_string()))
            .unwrap_or_else(|| {
                self.user_agent_profile
                    .map(BrowserProfile::from)
                    .unwrap_or_default()
            })
    }

    /// Returns the normalized proxy country hint, if any.
    fn proxy_country_hint(&self) -> Option<String> {
        self.proxy_country
            .as_ref()
            .map(|value| normalize_proxy_country(value))
            .filter(|value| !value.is_empty())
    }

    /// Resolves collection parameters in template-capable HTTP options.
    pub fn resolve_collection_params(
        &mut self,
        context: &str,
        query_name: &str,
        params: &HashMap<String, String>,
    ) -> Result<()> {
        if let Some(user_agent) = self.user_agent.as_mut() {
            *user_agent = query_helpers::resolve_required_template(
                context,
                query_name,
                "http.user_agent",
                user_agent,
                params,
            )?;
        }
        if let Some(proxy_country) = self.proxy_country.as_mut() {
            *proxy_country = normalize_proxy_country(&query_helpers::resolve_required_template(
                context,
                query_name,
                "http.proxy_country",
                proxy_country,
                params,
            )?);
        }
        Ok(())
    }
}

fn normalize_proxy_country(country: &str) -> String {
    country.trim().to_ascii_uppercase()
}

/// Installs a temporary HTTP router override used by integration/unit tests.
///
/// # Arguments
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
    http_config: ScraperHttpConfig,
    proxy_handle: SharedProxyConfigHandle,
    local_country: SharedLocalCountry,
    persistence_store: Arc<dyn PersistenceStore>,
    cookie_cache: Arc<AsyncRwLock<SharedCookieCache>>,
    browser_session_manager: Arc<BrowserSessionManager>,
    client: Arc<AsyncRwLock<Option<CachedHttpClient>>>,
}

impl HttpClient {
    const REQUEST_TIMEOUT: Duration = Duration::from_secs(25);

    /// Builds a client with default scraper HTTP configuration.
    ///
    /// # Arguments
    ///
    /// * `base_url` - Source base URL kept for API compatibility.
    pub fn new(_base_url: &str) -> Self {
        Self::with_http_config(ScraperHttpConfig::default())
    }

    /// Builds a client with explicit scraper HTTP configuration.
    pub fn with_http_config(http_config: ScraperHttpConfig) -> Self {
        Self::with_http_config_and_proxy_handle(http_config, SharedProxyConfigHandle::new())
    }

    /// Builds a client with explicit scraper HTTP configuration and a shared proxy handle.
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
    pub fn with_http_config_proxy_handle_and_local_country(
        http_config: ScraperHttpConfig,
        proxy_handle: SharedProxyConfigHandle,
        local_country: SharedLocalCountry,
    ) -> Self {
        Self::with_http_config_proxy_handle_and_local_country_and_persistence_store(
            http_config,
            proxy_handle,
            local_country,
            Arc::new(MemoryPersistenceStore::new()),
        )
    }

    /// Builds a client with explicit scraper HTTP configuration, a shared proxy
    /// handle, shared local-country state, and a shared persistence store.
    pub fn with_http_config_proxy_handle_and_local_country_and_persistence_store(
        http_config: ScraperHttpConfig,
        proxy_handle: SharedProxyConfigHandle,
        local_country: SharedLocalCountry,
        persistence_store: Arc<dyn PersistenceStore>,
    ) -> Self {
        Self::with_http_config_proxy_handle_and_cookie_cache(
            http_config,
            proxy_handle,
            local_country,
            persistence_store,
            global_cookie_cache(),
        )
    }

    fn with_http_config_proxy_handle_and_cookie_cache(
        http_config: ScraperHttpConfig,
        proxy_handle: SharedProxyConfigHandle,
        local_country: SharedLocalCountry,
        persistence_store: Arc<dyn PersistenceStore>,
        cookie_cache: Arc<AsyncRwLock<SharedCookieCache>>,
    ) -> Self {
        Self {
            http_config,
            proxy_handle,
            local_country,
            persistence_store,
            cookie_cache,
            browser_session_manager: Arc::new(BrowserSessionManager::new(
                BrowserSessionConfig::default(),
            )),
            client: Arc::new(AsyncRwLock::new(None)),
        }
    }

    /// Returns a clone using a fresh cookie cache while preserving HTTP and proxy config.
    pub fn with_isolated_cookies(&self) -> Self {
        Self::with_http_config_proxy_handle_and_cookie_cache(
            self.http_config.clone(),
            self.proxy_handle.clone(),
            self.local_country.clone(),
            self.persistence_store.clone(),
            Arc::new(AsyncRwLock::new(SharedCookieCache::default())),
        )
    }

    /// Returns the shared mutable proxy handle used by this client family.
    pub fn proxy_handle(&self) -> SharedProxyConfigHandle {
        self.proxy_handle.clone()
    }

    /// Returns a clone using a different HTTP configuration.
    pub fn configured(&self, http_config: ScraperHttpConfig) -> Self {
        if http_config == self.http_config {
            return self.clone();
        }

        // Browser execution settings do not alter the underlying HTTP transport.
        // Reuse its client and engine when mode, browser fingerprint, and proxy
        // routing remain unchanged so retained page sessions outlive sub-queries.
        if http_config.request_mode() == self.http_config.request_mode()
            && http_config.browser_profile() == self.http_config.browser_profile()
            && http_config.proxy_country_hint() == self.http_config.proxy_country_hint()
            && http_config.max_redirects == self.http_config.max_redirects
        {
            return Self {
                http_config,
                proxy_handle: self.proxy_handle.clone(),
                local_country: self.local_country.clone(),
                persistence_store: self.persistence_store.clone(),
                cookie_cache: self.cookie_cache.clone(),
                browser_session_manager: self.browser_session_manager.clone(),
                client: self.client.clone(),
            };
        }

        Self::with_http_config_proxy_handle_and_cookie_cache(
            http_config,
            self.proxy_handle.clone(),
            self.local_country.clone(),
            self.persistence_store.clone(),
            self.cookie_cache.clone(),
        )
    }

    /// Stores simple name/value cookies for a URL in the shared HTTP cache.
    pub async fn store_cookies_for_url(url: &str, cookies: &HashMap<String, String>) -> Result<()> {
        Self::store_cookies_in_cache(global_cookie_cache(), url, cookies).await
    }

    /// Stores simple name/value cookies for a URL in this client's cookie cache.
    pub async fn store_cookies_for_url_in_client(
        &self,
        url: &str,
        cookies: &HashMap<String, String>,
    ) -> Result<()> {
        Self::store_cookies_in_cache(self.cookie_cache.clone(), url, cookies).await
    }

    async fn store_cookies_in_cache(
        cache: Arc<AsyncRwLock<SharedCookieCache>>,
        url: &str,
        cookies: &HashMap<String, String>,
    ) -> Result<()> {
        let url = Url::parse(url)?;
        let domain = url
            .host_str()
            .context("Cookie URL must contain a host")?
            .to_string();
        let path = if url.path().is_empty() {
            "/".to_string()
        } else {
            url.path().to_string()
        };
        let secure = url.scheme() == "https";
        let mut cache = cache.write().await;

        for (name, value) in cookies {
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() || value.is_empty() {
                continue;
            }

            cache.insert_cookie(CookieEntry {
                name: name.to_string(),
                value: value.to_string(),
                domain: domain.clone(),
                path: path.clone(),
                expires_at: None,
                secure,
                http_only: false,
                same_site: None,
            });
        }

        Ok(())
    }

    /// Returns the shared-cookie view currently applicable to one absolute URL.
    ///
    /// # Arguments
    ///
    /// * `url` - Absolute URL whose matching cookies should be returned.
    ///
    /// # Errors
    ///
    /// Returns an error if `url` is invalid.
    pub async fn cookies_for_url(url: &str) -> Result<HashMap<String, String>> {
        Self::cookies_for_url_in_cache(global_cookie_cache(), url).await
    }

    /// Returns this client's cookie view currently applicable to one absolute URL.
    pub async fn cookies_for_url_in_client(&self, url: &str) -> Result<HashMap<String, String>> {
        Self::cookies_for_url_in_cache(self.cookie_cache.clone(), url).await
    }

    async fn cookies_for_url_in_cache(
        cache: Arc<AsyncRwLock<SharedCookieCache>>,
        url: &str,
    ) -> Result<HashMap<String, String>> {
        let mut cache = cache.write().await;
        let cookies = cache.cookies_for_url(url)?;
        Ok(cookies)
    }

    /// Lazily initializes and returns the underlying [`ArachneaHttpClient`].
    async fn http_client(&self) -> Result<Arc<ArachneaHttpClient>> {
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

        let mut builder = ArachneaHttpConfig::builder()
            .default_request_mode(self.http_config.request_mode())
            .user_agent_profile(self.http_config.browser_profile())
            .request_timeout(Self::REQUEST_TIMEOUT)
            .max_redirects(self.http_config.max_redirects);
        if let Some(proxy) = proxy_state.proxy {
            builder = builder.proxy(proxy);
        }
        if let Some(proxy_country) = self.http_config.proxy_country_hint() {
            builder = builder.proxy_parameter("country", proxy_country);
        }
        let config = builder.build()?;
        let client = Arc::new(
            ArachneaHttpClient::new_with_cookie_cache_browser_session_manager_and_persistence_store(
                config,
                self.cookie_cache.clone(),
                self.browser_session_manager.clone(),
                self.persistence_store.clone(),
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

    /// Fetches the raw response body for one request.
    ///
    /// # Arguments
    ///
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
    /// * `request_body` - Optional request body used as-is.
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

    /// Navigates a browser page and returns its stable HTML for a scraper query.
    ///
    /// The browser session and cookie handoff are provided by `arachnea-http`.
    pub async fn page_navigate_for_request(
        &self,
        url: &str,
        request_headers: &HashMap<String, String>,
    ) -> Result<String> {
        let headers = header_map_from_strings(
            request_headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        )?;
        let response = self
            .http_client()
            .await?
            .page_navigate(PageNavigationRequest {
                url: url.to_string(),
                headers,
                collect_body: true,
            })
            .await?;
        response
            .body
            .ok_or_else(|| anyhow::anyhow!("browser navigation did not return page HTML"))
    }

    /// Navigates a browser page, clicks one element, and returns the rendered HTML.
    pub async fn page_click_for_request(
        &self,
        page_url: &str,
        selector: &str,
        wait_for_selector: &str,
    ) -> Result<String> {
        let response = self
            .http_client()
            .await?
            .page_click(
                PageNavigationRequest {
                    url: page_url.to_string(),
                    headers: HeaderMap::new(),
                    collect_body: false,
                },
                PageClickRequest {
                    selector: selector.to_string(),
                    wait_for_selector: wait_for_selector.to_string(),
                },
            )
            .await?;
        Ok(response.body)
    }

    /// Opens an isolated browser page, clicks an element, and returns the rendered HTML.
    ///
    /// Unlike [`Self::page_click_for_request`], this operation does not reuse the
    /// origin-scoped page session, so independent clicks can run concurrently.
    pub async fn page_click_isolated_for_request(
        &self,
        page_url: &str,
        selector: &str,
        wait_for_selector: &str,
    ) -> Result<String> {
        let response = self
            .http_client()
            .await?
            .page_click_isolated(
                PageNavigationRequest {
                    url: page_url.to_string(),
                    headers: HeaderMap::new(),
                    collect_body: false,
                },
                PageClickRequest {
                    selector: selector.to_string(),
                    wait_for_selector: wait_for_selector.to_string(),
                },
            )
            .await?;
        Ok(response.body)
    }

    /// Closes the retained browser session associated with one page URL.
    pub async fn close_browser_session_for_url(&self, url: &str) -> Result<()> {
        self.http_client().await?.close_browser_session(url)?;
        Ok(())
    }

    /// Navigates a browser page and executes one JavaScript fetch for a scraper sub-query.
    ///
    /// The browser session, cookie handoff, callback token capture, and token
    /// rejection classification are provided by `arachnea-http`.
    pub async fn page_fetch_for_request(
        &self,
        page_url: &str,
        method: Method,
        url: &str,
        request_headers: &HashMap<String, String>,
        request_body: Option<&str>,
        token: Option<&ScraperBrowserTokenConfig>,
    ) -> Result<String> {
        let headers = header_map_from_strings(
            request_headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        )?;
        let token_rejection_statuses = token
            .map(|config| {
                config
                    .rejection_statuses
                    .iter()
                    .filter_map(|status| http::StatusCode::from_u16(*status).ok())
                    .collect()
            })
            .unwrap_or_default();
        let response = self
            .http_client()
            .await?
            .page_fetch(
                PageNavigationRequest {
                    url: page_url.to_string(),
                    headers: HeaderMap::new(),
                    collect_body: false,
                },
                PageFetchRequest {
                    method,
                    url: url.to_string(),
                    headers,
                    body: request_body.map(|body| body.as_bytes().to_vec().into()),
                    turnstile_token_placeholder: token.map(|config| config.placeholder.clone()),
                    reuse_turnstile_token: token.is_some_and(|config| {
                        config.cache_scope == Some(ScraperBrowserTokenCacheScope::Domain)
                    }),
                    token_rejection_statuses,
                    token_rejection_body_markers: token
                        .map(|config| config.rejection_body_markers.clone())
                        .unwrap_or_default(),
                },
            )
            .await?;
        Ok(response.body)
    }

    /// Resolves the final response URL after redirects for one HTTP request definition.
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP verb used for the request.
    /// * `url` - Absolute URL to request.
    /// * `request_headers` - Additional headers applied to this request only.
    /// * `request_body` - Optional request body used as-is.
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
    /// * `request_body` - Optional request body used as-is.
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
    /// * `request_body` - Optional request body used as-is.
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
    /// * `request_body` - Optional request body used as-is.
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
}
