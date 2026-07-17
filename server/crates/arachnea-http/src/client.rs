use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

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
/// # Parameters
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
/// Default maximum number of HTTP redirects followed by the client facade
/// when ArachneaHttpConfig.max_redirects is None.
const DEFAULT_MAX_REDIRECTS: usize = 16;

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

/// Prepared proxy runtime shared by engines that need live helper state.
///
/// This struct holds runtime state for proxy transports that need to be initialized
/// asynchronously, such as the `arachnea-proxy` loopback helper.
#[derive(Clone)]
struct PreparedProxyRuntime {
    #[cfg(feature = "arachnea-proxy")]
    loopback_proxy: Option<Arc<ArachneaRquestLoopback>>,
}

impl PreparedProxyRuntime {
    /// Prepares runtime state required by the configured proxy transport.
    ///
    /// # Parameters
    ///
    /// - `config`: Client configuration containing proxy settings
    ///
    /// # Returns
    ///
    /// A prepared proxy runtime with any necessary helper state
    ///
    /// # Errors
    ///
    /// Returns proxy initialization errors when the `arachnea-proxy` feature is enabled
    /// and the loopback helper cannot be started
    async fn new(config: &ArachneaHttpConfig) -> Result<Self, ArachneaHttpError> {
        #[cfg(feature = "arachnea-proxy")]
        {
            let loopback_proxy = match &config.proxy {
                HttpProxyConfig::Arachnea(core) => Some(Arc::new(
                    ArachneaRquestLoopback::start(core.clone())
                        .await
                        .map_err(|err| {
                            ArachneaHttpError::Proxy(format!(
                                "failed to start arachnea-proxy loopback helper: {err}"
                            ))
                        })?,
                )),
                _ => None,
            };
            return Ok(Self { loopback_proxy });
        }

        #[cfg(not(feature = "arachnea-proxy"))]
        {
            let _ = config;
            Ok(Self {})
        }
    }

    /// Returns the proxy URL that built-in engines should use, when one is available.
    ///
    /// # Parameters
    ///
    /// - `config`: Client configuration to inspect
    ///
    /// # Returns
    ///
    /// The proxy URL string when a network proxy is configured, or the loopback
    /// proxy URL when `arachnea-proxy` is enabled and initialized
    fn proxy_url<'a>(&'a self, config: &'a ArachneaHttpConfig) -> Option<&'a str> {
        match &config.proxy {
            HttpProxyConfig::Network(url) => Some(url.as_str()),
            #[cfg(feature = "arachnea-proxy")]
            HttpProxyConfig::Arachnea(_) => self
                .loopback_proxy
                .as_deref()
                .map(|loopback| loopback.proxy_url()),
            HttpProxyConfig::Disabled => None,
        }
    }

    /// Builds a rquest client configured with proxy transport and parameters.
    ///
    /// # Parameters
    ///
    /// - `config`: Client configuration containing proxy settings
    ///
    /// # Returns
    ///
    /// An optional rquest client configured with the arachnea-proxy loopback
    /// transport when available, or `None` when no loopback proxy is initialized
    ///
    /// # Errors
    ///
    /// Returns proxy errors when the loopback client cannot be created
    #[cfg(feature = "arachnea-proxy")]
    fn rquest_client(
        &self,
        config: &ArachneaHttpConfig,
    ) -> Result<Option<rquest::Client>, ArachneaHttpError> {
        let Some(loopback) = self.loopback_proxy.as_deref() else {
            return Ok(None);
        };
        if config.proxy_parameters.is_empty() {
            return loopback
                .client()
                .map(Some)
                .map_err(|err| ArachneaHttpError::Proxy(err.to_string()));
        }
        loopback
            .client_with_parameters(
                config
                    .proxy_parameters
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str())),
            )
            .map(Some)
            .map_err(|err| ArachneaHttpError::Proxy(err.to_string()))
    }

    /// Builds a rquest client configured with proxy transport and parameters.
    #[cfg(not(feature = "arachnea-proxy"))]
    fn rquest_client(
        &self,
        _config: &ArachneaHttpConfig,
    ) -> Result<Option<rquest::Client>, ArachneaHttpError> {
        Ok(None)
    }
}

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
    /// # Parameters
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
    /// # Parameters
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
        let browser_cloudflare_engine =
            build_browser_cloudflare_solver(&config, &config.cloudflare_browser_solver).await?;
        Ok(Self {
            config,
            cookies,
            rquest,
            direct_engine,
            smart_cloudflare_engine,
            browser_cloudflare_engine,
            _proxy_runtime: proxy_runtime,
            cloudflare_user_agents: global_cloudflare_user_agents(),
        })
    }

    /// Builds the optional smart Cloudflare refresh engine for a client.
    ///
    /// # Parameters
    ///
    /// - `config`: Client configuration.
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
                build_auto_smart_cloudflare_solver(config, proxy_runtime.proxy_url(config)).await
            }
        }
    }

    /// Creates a GET request builder.
    ///
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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

    /// Forces a Cloudflare refresh for an origin.
    ///
    /// # Parameters
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

    /// Forces a Cloudflare refresh using the first configured solver path.
    ///
    /// # Parameters
    ///
    /// - `url`: Absolute URL used to obtain or refresh Cloudflare cookies.
    ///
    /// # Errors
    ///
    /// Returns Cloudflare solver or URL parsing failures.
    async fn refresh_cloudflare_for_url(
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
    /// # Parameters
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
    async fn refresh_cloudflare_for_url_with_strategy(
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
        debug!(
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
    /// # Parameters
    ///
    /// - `strategy`: Solver class selected by the request mode.
    ///
    /// # Returns
    ///
    /// The configured engine for the strategy, when available.
    fn cloudflare_engine_for_strategy(
        &self,
        strategy: CloudflareRefreshStrategy,
    ) -> Option<&DynHttpEngine> {
        match strategy {
            CloudflareRefreshStrategy::Smart => self.smart_cloudflare_engine.as_ref(),
            CloudflareRefreshStrategy::Browser => self.browser_cloudflare_engine.as_ref(),
        }
    }

    /// Executes a prepared request with the configured engine policy.
    ///
    /// # Parameters
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// A materialized response.
    ///
    /// # Errors
    ///
    /// Returns URL, engine, cookie, or Cloudflare detection failures.
    async fn execute(
        &self,
        options: RequestOptions,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        let mode = options.mode.unwrap_or(self.config.default_request_mode);
        match mode {
            HttpRequestMode::Auto => self.execute_auto_request(options).await,
            HttpRequestMode::Direct => self.execute_with_rquest(options).await,
            HttpRequestMode::CloudflareSmart => {
                self.execute_cloudflare_cookie_mode(options, CloudflareRefreshStrategy::Smart)
                    .await
            }
            HttpRequestMode::CloudflareBrowser => {
                self.execute_cloudflare_cookie_mode(options, CloudflareRefreshStrategy::Browser)
                    .await
            }
        }
    }

    /// Executes one request with the automatic fallback chain.
    ///
    /// The automatic chain starts with the configured fast path, refreshes with
    /// the smart solver when available, then falls back to the browser solver
    /// when it is configured and the Cloudflare block remains.
    ///
    /// # Parameters
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// A materialized response.
    ///
    /// # Errors
    ///
    /// Returns URL, engine, cookie, or Cloudflare detection failures.
    async fn execute_auto_request(
        &self,
        options: RequestOptions,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        let origin = origin_url(&options.url)?;
        if self.direct_engine.is_some() {
            return self.execute_with_direct_engine(options).await;
        }
        let mut response = self.execute_with_rquest(options.clone()).await?;
        let detection =
            detect_cloudflare_block(response.status, &response.headers, response.preview_body());
        if !detection.detected {
            return Ok(response);
        }

        let mut attempted_refresh = false;
        if self.smart_cloudflare_engine.is_some() {
            attempted_refresh = true;
            info!(
                origin = %origin,
                reason = %detection.reason,
                "Cloudflare block detected; refreshing with smart solver"
            );
            match self
                .refresh_cloudflare_for_url_with_strategy(
                    &options.url,
                    CloudflareRefreshStrategy::Smart,
                    true,
                    Some(&options.headers),
                )
                .await
            {
                Ok(()) => {
                    response = self.execute_with_rquest(options.clone()).await?;
                    let retry_detection = detect_cloudflare_block(
                        response.status,
                        &response.headers,
                        response.preview_body(),
                    );
                    if !retry_detection.detected {
                        return Ok(response);
                    }
                    info!(
                        origin = %origin,
                        reason = %retry_detection.reason,
                        "Cloudflare block still present after smart solver"
                    );
                }
                Err(err) if self.browser_cloudflare_engine.is_some() => {
                    warn!(
                        origin = %origin,
                        error = %err,
                        "smart Cloudflare refresh failed; trying browser solver"
                    );
                }
                Err(err) => return Err(err),
            }
        }

        if self.browser_cloudflare_engine.is_some() {
            info!(
                origin = %origin,
                reason = %detection.reason,
                "Cloudflare block detected; refreshing with browser solver"
            );
            self.refresh_cloudflare_for_url_with_strategy(
                &options.url,
                CloudflareRefreshStrategy::Browser,
                true,
                Some(&options.headers),
            )
            .await?;
            response = self.execute_with_rquest(options.clone()).await?;
            let retry_detection = detect_cloudflare_block(
                response.status,
                &response.headers,
                response.preview_body(),
            );
            if !retry_detection.detected {
                return Ok(response);
            }
            info!(
                origin = %origin,
                reason = %retry_detection.reason,
                "Cloudflare block still present after browser solver rquest handoff; retrying with browser solver response"
            );
            match self
                .execute_with_browser_cloudflare_engine(options.clone())
                .await
            {
                Ok(browser_response) => {
                    let browser_detection = detect_cloudflare_block(
                        browser_response.status,
                        &browser_response.headers,
                        browser_response.preview_body(),
                    );
                    if !browser_detection.detected {
                        return Ok(browser_response);
                    }
                    return Err(ArachneaHttpError::CloudflareBlocked {
                        origin,
                        reason: browser_detection.reason,
                    });
                }
                Err(ArachneaHttpError::UnsupportedEngineOperation { .. }) => {}
                Err(err) => return Err(err),
            }
            return Err(ArachneaHttpError::CloudflareBlocked {
                origin,
                reason: retry_detection.reason,
            });
        }

        if attempted_refresh {
            Err(ArachneaHttpError::CloudflareBlocked {
                origin,
                reason: detection.reason,
            })
        } else {
            Err(ArachneaHttpError::CloudflareSolverUnavailable)
        }
    }

    /// Executes one request with cookie-aware Cloudflare refresh behavior.
    ///
    /// # Parameters
    ///
    /// - `options`: Prepared request options.
    /// - `strategy`: Cloudflare refresh strategy used when cookies are stale or
    ///   a block is detected.
    ///
    /// # Returns
    ///
    /// A materialized response.
    ///
    /// # Errors
    ///
    /// Returns URL, engine, cookie, or Cloudflare detection failures.
    async fn execute_cloudflare_cookie_mode(
        &self,
        options: RequestOptions,
        strategy: CloudflareRefreshStrategy,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        let origin = origin_url(&options.url)?;
        let mut refresh_attempts_used = 0;
        if self
            .cookies
            .write()
            .await
            .needs_refresh(&origin, self.config.cookie_refresh_margin)?
        {
            let fresh = matches!(strategy, CloudflareRefreshStrategy::Browser);
            self.refresh_cloudflare_for_url_with_strategy(
                &options.url,
                strategy,
                fresh,
                Some(&options.headers),
            )
            .await?;
            refresh_attempts_used = 1;
        }
        self.execute_rquest_with_retry(options, &origin, refresh_attempts_used, strategy)
            .await
    }

    /// Executes one request through rquest and optionally retries after Cloudflare refresh.
    ///
    /// # Parameters
    ///
    /// - `options`: Prepared request options.
    /// - `origin`: Origin used for Cloudflare refresh and errors.
    /// - `refresh_attempts_used`: Cloudflare refresh attempts already spent by
    ///   the caller before the first fast-path request.
    /// - `strategy`: Cloudflare refresh strategy used for retries.
    ///
    /// # Returns
    ///
    /// A materialized response.
    ///
    /// # Errors
    ///
    /// Returns Cloudflare block errors after bounded retry exhaustion.
    async fn execute_rquest_with_retry(
        &self,
        options: RequestOptions,
        origin: &str,
        refresh_attempts_used: u8,
        strategy: CloudflareRefreshStrategy,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        let mut response = self.execute_with_rquest(options.clone()).await?;
        let detection =
            detect_cloudflare_block(response.status, &response.headers, response.preview_body());
        if !detection.detected {
            return Ok(response);
        }

        let mut block_reason = detection.reason;
        let first_retry = refresh_attempts_used.min(self.config.max_cloudflare_retries);
        for retry in first_retry..self.config.max_cloudflare_retries {
            warn!(
                origin = %origin,
                retry = retry + 1,
                strategy = strategy.name(),
                reason = %block_reason,
                "Cloudflare block detected; refreshing cookies"
            );
            self.refresh_cloudflare_for_url_with_strategy(
                &options.url,
                strategy,
                true,
                Some(&options.headers),
            )
            .await?;
            response = self.execute_with_rquest(options.clone()).await?;
            let retry_detection = detect_cloudflare_block(
                response.status,
                &response.headers,
                response.preview_body(),
            );
            if !retry_detection.detected {
                return Ok(response);
            }
            warn!(
                origin = %origin,
                retry = retry + 1,
                strategy = strategy.name(),
                reason = %retry_detection.reason,
                "Cloudflare block still present after refresh"
            );
            block_reason = retry_detection.reason;
        }

        if matches!(strategy, CloudflareRefreshStrategy::Browser) {
            warn!(
                origin = %origin,
                reason = %block_reason,
                "Cloudflare block still present after browser refresh; retrying with browser solver response"
            );
            match self
                .execute_with_browser_cloudflare_engine(options.clone())
                .await
            {
                Ok(browser_response) => {
                    let browser_detection = detect_cloudflare_block(
                        browser_response.status,
                        &browser_response.headers,
                        browser_response.preview_body(),
                    );
                    if !browser_detection.detected {
                        return Ok(browser_response);
                    }
                    return Err(ArachneaHttpError::CloudflareBlocked {
                        origin: origin.to_string(),
                        reason: browser_detection.reason,
                    });
                }
                Err(ArachneaHttpError::UnsupportedEngineOperation { .. }) => {}
                Err(err) => return Err(err),
            }
        }

        Err(ArachneaHttpError::CloudflareBlocked {
            origin: origin.to_string(),
            reason: block_reason,
        })
    }

    /// Executes one request through the browser-backed Cloudflare solver.
    ///
    /// This is used as a last-resort browser response path when the solver
    /// succeeds but the cookie handoff back to `rquest` is still rejected.
    ///
    /// # Parameters
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// A materialized response from the browser-backed solver.
    ///
    /// # Errors
    ///
    /// Returns solver, redirect, cookie storage, or unsupported operation errors.
    async fn execute_with_browser_cloudflare_engine(
        &self,
        options: RequestOptions,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        self.execute_with_redirects(options, "browser-cloudflare", |options| async move {
            self.execute_one_with_browser_cloudflare_engine(options)
                .await
        })
        .await
    }

    /// Executes one request through the browser-backed Cloudflare solver without following redirects.
    ///
    /// # Parameters
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// One materialized browser-backed solver response.
    ///
    /// # Errors
    ///
    /// Returns solver execution or cookie storage errors.
    async fn execute_one_with_browser_cloudflare_engine(
        &self,
        options: RequestOptions,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        let engine = self
            .browser_cloudflare_engine
            .as_ref()
            .ok_or(ArachneaHttpError::CloudflareSolverUnavailable)?;
        info!(engine = engine.name(), method = %options.method, url = %options.url, "sending request");
        let request = self.engine_request(options).await?;
        let response = engine.send(request).await?;
        self.store_cloudflare_solver_metadata(&response.url, &response.headers)
            .await?;
        self.store_response_cookies(&response.url, &response.headers)
            .await?;
        Ok(ArachneaResponse::from_engine(response))
    }

    /// Executes one request through rquest.
    ///
    /// # Parameters
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// A materialized response.
    ///
    /// # Errors
    ///
    /// Returns request execution or cookie storage errors.
    async fn execute_with_rquest(
        &self,
        options: RequestOptions,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        self.execute_with_redirects(options, "rquest", |options| async move {
            self.execute_one_with_rquest(options).await
        })
        .await
    }

    /// Executes one request through rquest without following redirects.
    ///
    /// # Parameters
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// One materialized response.
    ///
    /// # Errors
    ///
    /// Returns request execution or cookie storage errors.
    async fn execute_one_with_rquest(
        &self,
        options: RequestOptions,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        info!(engine = "rquest", method = %options.method, url = %options.url, "sending request");
        let request = self.engine_request(options).await?;
        let response = if let Some(rquest) = self.rquest.as_ref() {
            rquest.send(request).await?
        } else {
            RquestEngine::new(
                &self.config,
                self._proxy_runtime.proxy_url(&self.config),
                self._proxy_runtime.rquest_client(&self.config)?,
            )?
            .send(request)
            .await?
        };
        self.store_response_cookies(&response.url, &response.headers)
            .await?;
        Ok(ArachneaResponse::from_engine(response))
    }

    /// Executes one request through the configured direct engine.
    ///
    /// # Parameters
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// A materialized response.
    ///
    /// # Errors
    ///
    /// Returns request execution or cookie storage errors.
    async fn execute_with_direct_engine(
        &self,
        options: RequestOptions,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        self.execute_with_redirects(options, "direct", |options| async move {
            self.execute_one_with_direct_engine(options).await
        })
        .await
    }

    /// Executes one request through the configured direct engine without following redirects.
    ///
    /// # Parameters
    ///
    /// - `options`: Prepared request options.
    ///
    /// # Returns
    ///
    /// One materialized response.
    ///
    /// # Errors
    ///
    /// Returns request execution or cookie storage errors.
    async fn execute_one_with_direct_engine(
        &self,
        options: RequestOptions,
    ) -> Result<ArachneaResponse, ArachneaHttpError> {
        let engine = self
            .direct_engine
            .as_ref()
            .ok_or(ArachneaHttpError::CloudflareSolverUnavailable)?;
        info!(engine = engine.name(), method = %options.method, url = %options.url, "sending request");
        let request = self.engine_request(options).await?;
        let response = engine.send(request).await?;
        self.store_response_cookies(&response.url, &response.headers)
            .await?;
        Ok(ArachneaResponse::from_engine(response))
    }

    /// Executes a request and follows redirects according to the client configuration.
    async fn execute_with_redirects<F, Fut>(
        &self,
        mut options: RequestOptions,
        engine_name: &str,
        mut execute_once: F,
    ) -> Result<ArachneaResponse, ArachneaHttpError>
    where
        F: FnMut(RequestOptions) -> Fut,
        Fut: std::future::Future<Output = Result<ArachneaResponse, ArachneaHttpError>>,
    {
        let max_redirects = self.config.max_redirects.unwrap_or(DEFAULT_MAX_REDIRECTS);
        let mut response = execute_once(options.clone()).await?;
        let mut redirect_urls = Vec::new();

        for redirect_index in 0..max_redirects {
            let Some(next_url) = redirect_target(&response)? else {
                response.set_redirect_urls(redirect_urls);
                return Ok(response);
            };

            debug!(
                engine = engine_name,
                from = %response.url,
                to = %next_url,
                redirect = redirect_index + 1,
                "following HTTP redirect"
            );
            redirect_urls.push(next_url.clone());
            options = options.for_redirect(next_url, response.status);
            response = execute_once(options.clone()).await?;
        }

        if redirect_target(&response)?.is_some() {
            return Err(ArachneaHttpError::RedirectLimitExceeded {
                limit: max_redirects,
            });
        }

        response.set_redirect_urls(redirect_urls);
        Ok(response)
    }

    /// Builds an engine request with shared cookies injected.
    ///
    /// # Parameters
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
        let mut headers = self.base_headers(cookie, user_agent.as_deref())?;
        headers.extend(options.headers);
        debug!("headers:{:?}", redacted_headers(&headers));
        debug!("cookies:{:?}", self.cookies);
        Ok(EngineRequest {
            method: options.method,
            url: options.url,
            headers,
            body: options.body,
        })
    }

    /// Builds common request headers.
    ///
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
    /// # Parameters
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
}

/// Builder for one outbound request.
pub struct ArachneaRequestBuilder {
    /// Owning client facade.
    client: ArachneaHttpClient,
    /// HTTP method.
    method: Method,
    /// Absolute URL.
    url: String,
    /// Optional per-request execution mode.
    mode: Option<HttpRequestMode>,
    /// Request headers.
    headers: HeaderMap,
    /// Optional request body.
    body: Option<Bytes>,
}

impl ArachneaRequestBuilder {
    /// Adds or replaces one header.
    ///
    /// # Parameters
    ///
    /// - `name`: Header name.
    /// - `value`: Header value.
    ///
    /// # Returns
    ///
    /// The updated request builder.
    pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }

    /// Adds or replaces multiple headers.
    ///
    /// # Parameters
    ///
    /// - `headers`: Header map merged into this request.
    ///
    /// # Returns
    ///
    /// The updated request builder.
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.headers.extend(headers);
        self
    }

    /// Sets the execution mode for this request.
    ///
    /// # Parameters
    ///
    /// - `mode`: Predefined request mode.
    ///
    /// # Returns
    ///
    /// The updated request builder.
    pub fn mode(mut self, mode: HttpRequestMode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Sets a raw request body.
    ///
    /// # Parameters
    ///
    /// - `body`: Body bytes.
    ///
    /// # Returns
    ///
    /// The updated request builder.
    pub fn body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Serializes and sets a JSON request body.
    ///
    /// # Parameters
    ///
    /// - `value`: Serializable JSON value.
    ///
    /// # Returns
    ///
    /// The updated request builder or a JSON serialization error.
    pub fn json<T: Serialize>(mut self, value: &T) -> Result<Self, ArachneaHttpError> {
        let body =
            serde_json::to_vec(value).map_err(|err| ArachneaHttpError::Json(err.to_string()))?;
        self.headers
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        self.body = Some(Bytes::from(body));
        Ok(self)
    }

    /// Sends the prepared request.
    ///
    /// # Returns
    ///
    /// A materialized HTTP response.
    ///
    /// # Errors
    ///
    /// Returns URL, engine, cookie, JSON, or Cloudflare errors.
    pub async fn send(self) -> Result<ArachneaResponse, ArachneaHttpError> {
        let options = RequestOptions {
            method: self.method,
            url: self.url,
            mode: self.mode,
            headers: self.headers,
            body: self.body,
        };
        self.client.execute(options).await
    }
}

/// Prepared request options used internally by the client.
#[derive(Debug, Clone)]
pub struct RequestOptions {
    /// HTTP method.
    pub method: Method,
    /// Absolute request URL.
    pub url: String,
    /// Optional per-request execution mode.
    pub mode: Option<HttpRequestMode>,
    /// Request headers.
    pub headers: HeaderMap,
    /// Optional request body.
    pub body: Option<Bytes>,
}

impl RequestOptions {
    /// Builds the next request options in a redirect chain.
    fn for_redirect(mut self, url: String, status: StatusCode) -> Self {
        if should_rewrite_redirect_to_get(status, &self.method) {
            self.method = Method::GET;
            self.body = None;
            self.headers.remove(CONTENT_TYPE);
        }
        self.url = url;
        self
    }
}

/// Materialized HTTP response returned by Arachnea.
#[derive(Debug, Clone)]
pub struct ArachneaResponse {
    /// Final URL reported by the engine.
    url: String,
    /// Redirect target URLs followed by the HTTP facade.
    redirect_urls: Vec<String>,
    /// HTTP status code.
    status: StatusCode,
    /// Response headers.
    headers: HeaderMap,
    /// Response body bytes.
    body: Bytes,
}

impl ArachneaResponse {
    /// Converts an engine response into a public response.
    ///
    /// # Parameters
    ///
    /// - `response`: Engine-normalized response.
    ///
    /// # Returns
    ///
    /// A public response wrapper.
    fn from_engine(response: crate::engine::EngineResponse) -> Self {
        Self {
            url: response.url,
            redirect_urls: Vec::new(),
            status: response.status,
            headers: response.headers,
            body: response.body,
        }
    }

    /// Builds an in-memory successful text response.
    ///
    /// This is intended for local fixtures and caller-side mock routers that
    /// need to share the same response helpers as real HTTP responses.
    pub fn from_text(url: impl Into<String>, body: impl Into<Bytes>) -> Self {
        Self {
            url: url.into(),
            redirect_urls: Vec::new(),
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: body.into(),
        }
    }

    /// Returns the final URL.
    ///
    /// # Returns
    ///
    /// Final response URL as a string slice.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns redirect target URLs followed before this response.
    ///
    /// # Returns
    ///
    /// Redirect target URLs in request order.
    pub fn redirect_urls(&self) -> &[String] {
        &self.redirect_urls
    }

    /// Replaces the redirect target URL history.
    fn set_redirect_urls(&mut self, redirect_urls: Vec<String>) {
        self.redirect_urls = redirect_urls;
    }

    /// Returns the HTTP status.
    ///
    /// # Returns
    ///
    /// Response status code.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns response headers.
    ///
    /// # Returns
    ///
    /// Borrowed response header map.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Returns response body bytes.
    ///
    /// # Returns
    ///
    /// Cloned body bytes.
    pub async fn bytes(&self) -> Result<Bytes, ArachneaHttpError> {
        Ok(self.body.clone())
    }

    /// Decodes the response body as text using the charset declared in `Content-Type`.
    ///
    /// When the `Content-Type` header specifies a charset (e.g. `text/html; charset=iso-8859-1`),
    /// the body is decoded using that encoding via `encoding_rs`. If no charset is declared
    /// or the declared encoding is unknown, the body is first tried as UTF-8; if that fails,
    /// it falls back to `windows-1252` (the superset of ISO-8859-1 commonly used for legacy
    /// HTML pages), then finally to lossy UTF-8.
    ///
    /// # Returns
    ///
    /// The response body as a `String`.
    ///
    /// # Errors
    ///
    /// Always succeeds — the body is always decodable by at least one fallback strategy.
    pub async fn text(&self) -> Result<String, ArachneaHttpError> {
        // 1. Try the charset declared in Content-Type (e.g. iso-8859-1, utf-8)
        let encoding = self
            .headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|ct| charset_from_content_type(ct))
            .and_then(|label| Encoding::for_label(label.as_bytes()));

        let output = if let Some(encoding) = encoding {
            let (decoded, _, _) = encoding.decode(&self.body);
            decoded.to_string()
        } else {
            // 2. Try strict UTF-8 first (common case)
            let bytes = &self.body;
            if let Ok(s) = std::str::from_utf8(bytes) {
                s.to_string()
            } else {
                // 3. Fallback to windows-1252 (ISO-8859-1 superset, many legacy sites)
                let (decoded, _, _) = WINDOWS_1252.decode(bytes);
                decoded.to_string()
            }
        };

        Ok(output)
    }

    /// Parses the response body as JSON.
    ///
    /// # Returns
    ///
    /// The deserialized JSON payload.
    ///
    /// # Errors
    ///
    /// Returns `Json` when the response body cannot be parsed.
    pub fn json<T>(&self) -> Result<T, ArachneaHttpError>
    where
        T: DeserializeOwned,
    {
        serde_json::from_slice(&self.body).map_err(|err| ArachneaHttpError::Json(err.to_string()))
    }

    /// Returns a bounded preview of the body for detection.
    ///
    /// # Returns
    ///
    /// At most the first 4096 body bytes.
    fn preview_body(&self) -> &[u8] {
        let end = self.body.len().min(4096);
        &self.body[..end]
    }
}

/// Normalizes a URL or origin string to an origin URL.
///
/// This function takes a URL or origin string and normalizes it to a consistent
/// origin format (scheme://host:port/). It's used throughout the client to ensure
/// consistent cookie and Cloudflare state management.
///
/// # Parameters
///
/// - `value`: Absolute URL or origin string to normalize
///
/// # Returns
///
/// A normalized origin URL string in the format `scheme://host:port/`
///
/// # Errors
///
/// Returns `InvalidUrl` when parsing fails or the URL has no host
///
/// # Examples
///
/// ```
/// # use arachnea_http::error::ArachneaHttpError;
/// # fn example() -> Result<(), ArachneaHttpError> {
/// let origin = origin_url("https://example.com/path?query=value")?;
/// assert_eq!(origin, "https://example.com/");
/// # Ok(())
/// # }
/// ```
fn origin_url(value: &str) -> Result<String, ArachneaHttpError> {
    let url = Url::parse(value).map_err(|err| ArachneaHttpError::InvalidUrl(err.to_string()))?;
    let scheme = url.scheme();
    let host = url
        .host_str()
        .ok_or_else(|| ArachneaHttpError::InvalidUrl("URL must contain a host".to_string()))?;
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    Ok(format!("{scheme}://{host}{port}/"))
}

/// Builds a header map from textual header names and values.
///
/// This helper function converts string-based header name/value pairs into a
/// validated `HeaderMap`. It's useful for callers that have headers in string
/// format (e.g., from configuration files) and need to convert them for use
/// with the HTTP client.
///
/// # Parameters
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

/// Extracts the next redirect target URL from a response, if one exists.
///
/// Returns None when the response is not a redirection or has no Location`n/// header. Resolves relative locations against the response URL.
///
/// # Parameters
///
/// - Response: Materialized response to inspect.
///
/// # Returns
///
/// An absolute redirect target URL, or None.
///
/// # Errors
///
/// Returns InvalidHeader or InvalidUrl when the Location header or its
/// resolution fails.
fn redirect_target(response: &ArachneaResponse) -> Result<Option<String>, ArachneaHttpError> {
    if !response.status.is_redirection() {
        return Ok(None);
    }

    let Some(location) = response.headers.get(LOCATION) else {
        return Ok(None);
    };
    let location = location
        .to_str()
        .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?;
    let base =
        Url::parse(&response.url).map_err(|err| ArachneaHttpError::InvalidUrl(err.to_string()))?;
    let next = base
        .join(location)
        .map_err(|err| ArachneaHttpError::InvalidUrl(err.to_string()))?;
    Ok(Some(next.to_string()))
}

/// Determines whether a redirect status code should rewrite the method to GET.
///
/// POST, PUT, and PATCH are rewritten to GET for 303 See Other. 302 Found and
/// 301 Moved Permanently are also rewritten except when the original method is
/// GET or HEAD.
///
/// # Parameters
///
/// - status: HTTP redirect status code.
/// - method: Original request method.
///
/// # Returns
///
/// 	rue when the method should be rewritten to GET for the next redirect hop.
fn should_rewrite_redirect_to_get(status: StatusCode, method: &Method) -> bool {
    if *method == Method::GET || *method == Method::HEAD {
        return false;
    }

    status == StatusCode::SEE_OTHER
        || status == StatusCode::FOUND
        || status == StatusCode::MOVED_PERMANENTLY
}

/// Builds a redacted header map suitable for logging or diagnostics.
///
/// Sensitive headers such as uthorization and cookie are replaced with
/// <redacted>. Non-UTF-8 values are replaced with <non-utf8>.`n///
/// # Parameters
///
/// - headers: Original header map to redact.
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
/// Unit tests for client URL helpers.
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use http::header::SET_COOKIE;

    use crate::engine::{EngineResponse, SOLVER_USER_AGENT_HEADER};

    use super::*;

    /// Minimal injected engine used by client tests.
    #[derive(Clone)]
    struct StaticEngine {
        /// Number of requests observed by the engine.
        calls: Arc<AtomicUsize>,
    }

    /// Injected engine that distinguishes refresh calls from regular sends.
    #[derive(Clone)]
    struct RefreshEngine {
        /// Number of regular sends observed by the engine.
        send_calls: Arc<AtomicUsize>,
        /// Number of Cloudflare refresh calls observed by the engine.
        refresh_calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl HttpEngine for StaticEngine {
        /// Returns the test engine name.
        ///
        /// # Returns
        ///
        /// The static test engine name.
        fn name(&self) -> &'static str {
            "static-test"
        }

        /// Sends a deterministic in-memory response.
        ///
        /// # Parameters
        ///
        /// - `request`: Request passed by the client.
        ///
        /// # Returns
        ///
        /// A response containing the requested URL and a tiny body.
        ///
        /// # Errors
        ///
        /// This test engine never fails.
        async fn send(&self, request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(EngineResponse {
                url: request.url,
                status: StatusCode::OK,
                headers: HeaderMap::new(),
                body: Bytes::from_static(b"injected"),
            })
        }
    }

    #[async_trait::async_trait]
    impl HttpEngine for RefreshEngine {
        /// Returns the refresh test engine name.
        ///
        /// # Returns
        ///
        /// The static test engine name.
        fn name(&self) -> &'static str {
            "refresh-test"
        }

        /// Records an unexpected regular send.
        ///
        /// # Parameters
        ///
        /// - `request`: Request passed by the client.
        ///
        /// # Returns
        ///
        /// A tiny successful response.
        ///
        /// # Errors
        ///
        /// This test engine never fails.
        async fn send(&self, request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError> {
            self.send_calls.fetch_add(1, Ordering::SeqCst);
            Ok(EngineResponse {
                url: request.url,
                status: StatusCode::OK,
                headers: HeaderMap::new(),
                body: Bytes::new(),
            })
        }

        /// Records a Cloudflare refresh and returns cookie plus user-agent metadata.
        ///
        /// # Parameters
        ///
        /// - `request`: Refresh request passed by the client.
        ///
        /// # Returns
        ///
        /// A response containing a clearance cookie and solver user-agent.
        ///
        /// # Errors
        ///
        /// This test engine never fails.
        async fn refresh_cloudflare(
            &self,
            request: EngineRequest,
        ) -> Result<EngineResponse, ArachneaHttpError> {
            self.refresh_calls.fetch_add(1, Ordering::SeqCst);
            let mut headers = HeaderMap::new();
            headers.insert(
                SET_COOKIE,
                "cf_clearance=ok; Max-Age=60; Path=/"
                    .parse()
                    .expect("valid Set-Cookie header"),
            );
            headers.insert(
                SOLVER_USER_AGENT_HEADER,
                "SolverUA/1".parse().expect("valid user-agent metadata"),
            );
            Ok(EngineResponse {
                url: request.url,
                status: StatusCode::OK,
                headers,
                body: Bytes::new(),
            })
        }
    }

    /// Verifies origin normalization keeps the scheme, host, and port.
    #[test]
    fn origin_url_keeps_authority() {
        assert_eq!(
            origin_url("https://example.com:8443/path?q=1").expect("valid origin"),
            "https://example.com:8443/"
        );
    }

    /// Verifies that a directly injected engine handles application requests.
    #[tokio::test]
    async fn injected_engine_executes_requests() {
        let calls = Arc::new(AtomicUsize::new(0));
        let engine = StaticEngine {
            calls: calls.clone(),
        };
        let config = ArachneaHttpConfig::builder()
            .engine_instance(engine)
            .build()
            .expect("valid config");
        let client = ArachneaHttpClient::new(config).await.expect("client");

        let response = client
            .get("https://example.com/custom")
            .send()
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.bytes().await.expect("body"),
            Bytes::from_static(b"injected")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// Verifies explicit Cloudflare refresh uses the solver refresh hook.
    #[tokio::test]
    async fn refresh_cloudflare_uses_refresh_hook_and_solver_user_agent() {
        let send_calls = Arc::new(AtomicUsize::new(0));
        let refresh_calls = Arc::new(AtomicUsize::new(0));
        let engine = RefreshEngine {
            send_calls: send_calls.clone(),
            refresh_calls: refresh_calls.clone(),
        };
        let config = ArachneaHttpConfig::builder()
            .engine_instance(StaticEngine {
                calls: Arc::new(AtomicUsize::new(0)),
            })
            .cloudflare_solver_instance(engine)
            .build()
            .expect("valid config");
        let client = ArachneaHttpClient::new(config).await.expect("client");

        client
            .refresh_cloudflare("https://refresh-hook.example/protected")
            .await
            .expect("refresh succeeds");

        let request = client
            .engine_request(RequestOptions {
                method: Method::GET,
                url: "https://refresh-hook.example/again".to_string(),
                mode: None,
                headers: HeaderMap::new(),
                body: None,
            })
            .await
            .expect("engine request");

        assert_eq!(send_calls.load(Ordering::SeqCst), 0);
        assert_eq!(refresh_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            request.headers.get(USER_AGENT).expect("user-agent"),
            "SolverUA/1"
        );
        assert_eq!(
            request.headers.get(COOKIE).expect("cookie"),
            "cf_clearance=ok"
        );
    }
}
