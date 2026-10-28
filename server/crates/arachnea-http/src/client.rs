mod browser_sessions;
mod cloudflare;
mod execution;
mod proxy_runtime;
mod request;
mod response;
mod urls;

use urls::*;

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

use crate::chaser_session::{memory_session_store, CachedChaserSession};
use arachnea_core::{persistence::TypedEntityStore, DEFAULT_MAX_REDIRECTS};
#[cfg(feature = "arachnea-proxy")]
use arachnea_proxy::connectors::ArachneaRquestLoopback;
use bytes::Bytes;
use encoding_rs::{Encoding, WINDOWS_1252};
use http::{
    header::{CONTENT_TYPE, COOKIE, LOCATION, USER_AGENT},
    HeaderMap, HeaderName, HeaderValue, Method, StatusCode,
};
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use url::Url;

use crate::{
    browser::{
        BrowserSessionKey, BrowserSessionManager, BrowserSessionMetadata, PageClickRequest,
        PageClickResponse, PageFetchRequest, PageFetchResponse, PageNavigationRequest,
        PageNavigationResponse,
    },
    cloudflare::detect_cloudflare_block,
    config::{ArachneaHttpConfig, CloudflareSolverKind, HttpProxyConfig, HttpRequestMode},
    cookies::{global_cookie_cache, SharedCookieCache},
    engine::{
        build_auto_smart_cloudflare_solver, build_browser_cloudflare_solver, rquest::RquestEngine,
        DynHttpEngine, EngineRequest, HttpEngine, SOLVER_USER_AGENT_HEADER,
    },
    error::ArachneaHttpError,
};

/// Extracts the charset label from a `Content-Type` header value.
///
/// For example, `text/html; charset=iso-8859-1` returns `iso-8859-1`.
/// Returns `None` when no charset is declared.
///
/// # Arguments
///
/// - `content_type`: The Content-Type header value to parse
///
/// # Returns
///
/// The charset label if found, or `None` if no charset is declared
fn charset_from_content_type(content_type: &str) -> Option<&str> {
    for part in content_type.split(';') {
        let part = part.trim();
        if let Some(charset) = part
            .strip_prefix("charset=")
            .or_else(|| part.strip_prefix("charset ="))
        {
            let charset = charset.trim().trim_matches('"').trim_matches('\'');
            if !charset.is_empty() {
                return Some(charset);
            }
        }
    }
    None
}

/// Global process-wide Cloudflare solver user-agent cache.
static CLOUDFLARE_USER_AGENTS: OnceLock<Arc<RwLock<HashMap<String, String>>>> = OnceLock::new();
/// Returns the shared Cloudflare solver user-agent cache.
///
/// # Returns
///
/// A clone of the process-wide origin-to-user-agent cache.
fn global_cloudflare_user_agents() -> Arc<RwLock<HashMap<String, String>>> {
    CLOUDFLARE_USER_AGENTS
        .get_or_init(|| Arc::new(RwLock::new(HashMap::new())))
        .clone()
}

#[cfg(feature = "arachnea-proxy")]
pub use proxy_runtime::ProxyTransportPool;
use proxy_runtime::*;

/// High-level outbound HTTP client facade.
#[derive(Clone)]
pub struct ArachneaHttpClient {
    /// Runtime configuration.
    config: ArachneaHttpConfig,
    /// Shared cookie cache used by this client.
    cookies: Arc<RwLock<SharedCookieCache>>,
    /// Fast rquest engine used by predefined request modes.
    rquest: Option<RquestEngine>,
    /// Optional direct browser-backed engine.
    direct_engine: Option<DynHttpEngine>,
    /// Optional smart Cloudflare challenge solver engine.
    smart_cloudflare_engine: Option<DynHttpEngine>,
    /// Optional browser-backed Cloudflare challenge solver engine.
    browser_cloudflare_engine: Option<DynHttpEngine>,
    /// Prepared helper state used by engines that need a live proxy runtime.
    _proxy_runtime: Arc<PreparedProxyRuntime>,
    /// User-agent observed by the solver for each refreshed origin.
    cloudflare_user_agents: Arc<RwLock<HashMap<String, String>>>,
    /// Manages reusable browser page sessions for in-page fetch operations.
    browser_session_manager: Option<Arc<BrowserSessionManager>>,
}

/// Cloudflare refresh path used after a mode selects a solver class.
///
/// This enum represents the two classes of Cloudflare solvers available:
/// smart solvers (like Ghostwire) and browser-backed solvers (like chaser-cf).
#[derive(Debug, Clone, Copy)]
enum CloudflareRefreshStrategy {
    /// Lightweight smart solver path, normally Ghostwire when compiled.
    Smart,
    /// Browser-backed solver path, normally chaser-cf or Tauri/Wry.
    Browser,
}

impl CloudflareRefreshStrategy {
    /// Returns a stable strategy name for diagnostics.
    ///
    /// # Returns
    ///
    /// A static strategy identifier.
    fn name(self) -> &'static str {
        match self {
            Self::Smart => "smart",
            Self::Browser => "browser",
        }
    }
}

impl ArachneaHttpClient {
    /// Builds a new HTTP client with the default configuration.
    ///
    /// This is a fallible async convenience constructor equivalent to:
    ///
    /// ```rust,no_run
    /// # use arachnea_http::{ArachneaHttpClient, ArachneaHttpConfig};
    /// # async fn build() -> Result<ArachneaHttpClient, arachnea_http::ArachneaHttpError> {
    /// ArachneaHttpClient::new(ArachneaHttpConfig::builder().build()?).await
    /// # }
    /// ```
    ///
    /// # Returns
    ///
    /// A client that shares the process-wide cookie cache.
    ///
    /// # Errors
    ///
    /// Returns configuration validation or engine construction errors.
    pub async fn default() -> Result<Self, ArachneaHttpError> {
        let config = ArachneaHttpConfig::builder().build()?;
        Self::new(config).await
    }

    /// Builds a new HTTP client.
    ///
    /// # Arguments
    ///
    /// - `config`: Validated runtime configuration.
    ///
    /// # Returns
    ///
    /// A client that shares the process-wide cookie cache.
    ///
    /// # Errors
    ///
    /// Returns engine construction errors.
    pub async fn new(config: ArachneaHttpConfig) -> Result<Self, ArachneaHttpError> {
        Self::new_with_cookie_cache(config, global_cookie_cache()).await
    }

    /// Builds a new HTTP client with an explicit cookie cache.
    ///
    /// # Arguments
    ///
    /// - `config`: Validated runtime configuration.
    /// - `cookies`: Cookie cache used by this client.
    ///
    /// # Returns
    ///
    /// A client bound to the provided cookie cache.
    ///
    /// # Errors
    ///
    /// Returns engine construction errors.
    pub async fn new_with_cookie_cache(
        config: ArachneaHttpConfig,
        cookies: Arc<RwLock<SharedCookieCache>>,
    ) -> Result<Self, ArachneaHttpError> {
        let browser_session_manager = Arc::new(BrowserSessionManager::new(config.browser_session));
        Self::new_with_cookie_cache_and_browser_session_manager(
            config,
            cookies,
            browser_session_manager,
        )
        .await
    }

    /// Builds a new HTTP client with explicit cookies and shared browser sessions.
    ///
    /// Callers that derive clients with distinct request modes can pass the same
    /// manager to preserve pages for matching origin, profile, and proxy keys.
    /// This constructor uses an in-memory Cloudflare session store.
    ///
    /// # Arguments
    ///
    /// - `config`: Validated runtime configuration.
    /// - `cookies`: Shared cookie cache used by this client.
    /// - `browser_session_manager`: Shared browser page manager.
    ///
    /// # Returns
    ///
    /// A client bound to the provided caches.
    ///
    /// # Errors
    ///
    /// Returns engine or proxy initialization errors.
    pub async fn new_with_cookie_cache_and_browser_session_manager(
        config: ArachneaHttpConfig,
        cookies: Arc<RwLock<SharedCookieCache>>,
        browser_session_manager: Arc<BrowserSessionManager>,
    ) -> Result<Self, ArachneaHttpError> {
        Self::new_with_cookie_cache_browser_session_manager_and_session_store(
            config,
            cookies,
            browser_session_manager,
            memory_session_store().expect("in-memory Cloudflare session store is valid"),
        )
        .await
    }

    /// Builds a new HTTP client with explicit cookies, shared browser sessions,
    /// and a shared typed Cloudflare session store.
    ///
    /// Callers that derive clients with distinct request modes can pass the same
    /// manager to preserve pages for matching origin, profile, and proxy keys.
    ///
    /// # Arguments
    ///
    /// - `config`: Validated runtime configuration.
    /// - `cookies`: Shared cookie cache used by this client.
    /// - `browser_session_manager`: Shared browser page manager.
    /// - `session_store`: Typed store used to persist chaser-cf sessions.
    ///
    /// # Returns
    ///
    /// A client bound to the provided caches and session store.
    ///
    /// # Errors
    ///
    /// Returns engine, proxy initialization, or session-store construction
    /// errors.
    pub async fn new_with_cookie_cache_browser_session_manager_and_session_store(
        config: ArachneaHttpConfig,
        cookies: Arc<RwLock<SharedCookieCache>>,
        browser_session_manager: Arc<BrowserSessionManager>,
        session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    ) -> Result<Self, ArachneaHttpError> {
        let proxy_runtime = Arc::new(PreparedProxyRuntime::new(&config).await?);
        let direct_engine = config.engine.direct_engine();
        let rquest = if direct_engine.is_some() {
            None
        } else {
            Some(RquestEngine::new(
                &config,
                proxy_runtime.proxy_url(&config),
                proxy_runtime.rquest_client(&config)?,
            )?)
        };
        let smart_cloudflare_engine =
            Self::build_smart_cloudflare_engine(&config, proxy_runtime.as_ref()).await?;
        let browser_cloudflare_engine = build_browser_cloudflare_solver(
            &config,
            &config.cloudflare_browser_solver,
            proxy_runtime.chaser_cf_proxy_url(&config),
            session_store,
        )
        .await?;
        Ok(Self {
            config,
            cookies,
            rquest,
            direct_engine,
            smart_cloudflare_engine,
            browser_cloudflare_engine,
            _proxy_runtime: proxy_runtime,
            cloudflare_user_agents: global_cloudflare_user_agents(),
            browser_session_manager: Some(browser_session_manager),
        })
    }

    /// Builds the optional smart Cloudflare refresh engine for a client.
    ///
    /// # Arguments
    ///
    /// - `config`: Client configuration.
    /// - `proxy_runtime`: Prepared runtime supplying the loopback proxy URL.
    ///
    /// # Returns
    ///
    /// A Cloudflare-capable engine when configuration and features provide one.
    ///
    /// # Errors
    ///
    /// Returns engine construction or disabled feature errors.
    async fn build_smart_cloudflare_engine(
        config: &ArachneaHttpConfig,
        proxy_runtime: &PreparedProxyRuntime,
    ) -> Result<Option<DynHttpEngine>, ArachneaHttpError> {
        match &config.cloudflare_solver {
            CloudflareSolverKind::Disabled => Ok(None),
            CloudflareSolverKind::Engine(engine) => Ok(Some(engine.clone())),
            CloudflareSolverKind::Auto => {
                build_auto_smart_cloudflare_solver(
                    config,
                    proxy_runtime.chaser_cf_proxy_url(config),
                )
                .await
            }
        }
    }

    /// Creates a GET request builder.
    ///
    /// # Arguments
    ///
    /// - `url`: Absolute request URL.
    ///
    /// # Returns
    ///
    /// A request builder.
    pub fn get(&self, url: impl Into<String>) -> ArachneaRequestBuilder {
        self.request(Method::GET, url)
    }

    /// Creates a POST request builder.
    ///
    /// # Arguments
    ///
    /// - `url`: Absolute request URL.
    ///
    /// # Returns
    ///
    /// A request builder.
    pub fn post(&self, url: impl Into<String>) -> ArachneaRequestBuilder {
        self.request(Method::POST, url)
    }

    /// Creates a PUT request builder.
    ///
    /// # Arguments
    ///
    /// - `url`: Absolute request URL.
    ///
    /// # Returns
    ///
    /// A request builder.
    pub fn put(&self, url: impl Into<String>) -> ArachneaRequestBuilder {
        self.request(Method::PUT, url)
    }

    /// Creates a PATCH request builder.
    ///
    /// # Arguments
    ///
    /// - `url`: Absolute request URL.
    ///
    /// # Returns
    ///
    /// A request builder.
    pub fn patch(&self, url: impl Into<String>) -> ArachneaRequestBuilder {
        self.request(Method::PATCH, url)
    }

    /// Creates a DELETE request builder.
    ///
    /// # Arguments
    ///
    /// - `url`: Absolute request URL.
    ///
    /// # Returns
    ///
    /// A request builder.
    pub fn delete(&self, url: impl Into<String>) -> ArachneaRequestBuilder {
        self.request(Method::DELETE, url)
    }

    /// Creates a HEAD request builder.
    ///
    /// # Arguments
    ///
    /// - `url`: Absolute request URL.
    ///
    /// # Returns
    ///
    /// A request builder.
    pub fn head(&self, url: impl Into<String>) -> ArachneaRequestBuilder {
        self.request(Method::HEAD, url)
    }

    /// Creates a request builder with a caller-provided method.
    ///
    /// # Arguments
    ///
    /// - `method`: HTTP method.
    /// - `url`: Absolute request URL.
    ///
    /// # Returns
    ///
    /// A request builder.
    pub fn request(&self, method: Method, url: impl Into<String>) -> ArachneaRequestBuilder {
        ArachneaRequestBuilder {
            client: self.clone(),
            method,
            url: url.into(),
            mode: None,
            headers: HeaderMap::new(),
            body: None,
        }
    }

    /// Creates a request builder with a caller-provided method and execution mode.
    ///
    /// # Arguments
    ///
    /// - `method`: HTTP method.
    /// - `url`: Absolute request URL.
    /// - `mode`: Request execution mode.
    ///
    /// # Returns
    ///
    /// A request builder.
    pub fn request_with_mode(
        &self,
        method: Method,
        url: impl Into<String>,
        mode: HttpRequestMode,
    ) -> ArachneaRequestBuilder {
        self.request(method, url).mode(mode)
    }

    /// Executes a request and follows redirects according to the client configuration.
    ///
    /// Redirect targets are recorded on the final response. A redirect response
    /// rejected by a correlated proxy classifier stops the chain without being
    /// followed. Exceeding the configured limit returns `RedirectLimitExceeded`.
    ///
    /// # Arguments
    ///
    /// - `options`: Prepared options for the first hop.
    /// - `engine_name`: Engine label used in diagnostics.
    /// - `execute_once`: Callback that executes one hop without following redirects.
    ///
    /// # Returns
    ///
    /// The final response with its redirect history.
    ///
    /// # Errors
    ///
    /// Returns URL, redirect, engine, or proxy-classification failures.
    async fn execute_with_redirects<F, Fut>(
        &self,
        mut options: RequestOptions,
        engine_name: &str,
        mut execute_once: F,
    ) -> Result<ArachneaResponse, ArachneaHttpError>
    where
        F: FnMut(RequestOptions, String, Vec<String>) -> Fut,
        Fut: std::future::Future<Output = Result<StepResponse, ArachneaHttpError>>,
    {
        let max_redirects = self.config.max_redirects.unwrap_or(DEFAULT_MAX_REDIRECTS);
        let requested_url = options.url.clone();
        let mut step = execute_once(options.clone(), requested_url.clone(), Vec::new()).await?;
        let mut redirect_urls = Vec::new();

        for redirect_index in 0..max_redirects {
            #[cfg(feature = "arachnea-proxy")]
            if matches!(step.response.proxy_response_decision(), Some(arachnea_proxy::core::http::response_classifier::ProxyResponseDecision::RejectedForDestination { .. })) {
                step.response.set_redirect_urls(redirect_urls);
                return Ok(step.response);
            }
            let Some(next_url) = redirect_target(&step.response)? else {
                step.response.set_redirect_urls(redirect_urls);
                return Ok(step.response);
            };

            #[cfg(feature = "arachnea-proxy")]
            if let Some(token) = step.response.proxy_selection() {
                token.record_redirect(step.response.status.as_u16()).await;
            }

            debug!(
                engine = engine_name,
                from = %step.response.url,
                to = %next_url,
                redirect = redirect_index + 1,
                "following HTTP redirect"
            );
            redirect_urls.push(next_url.clone());
            options = options.for_redirect(next_url, step.response.status);
            #[cfg(feature = "arachnea-proxy")]
            if let Some(pending) = step.redirect_transport.take() {
                let target = Url::parse(&options.url)
                    .map_err(|error| ArachneaHttpError::InvalidUrl(error.to_string()))?;
                if matches!(target.scheme(), "http" | "https") && target.host_str().is_some() {
                    tracing::debug!(
                        search_id = arachnea_proxy::core::ProxySearchContext::current().map(|search| search.id()),
                        destination = %origin_url(&step.response.url)?,
                        "retaining HTTP transport after followed proxy redirect"
                    );
                    pending.pool.retain(pending.key, pending.transport).await;
                }
            }
            step = execute_once(
                options.clone(),
                requested_url.clone(),
                redirect_urls.clone(),
            )
            .await?;
        }

        #[cfg(feature = "arachnea-proxy")]
        if matches!(step.response.proxy_response_decision(), Some(arachnea_proxy::core::http::response_classifier::ProxyResponseDecision::RejectedForDestination { .. })) {
            step.response.set_redirect_urls(redirect_urls);
            return Ok(step.response);
        }
        if redirect_target(&step.response)?.is_some() {
            return Err(ArachneaHttpError::RedirectLimitExceeded {
                limit: max_redirects,
            });
        }

        step.response.set_redirect_urls(redirect_urls);
        Ok(step.response)
    }

    /// Builds an engine request with shared cookies injected.
    ///
    /// # Arguments
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// A normalized engine request.
    ///
    /// # Errors
    ///
    /// Returns invalid header or URL errors.
    async fn engine_request(
        &self,
        options: RequestOptions,
    ) -> Result<EngineRequest, ArachneaHttpError> {
        let cookie = self.cookies.write().await.cookie_header_for(&options.url)?;
        let user_agent = self.cloudflare_user_agent_for_url(&options.url).await?;
        debug!(
            origin = %origin_url(&options.url)?,
            has_cf_clearance = cookie
                .as_deref()
                .is_some_and(|value| value.split(';').any(|part| part.trim_start().starts_with("cf_clearance="))),
            has_solver_user_agent = user_agent.is_some(),
            "building rquest request from Cloudflare session state"
        );
        let mut headers = self.base_headers(cookie, user_agent.as_deref())?;
        headers.extend(options.headers);
        debug!("headers:{:?}", redacted_headers(&headers));
        debug!(
            body_bytes = options.body.as_ref().map_or(0, |body| body.len()),
            "prepared HTTP request body metadata"
        );
        Ok(EngineRequest {
            method: options.method,
            url: options.url,
            headers,
            body: options.body,
        })
    }

    /// Builds common request headers.
    ///
    /// # Arguments
    ///
    /// - `cookie`: Optional cookie header value.
    /// - `user_agent`: Optional user-agent learned from a Cloudflare solver.
    ///
    /// # Returns
    ///
    /// Headers shared by outbound engines.
    ///
    /// # Errors
    ///
    /// Returns `InvalidHeader` when the configured user-agent or cookie cannot
    /// be represented as a header value.
    fn base_headers(
        &self,
        cookie: Option<String>,
        user_agent: Option<&str>,
    ) -> Result<HeaderMap, ArachneaHttpError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(
                user_agent.unwrap_or(self.config.user_agent_profile.user_agent()),
            )
            .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
        );
        if let Some(cookie) = cookie {
            headers.insert(
                COOKIE,
                HeaderValue::from_str(&cookie)
                    .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
            );
        }
        Ok(headers)
    }

    /// Stores solver metadata returned by a Cloudflare refresh.
    ///
    /// # Arguments
    ///
    /// - `url`: Final response URL associated with the refresh.
    /// - `headers`: Refresh response headers containing internal metadata.
    ///
    /// # Errors
    ///
    /// Returns invalid URL or invalid header errors.
    async fn store_cloudflare_solver_metadata(
        &self,
        url: &str,
        headers: &HeaderMap,
    ) -> Result<(), ArachneaHttpError> {
        let Some(value) = headers.get(SOLVER_USER_AGENT_HEADER) else {
            return Ok(());
        };
        let user_agent = value
            .to_str()
            .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?
            .trim();
        if user_agent.is_empty() {
            return Ok(());
        }
        let origin = origin_url(url)?;
        self.cloudflare_user_agents
            .write()
            .await
            .insert(origin, user_agent.to_string());
        Ok(())
    }

    /// Returns the solver-observed user-agent for a URL.
    ///
    /// # Arguments
    ///
    /// - `url`: Request URL whose origin is inspected.
    ///
    /// # Returns
    ///
    /// A user-agent value when a solver has refreshed the origin.
    ///
    /// # Errors
    ///
    /// Returns `InvalidUrl` when the URL cannot be normalized.
    async fn cloudflare_user_agent_for_url(
        &self,
        url: &str,
    ) -> Result<Option<String>, ArachneaHttpError> {
        let origin = origin_url(url)?;
        Ok(self
            .cloudflare_user_agents
            .read()
            .await
            .get(&origin)
            .cloned())
    }

    /// Stores cookies from a response into the shared cache.
    ///
    /// # Arguments
    ///
    /// - `url`: Final response URL.
    /// - `headers`: Response headers.
    ///
    /// # Errors
    ///
    /// Returns invalid URL errors from cookie parsing.
    async fn store_response_cookies(
        &self,
        url: &str,
        headers: &HeaderMap,
    ) -> Result<(), ArachneaHttpError> {
        self.cookies.write().await.set_from_set_cookie_headers(
            url,
            headers,
            self.config.default_session_cookie_ttl,
        )
    }

    /// Synchronizes browser page metadata with the shared HTTP cookie and
    /// user-agent caches.
    ///
    /// Metadata with an empty URL is ignored.
    ///
    /// # Arguments
    ///
    /// - `metadata`: Final page URL and synthesized browser headers.
    ///
    /// # Errors
    ///
    /// Returns URL or header errors from the shared caches.
    async fn store_browser_session_metadata(
        &self,
        metadata: BrowserSessionMetadata,
    ) -> Result<(), ArachneaHttpError> {
        if metadata.url.is_empty() {
            return Ok(());
        }
        self.store_cloudflare_solver_metadata(&metadata.url, &metadata.headers)
            .await?;
        self.store_response_cookies(&metadata.url, &metadata.headers)
            .await
    }
}

pub use request::{ArachneaRequestBuilder, RequestOptions};

pub use response::ArachneaResponse;

/// Builds a header map from textual header names and values.
///
/// This helper function converts string-based header name/value pairs into a
/// validated `HeaderMap`. It's useful for callers that have headers in string
/// format (e.g., from configuration files) and need to convert them for use
/// with the HTTP client.
///
/// # Arguments
///
/// - `headers`: Iterable of header name/value pairs as strings
///
/// # Returns
///
/// A validated `HeaderMap` containing the parsed headers
///
/// # Errors
///
/// Returns `InvalidHeader` if a name or value cannot be represented as an HTTP header
///
/// # Examples
///
/// ```
/// # use arachnea_http::{header_map_from_strings, ArachneaHttpError};
/// # fn example() -> Result<(), ArachneaHttpError> {
/// let headers = vec![
///     ("User-Agent", "MyClient/1.0"),
///     ("Accept", "application/json"),
/// ];
/// let header_map = header_map_from_strings(headers)?;
/// # Ok(())
/// # }
/// ```
pub fn header_map_from_strings<I, K, V>(headers: I) -> Result<HeaderMap, ArachneaHttpError>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut header_map = HeaderMap::new();
    for (name, value) in headers {
        let name = name.as_ref();
        let value = value.as_ref();
        let header_name = HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?;
        let header_value = HeaderValue::from_str(value.trim())
            .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?;
        header_map.insert(header_name, header_value);
    }
    Ok(header_map)
}

/// Builds a redacted header map suitable for logging or diagnostics.
///
/// Sensitive headers such as `Authorization` and `Cookie` are replaced with
/// `<redacted>`. Non-UTF-8 values are replaced with `<non-utf8>`.
///
/// # Arguments
///
/// - `headers`: Original header map to redact.
///
/// # Returns
///
/// A diagnostic-safe string map of header names to values.
fn redacted_headers(headers: &HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .map(|(name, value)| {
            let header_name = name.as_str().to_ascii_lowercase();
            let header_value = if header_name == "authorization" || header_name == "cookie" {
                "<redacted>".to_string()
            } else {
                value
                    .to_str()
                    .map(str::to_string)
                    .unwrap_or_else(|_| "<non-utf8>".to_string())
            };
            (header_name, header_value)
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/support/client.rs"]
mod test_support;

#[cfg(test)]
#[path = "../tests/client.rs"]
mod transport_tests;
