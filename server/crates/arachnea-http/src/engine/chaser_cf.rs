use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use bytes::Bytes;
use chaser_cf::core::BrowserManager;
use chaser_cf::{
    ChaserCF, ChaserConfig, Cookie as ChaserCookie, Profile, ProxyConfig, WafSession,
};
use chaser_oxide::auth::Credentials;
use chaser_oxide::{ChaserPage, Page};
use http::{
    header::{CONTENT_TYPE, HeaderName, SET_COOKIE},
    HeaderMap, HeaderValue, Method, StatusCode,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, warn};
use url::Url;

use crate::{
    browser::{
        BrowserPageSession, BrowserSessionMetadata, PageClickRequest, PageClickResponse,
        PageFetchRequest, PageFetchResponse, PageNavigationRequest, PageNavigationResponse,
    },
    config::ArachneaHttpConfig,
    engine::{EngineRequest, EngineResponse, HttpEngine, SOLVER_USER_AGENT_HEADER},
    error::ArachneaHttpError,
};

/// Registry name for the chaser-cf session solver.
pub const ENGINE_NAME: &str = "chaser-cf";

/// Default margin before a cached Cloudflare session is considered stale.
const DEFAULT_SESSION_CACHE_REFRESH_MARGIN: Duration = Duration::from_secs(300);

/// File name used by the default persistent chaser-cf session cache.
const DEFAULT_SESSION_CACHE_FILE_NAME: &str = "chaser-cf-sessions.json";

/// Dedicated timeout for chaser-cf browser challenge solving, kept independent
/// from the HTTP request timeout because solving a captcha (including browser
/// startup with lazy init) can take far longer than a regular request.
const CHASER_SOLVE_TIMEOUT: Duration = Duration::from_secs(180);

/// How long a retained interactive page waits for the target selector after a
/// programmatic click before giving up.
const CLICK_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long `read_turnstile_token` polls while the operator resolves a
/// Cloudflare captcha manually before returning `None`.
const TURNSTILE_WAIT_TIMEOUT: Duration = Duration::from_secs(300);

/// Upper bound for a cached session whose expiration could not be determined,
/// matching the typical Cloudflare `cf_clearance` lifetime.
const CACHE_TTL_NO_EXPIRY: Duration = Duration::from_secs(1_800);

/// chaser-cf adapter that resolves Cloudflare sessions and, for direct
/// requests, returns the cleared page content.
///
/// All challenge interaction is delegated to chaser-cf's public API:
/// `ChaserCF::solve_waf_session` produces the clearance cookies and the
/// browser user-agent, and `ChaserCF::get_source` returns the page HTML.
/// Refresh flows still hand the cookies and user-agent to
/// `ArachneaHttpClient` for the rquest transport.
pub struct ChaserCfEngine {
    /// Native chaser-cf configuration used to lazily build the public facade.
    config: ChaserConfig,
    /// Lazily initialized public chaser-cf facade.
    chaser: Arc<RwLock<Option<Arc<ChaserCF>>>>,
    /// Proxy route shared with the rquest handoff when supported.
    proxy: Option<ProxyConfig>,
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
        Self::new(&config).unwrap()
    }
}

impl ChaserCfEngine {
    /// Creates a configured chaser-cf session solver.
    ///
    /// # Errors
    ///
    /// Returns an error when the configured proxy cannot be shared with
    /// chaser-cf and rquest.
    pub fn new(config: &ArachneaHttpConfig) -> Result<Self, ArachneaHttpError> {
        Self::new_with_proxy_url(config, config.proxy.network_url())
    }

    /// Creates a configured chaser-cf session solver using the effective proxy
    /// URL selected by the HTTP client runtime.
    pub(crate) fn new_with_proxy_url(
        config: &ArachneaHttpConfig,
        proxy_url: Option<&str>,
    ) -> Result<Self, ArachneaHttpError> {
        let chaser_config = ChaserConfig::from_env()
            .with_headless(false)
            .with_timeout(CHASER_SOLVE_TIMEOUT)
            .with_lazy_init(true)
            .with_profile(Profile::Windows)
            .with_extra_args([
                 "--disable-blink-features=AutomationControlled",
                 "--disable-infobars",
                 "--no-first-run",
                 "--no-default-browser-check",
             ]);

        let engine = Self {
            config: chaser_config,
            chaser: Arc::new(RwLock::new(None)),
            proxy: chaser_proxy_from_url(proxy_url)?,
            session_cache: None,
        };
        if uses_dynamic_arachnea_proxy(config) {
            return Ok(engine);
        }
        Ok(engine
            .with_default_session_cache()
            .with_session_cache_refresh_margin(config.cookie_refresh_margin))
    }

    /// Creates an engine from native chaser-cf configuration.
    ///
    /// # Errors
    ///
    /// Returns `ChaserCfFailure` when chaser-cf cannot be constructed.
    pub async fn from_chaser_config(config: ChaserConfig) -> Result<Self, ArachneaHttpError> {
        let chaser = ChaserCF::new(config.clone())
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        Ok(Self {
            config,
            chaser: Arc::new(RwLock::new(Some(Arc::new(chaser)))),
            proxy: None,
            session_cache: None,
        })
    }

    /// Enables a persistent session cache at the default cache path.
    pub fn with_default_session_cache(self) -> Self {
        self.with_session_cache_path(default_session_cache_path())
    }

    /// Enables a persistent session cache at `path`.
    pub fn with_session_cache_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.session_cache = Some(ChaserSessionCache::new(
            path.into(),
            DEFAULT_SESSION_CACHE_REFRESH_MARGIN,
        ));
        self
    }

    /// Updates the refresh margin used by the persistent session cache.
    pub fn with_session_cache_refresh_margin(mut self, margin: Duration) -> Self {
        if let Some(cache) = &mut self.session_cache {
            cache.refresh_margin = margin;
        }
        self
    }

    /// Disables the persistent session cache.
    pub fn without_session_cache(mut self) -> Self {
        self.session_cache = None;
        self
    }

    /// Returns the public chaser-cf facade, creating it lazily when needed.
    async fn chaser(&self) -> Result<Arc<ChaserCF>, ArachneaHttpError> {
        if let Some(chaser) = self.chaser.read().await.as_ref() {
            return Ok(chaser.clone());
        }

        let mut slot = self.chaser.write().await;
        if let Some(chaser) = slot.as_ref() {
            return Ok(chaser.clone());
        }
        let chaser = Arc::new(
            ChaserCF::new(self.config.clone())
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?,
        );
        *slot = Some(chaser.clone());
        Ok(chaser)
    }

    /// Resolves a fresh Cloudflare session through chaser-cf's public API.
    async fn solve_waf_session(&self, url: &str) -> Result<WafSession, ArachneaHttpError> {
        let session = self
            .chaser()
            .await?
            .solve_waf_session(url, self.proxy.clone())
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
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
        debug!(
            origin = %url,
            cookie_names = ?session.cookies.iter().map(|cookie| cookie.name.as_str()).collect::<Vec<_>>(),
            has_solver_user_agent = session.headers.get("user-agent").is_some_and(|value| !value.trim().is_empty()),
            "chaser-cf resolved Cloudflare session"
        );
        Ok(session)
    }

    /// Builds response headers containing cookies extracted by chaser-cf.
    fn response_headers(cookies: &[ChaserCookie]) -> Result<HeaderMap, ArachneaHttpError> {
        if !cookies.iter().any(|cookie| cookie.name == "cf_clearance") {
            return Err(ArachneaHttpError::CookieAbsent {
                origin: "chaser-cf browser session".to_string(),
                name: "cf_clearance".to_string(),
            });
        }
        let mut headers = HeaderMap::new();
        for cookie in cookies {
            headers.append(
                SET_COOKIE,
                HeaderValue::from_str(&set_cookie_header(cookie))
                    .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
            );
        }
        Ok(headers)
    }

    /// Adds the browser user-agent reported by chaser-cf to solver metadata.
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
    fn cached_session_headers(&self, url: &str) -> Option<HeaderMap> {
        let cache = self.session_cache.as_ref()?;
        match cache.headers_for_url(url) {
            Ok(headers) => headers,
            Err(err) => {
                warn!(error = %err, "failed to read chaser-cf session cache");
                None
            }
        }
    }

    /// Stores a newly solved Cloudflare session in the optional cache.
    fn store_session_cache(&self, url: &str, session: &WafSession) {
        let Some(cache) = &self.session_cache else {
            return;
        };
        if let Err(err) = cache.store_session(url, session) {
            warn!(error = %err, "failed to write chaser-cf session cache");
        }
    }

    /// Refreshes Cloudflare cookies, optionally reusing a cached session.
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

        let session = self.solve_waf_session(&request.url).await?;
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

/// Returns whether a session cache would be unsafe because the effective egress
/// can vary independently from the origin.
#[cfg(feature = "arachnea-proxy")]
fn uses_dynamic_arachnea_proxy(config: &ArachneaHttpConfig) -> bool {
    matches!(&config.proxy, crate::config::HttpProxyConfig::Arachnea(_))
}

/// Returns false when the dynamic Arachnea proxy integration is unavailable.
#[cfg(not(feature = "arachnea-proxy"))]
fn uses_dynamic_arachnea_proxy(_config: &ArachneaHttpConfig) -> bool {
    false
}

#[async_trait]
impl HttpEngine for ChaserCfEngine {
    fn name(&self) -> &'static str {
        ENGINE_NAME
    }

    /// Executes a GET/HEAD request through chaser-cf's public solver.
    ///
    /// Challenge handling is fully delegated to chaser-cf: `solve_waf_session`
    /// produces the clearance cookies and browser user-agent, and `get_source`
    /// returns the page HTML. This engine only assembles the response headers
    /// and body, so no chaser-cf challenge logic is reimplemented here.
    ///
    /// # Context note
    ///
    /// Each chaser-cf public call runs in its own browser context. Without a
    /// proxy, both calls share the default context and reuse the solved
    /// `cf_clearance`. With a proxy, chaser-cf uses a fresh incognito context
    /// per call, so the HTML fetch can require its own challenge solve.
    async fn send(&self, request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError> {
        if request.method != Method::GET && request.method != Method::HEAD {
            return Err(ArachneaHttpError::UnsupportedEngineOperation {
                engine: self.name(),
                operation: "non-GET/HEAD HTTP requests",
            });
        }

        let session = self.solve_waf_session(&request.url).await?;
        let mut headers = Self::response_headers(&session.cookies)?;
        Self::insert_solver_user_agent(&mut headers, &session)?;
        self.store_session_cache(&request.url, &session);

        let body = if request.method == Method::GET {
            let html = self
                .chaser()
                .await?
                .get_source(&request.url, self.proxy.clone())
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
            headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            );
            Bytes::from(html)
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

    async fn refresh_cloudflare(
        &self,
        request: EngineRequest,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        self.refresh_cloudflare_with_cache_policy(request, true)
            .await
    }

    async fn refresh_cloudflare_fresh(
        &self,
        request: EngineRequest,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        self.refresh_cloudflare_with_cache_policy(request, false)
            .await
    }

    /// Opens an interactive page session on a dedicated, single Chrome window.
    ///
    /// A `BrowserManager` is created directly from the shared `ChaserConfig` —
    /// the crate publicly re-exports it — and one page is retained for the whole
    /// session. Nothing is auto-solved here: the operator loads the page, the
    /// session clicks the button that injects the Cloudflare captcha, the
    /// operator solves it manually in the same window, and `read_turnstile_token`
    /// then polls for the resulting token. No second window is ever launched
    /// while this session is the only chaser-cf activity.
    async fn open_browser_page_session(
        &self,
    ) -> Result<Box<dyn BrowserPageSession>, ArachneaHttpError> {
        let manager = BrowserManager::new(&self.config)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let ctx_id = match manager.create_context(self.proxy.as_ref()).await {
            Ok(ctx_id) => ctx_id,
            Err(err) => {
                manager.shutdown().await;
                return Err(ArachneaHttpError::ChaserCfFailure(err.to_string()));
            }
        };
        let (page, chaser) = match manager.new_page(ctx_id, "about:blank").await {
            Ok(pair) => pair,
            Err(err) => {
                manager.shutdown().await;
                return Err(ArachneaHttpError::ChaserCfFailure(err.to_string()));
            }
        };
        if let Some(proxy) = &self.proxy {
            if let (Some(username), Some(password)) = (&proxy.username, &proxy.password) {
                if let Err(err) = page
                    .authenticate(Credentials {
                        username: username.clone(),
                        password: password.clone(),
                    })
                    .await
                {
                    manager.shutdown().await;
                    return Err(ArachneaHttpError::ChaserCfFailure(err.to_string()));
                }
            }
        }
        Ok(Box::new(ChaserCfPageSession {
            browser: manager,
            page,
            chaser,
        }))
    }
}

/// Interactive chaser-cf page session on a single dedicated Chrome window.
///
/// The session owns its own `BrowserManager` (built from the shared
/// `ChaserConfig`), so it does not rely on the facade's internal window and no
/// second window appears while this session is the only chaser-cf activity. The
/// page is retained for the session lifetime, which is what makes the manual
/// Cloudflare flow possible in one window:
///
/// 1. `navigate` loads the target page.
/// 2. `click_and_wait` clicks the button that dynamically injects the
///    Cloudflare captcha.
/// 3. The operator solves the captcha by hand in the visible window while
///    `read_turnstile_token` polls for the token.
/// 4. Once resolved, `metadata` exposes the resulting cookies and user-agent
///    and the captcha-driven iframe can be read through `fetch`.
struct ChaserCfPageSession {
    /// Browser manager owning the single interactive Chrome window.
    browser: BrowserManager,
    /// Retained CDP page handle.
    page: Page,
    /// Retained JavaScript evaluation helper over `page`.
    chaser: ChaserPage,
}

#[async_trait]
impl BrowserPageSession for ChaserCfPageSession {
    async fn navigate(
        &mut self,
        request: PageNavigationRequest,
    ) -> Result<PageNavigationResponse, ArachneaHttpError> {
        self.chaser
            .goto(&request.url)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let url = self
            .page
            .url()
            .await
            .ok()
            .flatten()
            .unwrap_or(request.url);
        let body = if request.collect_body {
            let html = self
                .chaser
                .content()
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
            Some(html)
        } else {
            None
        };
        Ok(PageNavigationResponse { url, body })
    }

    async fn fetch(
        &mut self,
        request: PageFetchRequest,
    ) -> Result<PageFetchResponse, ArachneaHttpError> {
        let mut headers = HashMap::new();
        for (name, value) in &request.headers {
            let value = value
                .to_str()
                .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?;
            headers.insert(name.as_str().to_string(), value.to_string());
        }
        let body = request
            .body
            .map(|body| {
                String::from_utf8(body.to_vec()).map_err(|_| {
                    ArachneaHttpError::PageFetchFailed(
                        "in-page fetch currently requires a UTF-8 request body".to_string(),
                    )
                })
            })
            .transpose()?;
        let payload = serde_json::json!({
            "method": request.method.as_str(),
            "url": request.url,
            "headers": headers,
            "body": body,
        });
        let script = format!(
            "async function() {{ const input = {}; const response = await fetch(input.url, {{ method: input.method, headers: input.headers, body: input.body, credentials: 'same-origin' }}); const headers = {{}}; response.headers.forEach((value, name) => {{ headers[name] = value; }}); return {{ url: response.url, status: response.status, headers, body: await response.text() }}; }}",
            serde_json::to_string(&payload)
                .map_err(|err| ArachneaHttpError::PageFetchFailed(err.to_string()))?
        );
        let result = self
            .page
            .evaluate_function(script)
            .await
            .map_err(|err| ArachneaHttpError::PageFetchFailed(err.to_string()))?
            .into_value::<InPageFetchResult>()
            .map_err(|err| ArachneaHttpError::PageFetchFailed(err.to_string()))?;
        let mut response_headers = HeaderMap::new();
        for (name, value) in result.headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|err| ArachneaHttpError::PageFetchFailed(err.to_string()))?;
            let value = HeaderValue::from_str(&value)
                .map_err(|err| ArachneaHttpError::PageFetchFailed(err.to_string()))?;
            response_headers.append(name, value);
        }
        Ok(PageFetchResponse {
            url: result.url,
            status: StatusCode::from_u16(result.status).map_err(|err| {
                ArachneaHttpError::PageFetchFailed(format!("invalid browser fetch status: {err}"))
            })?,
            headers: response_headers,
            body: result.body,
        })
    }

    async fn click_and_wait(
        &mut self,
        request: PageClickRequest,
    ) -> Result<PageClickResponse, ArachneaHttpError> {
        let selector = serde_json::to_string(&request.selector)
            .map_err(|err| ArachneaHttpError::PageInteractionFailed(err.to_string()))?;
        let click_script = format!(
            "(() => {{ const element = document.querySelector({selector}); if (!element) throw new Error('click target was not found'); element.click(); return true; }})()"
        );
        self.chaser
            .evaluate(&click_script)
            .await
            .map_err(|err| ArachneaHttpError::PageInteractionFailed(err.to_string()))?;

        let wait_script = format!(
            "(() => document.querySelector({selector}) !== null)()",
            selector = serde_json::to_string(&request.wait_for_selector)
                .map_err(|err| ArachneaHttpError::PageInteractionFailed(err.to_string()))?
        );
        let started = Instant::now();
        loop {
            let found = self
                .chaser
                .evaluate(&wait_script)
                .await
                .map_err(|err| ArachneaHttpError::PageInteractionFailed(err.to_string()))?
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            if found {
                let body = self
                    .chaser
                    .content()
                    .await
                    .map_err(|err| ArachneaHttpError::PageInteractionFailed(err.to_string()))?;
                return Ok(PageClickResponse { body });
            }
            if started.elapsed() >= CLICK_WAIT_TIMEOUT {
                return Err(ArachneaHttpError::PageInteractionFailed(
                    "timed out waiting for the page action result".to_string(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    async fn metadata(&mut self) -> Result<BrowserSessionMetadata, ArachneaHttpError> {
        let cookies = self
            .page
            .get_cookies()
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let mut headers = HeaderMap::new();
        for cookie in cookies {
            let chaser_cookie = ChaserCookie {
                name: cookie.name,
                value: cookie.value,
                domain: Some(cookie.domain),
                path: Some(cookie.path),
                expires: Some(cookie.expires),
                http_only: Some(cookie.http_only),
                secure: Some(cookie.secure),
                same_site: cookie.same_site.map(|value| format!("{value:?}")),
            };
            headers.append(
                SET_COOKIE,
                HeaderValue::from_str(&set_cookie_header(&chaser_cookie))
                    .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
            );
        }
        if let Some(user_agent) = self
            .chaser
            .evaluate("navigator.userAgent")
            .await
            .ok()
            .and_then(|value| value?.as_str().map(str::to_owned))
            .filter(|value| !value.is_empty())
        {
            headers.insert(
                SOLVER_USER_AGENT_HEADER,
                HeaderValue::from_str(&user_agent)
                    .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
            );
        }
        let url = self
            .page
            .url()
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?
            .unwrap_or_default();
        Ok(BrowserSessionMetadata { url, headers })
    }

    async fn read_turnstile_token(&mut self) -> Result<Option<String>, ArachneaHttpError> {
        let script = r#"(function() {
            if (window.turnstile && typeof window.turnstile.getResponse === 'function') {
                var t = window.turnstile.getResponse();
                if (t && t.length > 10) return t;
            }
            var el = document.querySelector('[name="cf-response"]');
            if (el && el.value && el.value.length > 10) return el.value;
            return null;
        })()"#;
        let started = Instant::now();
        loop {
            let token = self
                .chaser
                .evaluate(script)
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?
                .and_then(|value| value.as_str().map(str::to_owned));
            if let Some(token) = token {
                return Ok(Some(token));
            }
            if started.elapsed() >= TURNSTILE_WAIT_TIMEOUT {
                return Ok(None);
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    async fn clear_turnstile_token(&mut self) -> Result<(), ArachneaHttpError> {
        let script = r#"(function() {
            if (window.turnstile && typeof window.turnstile.reset === 'function') {
                try { window.turnstile.reset(); } catch (e) {}
            }
            var el = document.querySelector('[name="cf-response"]');
            if (el) { el.value = ''; }
            return true;
        })()"#;
        self.chaser
            .evaluate(script)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        Ok(())
    }

    async fn close(self: Box<Self>) {
        let ChaserCfPageSession { browser, .. } = *self;
        browser.shutdown().await;
    }
}

/// Serializable shape of the object returned by the in-page `fetch` helper.
#[derive(Deserialize)]
struct InPageFetchResult {
    url: String,
    status: u16,
    headers: HashMap<String, String>,
    body: String,
}

impl ChaserSessionCache {
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

    fn persist_entries(&self, entries: &ChaserSessionCacheFile) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let payload = serde_json::to_vec_pretty(entries).map_err(|err| err.to_string())?;
        fs::write(&self.path, payload).map_err(|err| err.to_string())
    }
}

impl CachedChaserSession {
    fn from_waf_session(session: &WafSession) -> Option<Self> {
        if !session
            .cookies
            .iter()
            .any(|cookie| cookie.name == "cf_clearance")
        {
            return None;
        }
        Some(Self {
            set_cookie_headers: session.cookies.iter().map(set_cookie_header).collect(),
            user_agent: session
                .headers
                .get("user-agent")
                .filter(|value| !value.trim().is_empty())
                .cloned(),
            clearance_expires_at: clearance_expires_at(&session.cookies),
            stored_at: unix_timestamp(),
        })
    }

    fn is_usable(&self, refresh_margin: Duration) -> bool {
        match self.clearance_expires_at {
            Some(expires_at) => {
                expires_at > unix_timestamp().saturating_add(refresh_margin.as_secs())
            }
            None => self
                .stored_at
                .saturating_add(CACHE_TTL_NO_EXPIRY.as_secs())
                > unix_timestamp(),
        }
    }
}

fn chaser_proxy_from_url(
    proxy_url: Option<&str>,
) -> Result<Option<ProxyConfig>, ArachneaHttpError> {
    let Some(proxy_url) = proxy_url else {
        return Ok(None);
    };
    let url = Url::parse(proxy_url).map_err(|err| {
        ArachneaHttpError::InvalidConfiguration(format!("invalid chaser-cf proxy URL: {err}"))
    })?;
    let host = url.host_str().ok_or_else(|| {
        ArachneaHttpError::InvalidConfiguration(
            "chaser-cf proxy URL must contain a host".to_string(),
        )
    })?;
    let port = url.port_or_known_default().ok_or_else(|| {
        ArachneaHttpError::InvalidConfiguration(
            "chaser-cf proxy URL must contain a port".to_string(),
        )
    })?;
    let mut config = ProxyConfig::new(host, port).with_scheme(url.scheme());
    if !url.username().is_empty() {
        config = config.with_auth(
            url.username(),
            url.password().ok_or_else(|| {
                ArachneaHttpError::InvalidConfiguration(
                    "chaser-cf proxy credentials require a password".to_string(),
                )
            })?,
        );
    }
    Ok(Some(config))
}

fn load_session_cache(path: &Path) -> Result<ChaserSessionCacheFile, String> {
    match fs::read_to_string(path) {
        Ok(payload) => serde_json::from_str(&payload).map_err(|err| err.to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok(ChaserSessionCacheFile::default())
        }
        Err(err) => Err(err.to_string()),
    }
}

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

fn clearance_expires_at(cookies: &[ChaserCookie]) -> Option<u64> {
    cookies
        .iter()
        .find(|cookie| cookie.name == "cf_clearance")
        .and_then(|cookie| cookie.expires)
        .filter(|expires| expires.is_finite() && *expires >= 0.0)
        .map(|expires| expires as u64)
}

fn expires_http_date(expires: Option<f64>) -> Option<String> {
    let expires = expires?;
    if !expires.is_finite() || expires < 0.0 {
        return None;
    }
    let duration = Duration::try_from_secs_f64(expires).ok()?;
    let instant = UNIX_EPOCH.checked_add(duration)?;
    Some(httpdate::fmt_http_date(instant))
}

fn default_session_cache_path() -> PathBuf {
    std::env::temp_dir()
        .join("arachnea-http")
        .join(DEFAULT_SESSION_CACHE_FILE_NAME)
}

fn cache_origin_key(value: &str) -> Result<String, ArachneaHttpError> {
    let url = Url::parse(value).map_err(|err| ArachneaHttpError::InvalidUrl(err.to_string()))?;
    let host = url
        .host_str()
        .ok_or_else(|| ArachneaHttpError::InvalidUrl("URL must contain a host".to_string()))?;
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    Ok(format!("{}://{host}{port}/", url.scheme()))
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(headers.get_all(SET_COOKIE).iter().any(|value| value
            .to_str()
            .is_ok_and(|value| value.starts_with("cf_clearance=ok"))));
        assert_eq!(
            headers
                .get(SOLVER_USER_AGENT_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some("test-browser")
        );
    }

    #[test]
    fn persistent_session_cache_ignores_expiring_clearance() {
        let cache = ChaserSessionCache::new(cache_path("expiring"), Duration::from_secs(300));
        cache
            .store_session(
                "https://example.com/search",
                &waf_session(unix_timestamp() + 30),
            )
            .expect("session stores");

        assert!(cache
            .headers_for_url("https://example.com/other")
            .expect("cache lookup succeeds")
            .is_none());
    }
}
