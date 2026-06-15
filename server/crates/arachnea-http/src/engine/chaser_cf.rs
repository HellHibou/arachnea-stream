use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use bytes::Bytes;
use chaser_cf::{core::BrowserManager, ChaserConfig, Cookie as ChaserCookie, WafSession};
use chaser_oxide::{
    cdp::browser_protocol::dom::{GetBoxModelParams, GetDocumentParams, Node, NodeId},
    cdp::browser_protocol::network::EventRequestWillBeSent,
    cdp::browser_protocol::network::{Headers, SetExtraHttpHeadersParams},
    cdp::browser_protocol::page::NavigateParams,
    ChaserPage,
};
use futures::StreamExt;
use http::{
    header::{CONTENT_TYPE, REFERER, SET_COOKIE},
    HeaderMap, HeaderValue, Method, StatusCode,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};
use url::Url;

use crate::{
    config::ArachneaHttpConfig,
    engine::{EngineRequest, EngineResponse, HttpEngine, SOLVER_USER_AGENT_HEADER},
    error::ArachneaHttpError,
};

/// Registry name for the chaser-cf engine.
pub const ENGINE_NAME: &str = "chaser-cf";

/// Default margin before a cached Cloudflare session is considered stale.
const DEFAULT_SESSION_CACHE_REFRESH_MARGIN: Duration = Duration::from_secs(300);

/// File name used by the default persistent chaser-cf session cache.
const DEFAULT_SESSION_CACHE_FILE_NAME: &str = "chaser-cf-sessions.json";

/// Minimum time to keep sampling source after chaser-cf clears Cloudflare.
const DEFAULT_SOURCE_MIN_WAIT: Duration = Duration::from_secs(4);

/// Interval between page source sampling attempts.
const DEFAULT_SOURCE_POLL_INTERVAL: Duration = Duration::from_millis(1_000);

/// Time the page source must remain unchanged before it is returned.
const DEFAULT_SOURCE_STABILITY: Duration = Duration::from_millis(2_500);

/// `chaser-cf` engine adapter for browser-backed Cloudflare solving.
pub struct ChaserCfEngine {
    /// Native chaser-cf browser automation configuration.
    config: ChaserConfig,
    /// Reused chaser-cf browser manager.
    browser: Arc<RwLock<Option<Arc<BrowserManager>>>>,
    /// Whether the chaser-cf browser manager was initialized.
    initialized: Arc<RwLock<bool>>,
    /// Optional persistent session cache used before opening Chrome again.
    session_cache: Option<ChaserSessionCache>,
}

/// Persistent Cloudflare sessions indexed by normalized origin.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ChaserSessionCacheFile {
    /// Cached sessions keyed by origin URL.
    sessions: HashMap<String, CachedChaserSession>,
}

/// Serialized Cloudflare session extracted from chaser-cf.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedChaserSession {
    /// Serialized `Set-Cookie` headers produced from browser cookies.
    set_cookie_headers: Vec<String>,
    /// Browser user-agent observed by chaser-cf.
    user_agent: Option<String>,
    /// Unix timestamp for `cf_clearance` expiration when available.
    clearance_expires_at: Option<u64>,
    /// Unix timestamp when the session was stored.
    stored_at: u64,
}

/// File-backed cache for chaser-cf Cloudflare sessions.
struct ChaserSessionCache {
    /// JSON cache file path.
    path: PathBuf,
    /// Proactive refresh margin before `cf_clearance` expiry.
    refresh_margin: Duration,
    /// In-memory cache loaded from disk.
    entries: Mutex<ChaserSessionCacheFile>,
}

impl Default for ChaserCfEngine {
    fn default() -> Self {
        let config = ArachneaHttpConfig::builder().build().unwrap();
        ChaserCfEngine::new(&config).unwrap()
    }
}

impl ChaserCfEngine {
    /// Creates a configured `chaser-cf` engine.
    ///
    /// # Parameters
    ///
    /// - `config`: Client configuration used to derive chaser-cf options.
    ///
    /// # Returns
    ///
    /// A configured chaser-cf adapter.
    ///
    /// # Errors
    ///
    /// This constructor keeps browser initialization lazy. Runtime browser
    /// failures are reported by request execution methods.
    pub fn new(config: &ArachneaHttpConfig) -> Result<Self, ArachneaHttpError> {
        let chaser_config = ChaserConfig::from_env()
            .with_timeout(config.request_timeout)
            .with_lazy_init(true);
        Ok(Self {
            config: chaser_config,
            browser: Arc::new(RwLock::new(None)),
            initialized: Arc::new(RwLock::new(false)),
            session_cache: None,
        }
        .with_default_session_cache()
        .with_session_cache_refresh_margin(config.cookie_refresh_margin))
    }

    /// Creates a `chaser-cf` engine from native chaser-cf configuration.
    ///
    /// # Parameters
    ///
    /// - `config`: chaser-cf-specific browser automation configuration.
    ///
    /// # Returns
    ///
    /// A configured chaser-cf adapter.
    ///
    /// # Errors
    ///
    /// Returns `ChaserCfFailure` when chaser-cf cannot be constructed.
    pub async fn from_chaser_config(config: ChaserConfig) -> Result<Self, ArachneaHttpError> {
        let eager_browser = if config.lazy_init {
            None
        } else {
            Some(Arc::new(BrowserManager::new(&config).await.map_err(
                |err| ArachneaHttpError::ChaserCfFailure(err.to_string()),
            )?))
        };
        let engine = Self {
            config: config.clone(),
            initialized: Arc::new(RwLock::new(eager_browser.is_some())),
            browser: Arc::new(RwLock::new(eager_browser)),
            session_cache: None,
        };
        Ok(engine)
    }

    /// Enables a persistent session cache at the default cache path.
    ///
    /// The cache stores Cloudflare cookies and the browser user-agent so a
    /// later process can reuse a still-valid `cf_clearance` without opening
    /// Chrome again.
    ///
    /// # Returns
    ///
    /// The updated chaser-cf adapter.
    pub fn with_default_session_cache(self) -> Self {
        self.with_session_cache_path(default_session_cache_path())
    }

    /// Enables a persistent session cache at `path`.
    ///
    /// # Parameters
    ///
    /// - `path`: JSON file used to store Cloudflare sessions.
    ///
    /// # Returns
    ///
    /// The updated chaser-cf adapter.
    pub fn with_session_cache_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.session_cache = Some(ChaserSessionCache::new(
            path.into(),
            DEFAULT_SESSION_CACHE_REFRESH_MARGIN,
        ));
        self
    }

    /// Updates the refresh margin used by the persistent session cache.
    ///
    /// If no persistent cache has been enabled yet, this method only keeps the
    /// engine unchanged.
    ///
    /// # Parameters
    ///
    /// - `margin`: Duration before expiry where a cached session is considered
    ///   stale.
    ///
    /// # Returns
    ///
    /// The updated chaser-cf adapter.
    pub fn with_session_cache_refresh_margin(mut self, margin: Duration) -> Self {
        if let Some(cache) = &mut self.session_cache {
            cache.refresh_margin = margin;
        }
        self
    }

    /// Disables the persistent session cache.
    ///
    /// # Returns
    ///
    /// The updated chaser-cf adapter.
    pub fn without_session_cache(mut self) -> Self {
        self.session_cache = None;
        self
    }

    /// Initializes the underlying chaser-cf browser manager.
    ///
    /// # Errors
    ///
    /// Returns `ChaserCfFailure` when Chrome cannot be started by chaser-cf.
    async fn init_browser(&self) -> Result<(), ArachneaHttpError> {
        let mut initialized = self.initialized.write().await;
        if *initialized {
            return Ok(());
        }

        let manager = BrowserManager::new(&self.config)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let mut browser = self.browser.write().await;
        *browser = Some(Arc::new(manager));
        *initialized = true;
        Ok(())
    }

    /// Returns the chaser-cf browser manager, initializing it lazily when needed.
    ///
    /// # Errors
    ///
    /// Returns `ChaserCfFailure` when initialization fails or the browser has
    /// already stopped.
    async fn browser(&self) -> Result<Arc<BrowserManager>, ArachneaHttpError> {
        if !*self.initialized.read().await {
            self.init_browser().await?;
        }

        let browser = self.browser.read().await;
        let manager = browser.as_ref().ok_or_else(|| {
            ArachneaHttpError::ChaserCfFailure("chaser-cf browser is not initialized".to_string())
        })?;
        if !manager.is_healthy() {
            return Err(ArachneaHttpError::ChaserCfFailure(
                "chaser-cf browser is no longer healthy".to_string(),
            ));
        }
        Ok(manager.clone())
    }

    /// Opens one chaser-cf page, solves Cloudflare, and optionally collects stable HTML.
    ///
    /// # Parameters
    ///
    /// - `url`: URL to open in the browser.
    /// - `collect_body`: Whether to wait for stable page source before closing.
    /// - `custom_headers`: Optional extra HTTP headers to set before navigating
    ///   (for example `Referer`).
    ///
    /// # Returns
    ///
    /// The solved WAF session and optional page body.
    ///
    /// # Errors
    ///
    /// Returns chaser-cf failures, missing clearance, or page-source failures.
    async fn solve_browser_page(
        &self,
        url: &str,
        collect_body: bool,
        custom_headers: Option<&HeaderMap>,
    ) -> Result<(WafSession, Bytes), ArachneaHttpError> {
        let manager = self.browser().await?;
        let _permit = manager
            .acquire_permit()
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let ctx_id = manager
            .create_context(None)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let (page, chaser) = manager
            .new_page(ctx_id, "about:blank")
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;

        let result = async {
            let expected_referer = custom_headers
                .and_then(|headers| headers.get(REFERER))
                .and_then(|value| value.to_str().ok())
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned);
            let request_probe =
                Self::spawn_navigation_header_probe(&page, url, expected_referer.clone()).await?;
            Self::apply_custom_headers(&page, custom_headers).await?;
            page.goto(Self::navigate_params(url, custom_headers))
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
            if let Some(probe) = request_probe {
                let _ = probe.await;
            }

            let body = self
                .wait_for_clearance_and_stable_source(url, &page, &chaser, collect_body)
                .await?;

            let raw_cookies = page
                .get_cookies()
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
            let cookies = raw_cookies
                .into_iter()
                .map(|cookie| ChaserCookie {
                    name: cookie.name,
                    value: cookie.value,
                    domain: Some(cookie.domain),
                    path: Some(cookie.path),
                    expires: Some(cookie.expires),
                    http_only: Some(cookie.http_only),
                    secure: Some(cookie.secure),
                    same_site: cookie.same_site.map(|value| format!("{value:?}")),
                })
                .collect::<Vec<_>>();
            let user_agent = chaser
                .evaluate("navigator.userAgent")
                .await
                .ok()
                .and_then(|value| value?.as_str().map(str::to_owned))
                .unwrap_or_default();
            let mut headers = HashMap::new();
            headers.insert("user-agent".to_string(), user_agent);

            Ok((WafSession::new(cookies, headers), body))
        }
        .await;

        if let Err(err) = page.close().await {
            warn!(error = %err, "failed to close chaser-cf page");
        }

        result
    }

    /// Applies extra HTTP headers to the current browser page before navigation.
    ///
    /// # Parameters
    ///
    /// - `page`: Browser page that will perform the request.
    /// - `custom_headers`: Optional extra headers to attach.
    ///
    /// # Errors
    ///
    /// Returns `InvalidHeader` when a header value is not valid UTF-8, or
    /// `ChaserCfFailure` when Chrome rejects the header override.
    async fn apply_custom_headers(
        page: &chaser_oxide::Page,
        custom_headers: Option<&HeaderMap>,
    ) -> Result<(), ArachneaHttpError> {
        let Some(custom_headers) = custom_headers else {
            return Ok(());
        };

        let mut headers = HashMap::new();
        for (name, value) in custom_headers {
            if *name == REFERER {
                continue;
            }
            let value = value
                .to_str()
                .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?;
            headers.insert(name.as_str().to_string(), value.to_string());
        }

        if headers.is_empty() {
            return Ok(());
        }

        let payload = serde_json::to_value(headers)
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        page.execute(SetExtraHttpHeadersParams::new(Headers::new(payload)))
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;

        Ok(())
    }

    /// Starts a short-lived probe that logs the outbound `Referer` observed by
    /// Chrome for the main navigation request.
    ///
    /// # Parameters
    ///
    /// - `page`: Browser page that will perform the navigation.
    /// - `url`: Target URL expected for the main document request.
    /// - `expected_referer`: Referer value the caller expects Chrome to send.
    ///
    /// # Errors
    ///
    /// Returns `ChaserCfFailure` when the event listener cannot be registered.
    async fn spawn_navigation_header_probe(
        page: &chaser_oxide::Page,
        url: &str,
        expected_referer: Option<String>,
    ) -> Result<Option<tokio::task::JoinHandle<()>>, ArachneaHttpError> {
        let Some(expected_referer) = expected_referer else {
            return Ok(None);
        };

        let mut events = page
            .event_listener::<EventRequestWillBeSent>()
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let expected_url = url.to_string();
        let timeout_url = expected_url.clone();
        let timeout_referer = expected_referer.clone();
        Ok(Some(tokio::spawn(async move {
            let probe = tokio::time::timeout(std::time::Duration::from_secs(5), async move {
                while let Some(event) = events.next().await {
                    if event.request.url != expected_url {
                        continue;
                    }
                    let observed_referer =
                        lookup_json_header_value(event.request.headers.inner(), "Referer");
                    info!(
                        url = %event.request.url,
                        expected_referer = %expected_referer,
                        observed_referer = observed_referer.as_deref().unwrap_or("<missing>"),
                        referrer_policy = %event.request.referrer_policy.as_ref(),
                        "chaser-cf observed outbound navigation request"
                    );
                    return;
                }
            })
            .await;

            if probe.is_err() {
                warn!(
                    url = %timeout_url,
                    expected_referer = %timeout_referer,
                    "chaser-cf did not observe the outbound navigation request before timeout"
                );
            }
        })))
    }

    /// Builds native navigation parameters for Chrome.
    ///
    /// `Referer` is passed through `Page.navigate.referrer` because Chrome may
    /// reject top-level navigation when it is injected as an extra HTTP header.
    fn navigate_params(url: &str, custom_headers: Option<&HeaderMap>) -> NavigateParams {
        let mut params = NavigateParams::new(url);
        if let Some(referer) = custom_headers
            .and_then(|headers| headers.get(REFERER))
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.trim().is_empty())
        {
            params.referrer = Some(referer.to_string());
        }
        params
    }

    /// Waits for Cloudflare clearance and, when requested, stable page source.
    ///
    /// The polling and Turnstile click happen in the same browser tab so one
    /// request cannot create a loop of new chaser-cf pages.
    async fn wait_for_clearance_and_stable_source(
        &self,
        url: &str,
        page: &chaser_oxide::Page,
        chaser: &ChaserPage,
        collect_body: bool,
    ) -> Result<Bytes, ArachneaHttpError> {
        const PASSIVE_WAIT: Duration = Duration::from_secs(6);
        const CLICK_INTERVAL: Duration = Duration::from_millis(1_200);
        const CLEARANCE_STABILIZATION: Duration = Duration::from_millis(500);

        let started = Instant::now();
        let timeout = self.config.timeout();
        let mut last_click = started
            .checked_sub(Duration::from_secs(30))
            .unwrap_or(started);
        let mut clearance_seen_at: Option<Instant> = None;
        let mut last_signature: Option<(usize, u64)> = None;
        let mut stable_since: Option<Instant> = None;
        let mut latest_non_challenge_body: Option<Bytes> = None;

        loop {
            let mut should_click_challenge = false;
            if has_clearance_cookie(page).await {
                let seen_at = *clearance_seen_at.get_or_insert_with(Instant::now);
                if seen_at.elapsed() >= CLEARANCE_STABILIZATION {
                    let source = chaser
                        .content()
                        .await
                        .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
                    let body_is_usable =
                        !source.trim().is_empty() && !looks_like_cloudflare_interstitial(&source);
                    if !collect_body && body_is_usable {
                        return Ok(Bytes::new());
                    } else if collect_body && body_is_usable {
                        let body = Bytes::from(source.clone());
                        latest_non_challenge_body = Some(body.clone());
                        let signature = page_source_signature(&source);
                        if last_signature == Some(signature) {
                            let stable_since = stable_since.get_or_insert_with(Instant::now);
                            if seen_at.elapsed() >= DEFAULT_SOURCE_MIN_WAIT
                                && stable_since.elapsed() >= DEFAULT_SOURCE_STABILITY
                            {
                                return Ok(body);
                            }
                        } else {
                            last_signature = Some(signature);
                            stable_since = Some(Instant::now());
                        }
                    } else {
                        last_signature = None;
                        stable_since = None;
                        should_click_challenge = true;
                    }
                }
            } else {
                clearance_seen_at = None;
                last_signature = None;
                stable_since = None;
                should_click_challenge = true;
            }

            if should_click_challenge
                && started.elapsed() >= PASSIVE_WAIT
                && last_click.elapsed() >= CLICK_INTERVAL
            {
                try_click_challenge(chaser).await;
                last_click = Instant::now();
            }

            if started.elapsed() >= timeout {
                if let Some(body) = latest_non_challenge_body {
                    return Ok(body);
                }
                return Err(ArachneaHttpError::CookieAbsent {
                    origin: url.to_string(),
                    name: "cf_clearance".to_string(),
                });
            }

            tokio::time::sleep(DEFAULT_SOURCE_POLL_INTERVAL).await;
        }
    }

    /// Solves a Cloudflare WAF session with chaser-cf's automatic Turnstile handling.
    ///
    /// # Parameters
    ///
    /// - `url`: URL used to create the WAF session.
    ///
    /// # Returns
    ///
    /// The solved WAF session containing cookies and browser metadata.
    ///
    /// # Errors
    ///
    /// Returns chaser-cf failures or missing `cf_clearance`.
    async fn solve_waf_session(
        &self,
        url: &str,
        custom_headers: Option<&HeaderMap>,
    ) -> Result<WafSession, ArachneaHttpError> {
        let (session, _body) = self.solve_browser_page(url, false, custom_headers).await?;
        if !session
            .cookies
            .iter()
            .any(|cookie| cookie.name == "cf_clearance")
        {
            return Err(ArachneaHttpError::CookieAbsent {
                origin: url.to_string(),
                name: "cf_clearance".to_string(),
            });
        }
        Ok(session)
    }

    /// Builds response headers containing cookies extracted by chaser-cf.
    ///
    /// # Parameters
    ///
    /// - `cookies`: Cookies extracted from the browser session.
    ///
    /// # Returns
    ///
    /// A header map containing one `Set-Cookie` header per cookie.
    ///
    /// # Errors
    ///
    /// Returns `InvalidHeader` when a synthesized `Set-Cookie` value cannot be
    /// represented as an HTTP header.
    fn response_headers(cookies: &[ChaserCookie]) -> Result<HeaderMap, ArachneaHttpError> {
        if !cookies.iter().any(|cookie| cookie.name == "cf_clearance") {
            return Err(ArachneaHttpError::CookieAbsent {
                origin: "chaser-cf browser session".to_string(),
                name: "cf_clearance".to_string(),
            });
        }

        let mut headers = HeaderMap::new();
        for cookie in cookies {
            let set_cookie = set_cookie_header(cookie);
            headers.append(
                SET_COOKIE,
                HeaderValue::from_str(&set_cookie)
                    .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
            );
        }
        Ok(headers)
    }

    /// Adds the browser user-agent reported by chaser-cf to response metadata.
    ///
    /// # Parameters
    ///
    /// - `headers`: Header map that will carry internal solver metadata.
    /// - `session`: WAF session returned by chaser-cf.
    ///
    /// # Errors
    ///
    /// Returns `InvalidHeader` when the user-agent cannot be represented as a
    /// header value.
    fn insert_solver_user_agent(
        headers: &mut HeaderMap,
        session: &WafSession,
    ) -> Result<(), ArachneaHttpError> {
        let Some(user_agent) = session
            .headers
            .get("user-agent")
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(());
        };
        headers.insert(
            SOLVER_USER_AGENT_HEADER,
            HeaderValue::from_str(user_agent)
                .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
        );
        Ok(())
    }

    /// Returns cached headers for a still-valid Cloudflare session.
    ///
    /// # Parameters
    ///
    /// - `url`: URL whose origin should be resolved from the cache.
    ///
    /// # Returns
    ///
    /// Cached headers when a reusable session exists.
    fn cached_session_headers(&self, url: &str) -> Option<HeaderMap> {
        let Some(cache) = &self.session_cache else {
            return None;
        };
        match cache.headers_for_url(url) {
            Ok(headers) => headers,
            Err(err) => {
                warn!(error = %err, "failed to read chaser-cf session cache");
                None
            }
        }
    }

    /// Stores a newly solved Cloudflare session in the optional cache.
    ///
    /// # Parameters
    ///
    /// - `url`: URL whose origin owns the Cloudflare session.
    /// - `session`: chaser-cf WAF session to persist.
    fn store_session_cache(&self, url: &str, session: &WafSession) {
        let Some(cache) = &self.session_cache else {
            return;
        };
        if let Err(err) = cache.store_session(url, session) {
            warn!(error = %err, "failed to write chaser-cf session cache");
        }
    }

    /// Refreshes Cloudflare cookies, optionally ignoring the persistent session cache.
    ///
    /// # Parameters
    ///
    /// - `request`: Normalized refresh request.
    /// - `use_session_cache`: Whether a valid cached chaser-cf session can
    ///   satisfy the refresh without opening Chrome.
    ///
    /// # Returns
    ///
    /// A normalized response carrying synthesized cookies and solver metadata.
    ///
    /// # Errors
    ///
    /// Returns unsupported operation or chaser-cf failures.
    async fn refresh_cloudflare_with_cache_policy(
        &self,
        request: EngineRequest,
        use_session_cache: bool,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        if request.method != Method::GET && request.method != Method::HEAD {
            return Err(ArachneaHttpError::UnsupportedEngineOperation {
                engine: self.name(),
                operation: "non-GET Cloudflare refresh requests",
            });
        }

        if use_session_cache {
            if let Some(headers) = self.cached_session_headers(&request.url) {
                return Ok(EngineResponse {
                    url: request.url,
                    status: StatusCode::OK,
                    headers,
                    body: Bytes::new(),
                });
            }
        }

        let session = self
            .solve_waf_session(&request.url, Some(&request.headers))
            .await?;
        let mut headers = Self::response_headers(&session.cookies)?;
        Self::insert_solver_user_agent(&mut headers, &session)?;
        self.store_session_cache(&request.url, &session);

        Ok(EngineResponse {
            url: request.url,
            status: StatusCode::OK,
            headers,
            body: Bytes::new(),
        })
    }
}

impl Drop for ChaserCfEngine {
    fn drop(&mut self) {
        if let Ok(browser) = self.browser.try_read() {
            if browser.is_some() {
                warn!(
                    "ChaserCfEngine dropped with an active browser; chaser-cf does not expose async drop"
                );
            }
        }
    }
}

#[async_trait]
impl HttpEngine for ChaserCfEngine {
    /// Returns the chaser-cf engine name.
    ///
    /// # Returns
    ///
    /// The static engine name.
    fn name(&self) -> &'static str {
        ENGINE_NAME
    }

    /// Sends a request with `chaser-cf`.
    ///
    /// # Parameters
    ///
    /// - `request`: Normalized request to execute.
    ///
    /// # Returns
    ///
    /// A normalized response containing cookies extracted from the browser
    /// session. `GET` requests also include page source after Cloudflare has
    /// been cleared.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` for methods that chaser-cf cannot
    /// execute directly, or `ChaserCfFailure` for browser automation failures.
    async fn send(&self, request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError> {
        if request.method != Method::GET && request.method != Method::HEAD {
            return Err(ArachneaHttpError::UnsupportedEngineOperation {
                engine: self.name(),
                operation: "non-GET HTTP requests",
            });
        }

        let (session, body) = self
            .solve_browser_page(
                &request.url,
                request.method == Method::GET,
                Some(&request.headers),
            )
            .await?;
        let mut headers = Self::response_headers(&session.cookies)?;
        Self::insert_solver_user_agent(&mut headers, &session)?;
        self.store_session_cache(&request.url, &session);
        let body = if request.method == Method::GET {
            headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            );
            body
        } else {
            Bytes::new()
        };

        Ok(EngineResponse {
            url: request.url,
            status: StatusCode::OK,
            headers,
            body,
        })
    }

    /// Refreshes Cloudflare cookies with chaser-cf's automatic WAF solver.
    ///
    /// # Parameters
    ///
    /// - `request`: Normalized refresh request.
    ///
    /// # Returns
    ///
    /// A normalized response carrying synthesized `Set-Cookie` headers and the
    /// solver-observed user-agent.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` for methods that chaser-cf cannot
    /// execute directly, or `ChaserCfFailure` for browser automation failures.
    async fn refresh_cloudflare(
        &self,
        request: EngineRequest,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        self.refresh_cloudflare_with_cache_policy(request, true)
            .await
    }

    /// Refreshes Cloudflare cookies by opening Chrome instead of reusing a cached session.
    ///
    /// # Parameters
    ///
    /// - `request`: Normalized refresh request.
    ///
    /// # Returns
    ///
    /// A normalized response carrying synthesized `Set-Cookie` headers and the
    /// solver-observed user-agent.
    ///
    /// # Errors
    ///
    /// Returns unsupported operation or chaser-cf failures.
    async fn refresh_cloudflare_fresh(
        &self,
        request: EngineRequest,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        self.refresh_cloudflare_with_cache_policy(request, false)
            .await
    }
}

impl ChaserSessionCache {
    /// Creates a cache backed by `path`.
    ///
    /// # Parameters
    ///
    /// - `path`: JSON file used to load and persist sessions.
    /// - `refresh_margin`: Proactive refresh margin before expiry.
    ///
    /// # Returns
    ///
    /// A file-backed chaser-cf session cache.
    fn new(path: PathBuf, refresh_margin: Duration) -> Self {
        let entries = load_session_cache(&path).unwrap_or_else(|err| {
            warn!(path = %path.display(), error = %err, "failed to load chaser-cf session cache");
            ChaserSessionCacheFile::default()
        });
        Self {
            path,
            refresh_margin,
            entries: Mutex::new(entries),
        }
    }

    /// Returns cached headers for `url` when a valid session exists.
    ///
    /// # Parameters
    ///
    /// - `url`: URL whose origin should be looked up.
    ///
    /// # Returns
    ///
    /// Cached headers when the origin has a reusable `cf_clearance`.
    ///
    /// # Errors
    ///
    /// Returns URL, lock, or header conversion failures.
    fn headers_for_url(&self, url: &str) -> Result<Option<HeaderMap>, ArachneaHttpError> {
        let origin = cache_origin_key(url)?;
        let mut entries = self.entries.lock().map_err(|_| {
            ArachneaHttpError::ChaserCfFailure(
                "chaser-cf session cache lock was poisoned".to_string(),
            )
        })?;
        let Some(session) = entries.sessions.get(&origin) else {
            return Ok(None);
        };
        if !session.is_usable(self.refresh_margin) {
            entries.sessions.remove(&origin);
            if let Err(err) = self.persist_entries(&entries) {
                warn!(path = %self.path.display(), error = %err, "failed to prune chaser-cf session cache");
            }
            return Ok(None);
        }
        cached_session_headers(session).map(Some)
    }

    /// Stores a chaser-cf session for `url`.
    ///
    /// Sessions that do not contain `cf_clearance` are ignored.
    ///
    /// # Parameters
    ///
    /// - `url`: URL whose origin owns the session.
    /// - `session`: chaser-cf session to serialize.
    ///
    /// # Errors
    ///
    /// Returns URL, lock, serialization, or file write failures.
    fn store_session(&self, url: &str, session: &WafSession) -> Result<(), String> {
        let origin = cache_origin_key(url).map_err(|err| err.to_string())?;
        let Some(session) = CachedChaserSession::from_waf_session(session) else {
            return Ok(());
        };
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "chaser-cf session cache lock was poisoned".to_string())?;
        entries.sessions.insert(origin, session);
        self.persist_entries(&entries)
    }

    /// Persists cache entries to disk.
    ///
    /// # Parameters
    ///
    /// - `entries`: Cache entries to serialize.
    ///
    /// # Errors
    ///
    /// Returns serialization or file system failures.
    fn persist_entries(&self, entries: &ChaserSessionCacheFile) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let payload = serde_json::to_vec_pretty(entries).map_err(|err| err.to_string())?;
        fs::write(&self.path, payload).map_err(|err| err.to_string())
    }
}

impl CachedChaserSession {
    /// Builds a cached session from a chaser-cf WAF session.
    ///
    /// # Parameters
    ///
    /// - `session`: chaser-cf WAF session.
    ///
    /// # Returns
    ///
    /// A cached session when `cf_clearance` exists.
    fn from_waf_session(session: &WafSession) -> Option<Self> {
        if !session
            .cookies
            .iter()
            .any(|cookie| cookie.name == "cf_clearance")
        {
            return None;
        }
        let user_agent = session
            .headers
            .get("user-agent")
            .filter(|value| !value.trim().is_empty())
            .cloned();
        Some(Self {
            set_cookie_headers: session.cookies.iter().map(set_cookie_header).collect(),
            user_agent,
            clearance_expires_at: clearance_expires_at(&session.cookies),
            stored_at: unix_timestamp(),
        })
    }

    /// Checks whether the cached session can still be used.
    ///
    /// # Parameters
    ///
    /// - `refresh_margin`: Proactive refresh margin before expiry.
    ///
    /// # Returns
    ///
    /// `true` when the session is not expired or close to expiry.
    fn is_usable(&self, refresh_margin: Duration) -> bool {
        self.clearance_expires_at
            .map(|expires_at| {
                expires_at > unix_timestamp().saturating_add(refresh_margin.as_secs())
            })
            .unwrap_or(true)
    }
}

/// Loads a session cache file.
///
/// # Parameters
///
/// - `path`: JSON cache file path.
///
/// # Returns
///
/// Cached sessions loaded from disk, or an empty cache when the file does not
/// exist.
///
/// # Errors
///
/// Returns file read or JSON parse failures.
fn load_session_cache(path: &Path) -> Result<ChaserSessionCacheFile, String> {
    match fs::read_to_string(path) {
        Ok(payload) => serde_json::from_str(&payload).map_err(|err| err.to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok(ChaserSessionCacheFile::default())
        }
        Err(err) => Err(err.to_string()),
    }
}

/// Builds response headers from a cached session.
///
/// # Parameters
///
/// - `session`: Cached chaser-cf session.
///
/// # Returns
///
/// Headers carrying `Set-Cookie` values and solver user-agent metadata.
///
/// # Errors
///
/// Returns `InvalidHeader` when cached values cannot be represented as HTTP
/// headers.
fn cached_session_headers(session: &CachedChaserSession) -> Result<HeaderMap, ArachneaHttpError> {
    let mut headers = HeaderMap::new();
    for value in &session.set_cookie_headers {
        headers.append(
            SET_COOKIE,
            HeaderValue::from_str(value)
                .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
        );
    }
    if let Some(user_agent) = &session.user_agent {
        headers.insert(
            SOLVER_USER_AGENT_HEADER,
            HeaderValue::from_str(user_agent)
                .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
        );
    }
    Ok(headers)
}

/// Returns one request header value from a CDP JSON header map.
fn lookup_json_header_value(headers: &serde_json::Value, name: &str) -> Option<String> {
    let object = headers.as_object()?;
    object.iter().find_map(|(key, value)| {
        if !key.eq_ignore_ascii_case(name) {
            return None;
        }
        value.as_str().map(str::to_owned)
    })
}

/// Returns whether the current browser page has a Cloudflare clearance cookie.
async fn has_clearance_cookie(page: &chaser_oxide::Page) -> bool {
    page.get_cookies()
        .await
        .map(|cookies| cookies.iter().any(|cookie| cookie.name == "cf_clearance"))
        .unwrap_or(false)
}

/// Attempts to click the visible Cloudflare Turnstile challenge node.
async fn try_click_challenge(chaser: &ChaserPage) {
    let page = chaser.raw_page();
    let doc = match page
        .execute(GetDocumentParams {
            depth: Some(-1),
            pierce: Some(true),
        })
        .await
    {
        Ok(doc) => doc,
        Err(_) => return,
    };
    let Some(node_id) = find_shadow_challenge_node(&doc.result.root) else {
        return;
    };
    let box_model = match page
        .execute(GetBoxModelParams {
            node_id: Some(node_id),
            backend_node_id: None,
            object_id: None,
        })
        .await
    {
        Ok(box_model) => box_model,
        Err(_) => return,
    };
    let content = box_model.result.model.content.inner();
    if content.len() < 8 {
        return;
    }
    let cx = (content[0] + content[2]) / 2.0;
    let cy = (content[1] + content[5]) / 2.0;
    let (tx, ty, curve_points, post_pause_ms) = {
        use rand::Rng as _;

        let mut rng = rand::rng();
        let tx = cx + rng.random_range(-5.0..=5.0_f64);
        let ty = cy + rng.random_range(-4.0..=4.0_f64);
        let p0x = tx + rng.random_range(-200.0..=-60.0_f64);
        let p0y = ty + rng.random_range(-120.0..=120.0_f64);
        let p1x = p0x + (tx - p0x) * rng.random_range(0.2..0.5_f64) + rng.random_range(-30.0..30.0);
        let p1y = p0y + (ty - p0y) * rng.random_range(0.1..0.4_f64) + rng.random_range(-40.0..40.0);
        let p2x = p0x + (tx - p0x) * rng.random_range(0.5..0.8_f64) + rng.random_range(-20.0..20.0);
        let p2y = p0y + (ty - p0y) * rng.random_range(0.5..0.9_f64) + rng.random_range(-20.0..20.0);
        let steps: u8 = rng.random_range(12..22);
        let mut curve_points = Vec::with_capacity(steps as usize);
        for i in 1..=steps {
            let t = f64::from(i) / f64::from(steps);
            let inv = 1.0 - t;
            let bx = inv.powi(3) * p0x
                + 3.0 * inv.powi(2) * t * p1x
                + 3.0 * inv * t.powi(2) * p2x
                + t.powi(3) * tx;
            let by = inv.powi(3) * p0y
                + 3.0 * inv.powi(2) * t * p1y
                + 3.0 * inv * t.powi(2) * p2y
                + t.powi(3) * ty;
            let step_ms = rng.random_range(12..45_u64) + u64::from(i % 4) * 7;
            curve_points.push((bx, by, step_ms.min(80)));
        }
        let post_pause_ms = rng.random_range(40..120_u64);
        (tx, ty, curve_points, post_pause_ms)
    };

    for (bx, by, step_ms) in curve_points {
        let _ = page
            .move_mouse(chaser_oxide::layout::Point::new(bx, by))
            .await;
        tokio::time::sleep(Duration::from_millis(step_ms)).await;
    }
    tokio::time::sleep(Duration::from_millis(post_pause_ms)).await;
    let _ = page.click(chaser_oxide::layout::Point::new(tx, ty)).await;
}

/// Finds the visible Turnstile node inside Cloudflare's pierced shadow DOM.
fn find_shadow_challenge_node(node: &Node) -> Option<NodeId> {
    let children = node.children.as_deref().unwrap_or(&[]);
    let is_shadow_host = children.iter().any(|child| {
        child
            .attributes
            .as_deref()
            .unwrap_or(&[])
            .chunks(2)
            .any(|pair| pair.len() == 2 && pair[0] == "name" && pair[1] == "cf-turnstile-response")
    });

    if is_shadow_host {
        if let Some(shadow_root) = node.shadow_roots.as_ref().and_then(|roots| roots.first()) {
            for child in shadow_root.children.as_deref().unwrap_or(&[]) {
                let hidden = child
                    .attributes
                    .as_deref()
                    .unwrap_or(&[])
                    .chunks(2)
                    .any(|pair| {
                        pair.len() == 2 && pair[0] == "style" && pair[1].contains("display: none")
                    });
                if !hidden {
                    return Some(child.node_id);
                }
            }
        }
    }

    for child in children {
        if let Some(node_id) = find_shadow_challenge_node(child) {
            return Some(node_id);
        }
    }
    for shadow_root in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for child in shadow_root.children.as_deref().unwrap_or(&[]) {
            if let Some(node_id) = find_shadow_challenge_node(child) {
                return Some(node_id);
            }
        }
    }
    None
}

/// Builds a compact signature for page-source stability checks.
///
/// # Parameters
///
/// - `source`: HTML source returned by chaser-cf.
///
/// # Returns
///
/// The source length and a process-local hash.
fn page_source_signature(source: &str) -> (usize, u64) {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    (source.len(), hasher.finish())
}

/// Checks whether an HTML body still looks like a Cloudflare challenge page.
///
/// # Parameters
///
/// - `source`: HTML source returned by chaser-cf.
///
/// # Returns
///
/// `true` when the source carries common Cloudflare interstitial markers.
fn looks_like_cloudflare_interstitial(source: &str) -> bool {
    let source = source.to_ascii_lowercase();
    [
        "cf-challenge",
        "cf-chl-",
        "cf-please-wait",
        "cf-turnstile-response",
        "challenge-platform",
        "challenges.cloudflare.com",
        "checking if the site connection is secure",
        "verify you are human",
        "just a moment",
    ]
    .iter()
    .any(|marker| source.contains(marker))
}

/// Synthesizes a `Set-Cookie` header from a chaser-cf cookie.
///
/// # Parameters
///
/// - `cookie`: Browser cookie reported by chaser-cf.
///
/// # Returns
///
/// A `Set-Cookie` header value that can be parsed by the shared cookie cache.
fn set_cookie_header(cookie: &ChaserCookie) -> String {
    let mut value = format!("{}={}", cookie.name, cookie.value);
    if let Some(domain) = &cookie.domain {
        value.push_str("; Domain=");
        value.push_str(domain);
    }
    if let Some(path) = &cookie.path {
        value.push_str("; Path=");
        value.push_str(path);
    }
    if let Some(expires) = expires_http_date(cookie.expires) {
        value.push_str("; Expires=");
        value.push_str(&expires);
    }
    if cookie.secure.unwrap_or(false) {
        value.push_str("; Secure");
    }
    if cookie.http_only.unwrap_or(false) {
        value.push_str("; HttpOnly");
    }
    if let Some(same_site) = &cookie.same_site {
        value.push_str("; SameSite=");
        value.push_str(same_site);
    }
    value
}

/// Extracts the `cf_clearance` expiration timestamp from browser cookies.
///
/// # Parameters
///
/// - `cookies`: Browser cookies reported by chaser-cf.
///
/// # Returns
///
/// A Unix timestamp when the clearance cookie has a finite expiration.
fn clearance_expires_at(cookies: &[ChaserCookie]) -> Option<u64> {
    cookies
        .iter()
        .find(|cookie| cookie.name == "cf_clearance")
        .and_then(|cookie| cookie.expires)
        .filter(|expires| expires.is_finite() && *expires >= 0.0)
        .map(|expires| expires as u64)
}

/// Converts a browser cookie expiry timestamp into an HTTP date.
///
/// # Parameters
///
/// - `expires`: Optional Unix timestamp in seconds, as reported by chaser-cf.
///
/// # Returns
///
/// A formatted HTTP date when the timestamp is finite and non-negative.
fn expires_http_date(expires: Option<f64>) -> Option<String> {
    let expires = expires?;
    if !expires.is_finite() || expires < 0.0 {
        return None;
    }
    let duration = Duration::from_secs_f64(expires);
    Some(httpdate::fmt_http_date(UNIX_EPOCH + duration))
}

/// Builds the default session cache path.
///
/// # Returns
///
/// A platform temporary path used to store chaser-cf sessions.
fn default_session_cache_path() -> PathBuf {
    std::env::temp_dir()
        .join("arachnea-http")
        .join(DEFAULT_SESSION_CACHE_FILE_NAME)
}

/// Builds a normalized origin key for a URL.
///
/// # Parameters
///
/// - `value`: Absolute URL to normalize.
///
/// # Returns
///
/// A normalized origin string.
///
/// # Errors
///
/// Returns `InvalidUrl` when the URL cannot be parsed or has no host.
fn cache_origin_key(value: &str) -> Result<String, ArachneaHttpError> {
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

/// Returns the current Unix timestamp.
///
/// # Returns
///
/// Seconds since the Unix epoch.
fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
/// Unit tests for chaser-cf session caching.
mod tests {
    use super::*;

    /// Builds a unique cache file path for a test.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable test name included in the file name.
    ///
    /// # Returns
    ///
    /// A unique path inside the platform temporary directory.
    fn cache_path(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "arachnea-http-{name}-{}-{stamp}.json",
            std::process::id()
        ))
    }

    /// Builds a WAF session containing a `cf_clearance` cookie.
    ///
    /// # Parameters
    ///
    /// - `expires_at`: Unix expiration timestamp for the clearance cookie.
    ///
    /// # Returns
    ///
    /// A synthetic chaser-cf WAF session.
    fn waf_session(expires_at: u64) -> WafSession {
        let mut headers = HashMap::new();
        headers.insert("user-agent".to_string(), "test-browser".to_string());
        WafSession::new(
            vec![ChaserCookie {
                name: "cf_clearance".to_string(),
                value: "ok".to_string(),
                domain: Some("example.com".to_string()),
                path: Some("/".to_string()),
                expires: Some(expires_at as f64),
                http_only: Some(true),
                secure: Some(true),
                same_site: Some("None".to_string()),
            }],
            headers,
        )
    }

    /// Verifies that a valid cached clearance returns HTTP headers.
    #[test]
    fn persistent_session_cache_returns_headers_for_valid_clearance() {
        let cache = ChaserSessionCache::new(cache_path("valid"), Duration::from_secs(300));
        cache
            .store_session(
                "https://example.com/search",
                &waf_session(unix_timestamp() + 3_600),
            )
            .expect("session stores");

        let headers = cache
            .headers_for_url("https://example.com/other")
            .expect("cache lookup succeeds")
            .expect("session exists");
        let cookies = headers
            .get_all(SET_COOKIE)
            .iter()
            .map(|value| value.to_str().expect("valid cookie header"))
            .collect::<Vec<_>>();

        assert!(cookies
            .iter()
            .any(|value| value.starts_with("cf_clearance=ok")));
        assert_eq!(
            headers
                .get(SOLVER_USER_AGENT_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some("test-browser")
        );
    }

    /// Verifies that a clearance inside the refresh margin is ignored.
    #[test]
    fn persistent_session_cache_ignores_expiring_clearance() {
        let cache = ChaserSessionCache::new(cache_path("expiring"), Duration::from_secs(300));
        cache
            .store_session(
                "https://example.com/search",
                &waf_session(unix_timestamp() + 30),
            )
            .expect("session stores");

        let headers = cache
            .headers_for_url("https://example.com/other")
            .expect("cache lookup succeeds");

        assert!(headers.is_none());
    }
}
