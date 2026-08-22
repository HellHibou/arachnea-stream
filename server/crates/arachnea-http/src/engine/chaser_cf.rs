use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use arachnea_core::persistence::{PersistedRecord, PersistenceStore};
use async_trait::async_trait;
use bytes::Bytes;
use chaser_cf::core::BrowserManager;
use chaser_cf::{ChaserConfig, Cookie as ChaserCookie, Profile, ProxyConfig, WafSession};
use chaser_oxide::auth::Credentials;
use chaser_oxide::cdp::browser_protocol::dom::{
    GetBoxModelParams, GetDocumentParams, Node, NodeId,
};
use chaser_oxide::cdp::js_protocol::runtime::EvaluateParams;
use chaser_oxide::layout::Point;
use chaser_oxide::{ChaserPage, Page};
use http::{
    header::{HeaderName, CONTENT_TYPE, SET_COOKIE},
    HeaderMap, HeaderValue, Method, StatusCode,
};
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
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

/// Namespace used for persisted Cloudflare sessions.
const CLOUDFLARE_SESSION_NAMESPACE: &str = "cloudflare-session";

/// Format version of the persisted Cloudflare session payload.
const CLOUDFLARE_SESSION_FORMAT_VERSION: u32 = 1;

/// Dedicated timeout for chaser-cf browser challenge solving, kept independent
/// from the HTTP request timeout because solving a captcha (including browser
/// startup with lazy init) can take far longer than a regular request.
const CHASER_SOLVE_TIMEOUT: Duration = Duration::from_secs(180);

/// How long a retained interactive page waits for the target selector after a
/// programmatic click before giving up.
const CLICK_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the automatically solved Cloudflare captcha may take after a
/// page action click before `click_and_wait` gives up.
const CLICK_SOLVE_TIMEOUT: Duration = Duration::from_secs(90);

/// How long `read_turnstile_token` polls for the token produced by the
/// automatically solved Cloudflare captcha before returning `None`.
const TURNSTILE_WAIT_TIMEOUT: Duration = Duration::from_secs(300);

/// Upper bound for a cached session whose expiration could not be determined,
/// matching the typical Cloudflare `cf_clearance` lifetime.
const CACHE_TTL_NO_EXPIRY: Duration = Duration::from_secs(1_800);

/// Passive wait before Cloudflare's invisible challenge JS starts its PoW.
const CLEARANCE_PASSIVE_WAIT_MS: u64 = 2_000;

/// Minimum interval between automated Turnstile challenge clicks.
const CLEARANCE_CLICK_INTERVAL_MS: u64 = 1_000;

/// Process-wide shared chaser-cf browser manager.
///
/// A single Chrome instance is shared by every `ChaserCfEngine` and page
/// session. This prevents two Chrome processes from racing on the same default
/// `user-data-dir`, which previously made a Cloudflare session solve following
/// a browser page session (or vice versa) fail with
/// `Browser process exited with status ExitStatus(ExitStatus(0))`.
static GLOBAL_BROWSER: OnceLock<Arc<RwLock<Option<Arc<BrowserManager>>>>> = OnceLock::new();

/// Returns the process-wide shared chaser-cf browser manager slot.
fn global_browser_slot() -> &'static Arc<RwLock<Option<Arc<BrowserManager>>>> {
    GLOBAL_BROWSER.get_or_init(|| Arc::new(RwLock::new(None)))
}

/// chaser-cf adapter that resolves Cloudflare sessions and, for direct
/// requests, returns the cleared page content.
///
/// All challenge interaction is implemented directly on the process-wide shared
/// `BrowserManager`: clearing the `cf_clearance` cookie and clicking the
/// Turnstile challenge through the CDP shadow root. The same browser instance
/// is reused by persistent page sessions, so only one Chrome profile lock is
/// ever held.
pub struct ChaserCfEngine {
    /// Native chaser-cf configuration used to lazily build the shared browser.
    config: ChaserConfig,
    /// Proxy route shared with the rquest handoff when supported.
    proxy: Option<ProxyConfig>,
    /// Optional persistent session cache used before opening Chrome again.
    session_cache: Option<ChaserSessionCache>,
}

/// Serialized Cloudflare session extracted from the shared browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedChaserSession {
    /// Structured cookies produced from browser cookies.
    cookies: Vec<StructuredCookie>,
    /// Browser user-agent observed by the solver.
    user_agent: Option<String>,
    /// Unix timestamp for `cf_clearance` expiration when available.
    clearance_expires_at: Option<u64>,
    /// Unix timestamp when the session was stored.
    stored_at: u64,
}

/// Structured cookie representation persisted in the session record.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StructuredCookie {
    /// Cookie name.
    name: String,
    /// Cookie value.
    value: String,
    /// Cookie domain.
    domain: Option<String>,
    /// Cookie path.
    path: Option<String>,
    /// Cookie expiration as Unix timestamp.
    expires: Option<f64>,
    /// Whether the cookie is HTTP-only.
    http_only: Option<bool>,
    /// Whether the cookie is secure.
    secure: Option<bool>,
    /// SameSite attribute.
    same_site: Option<String>,
}

/// Asynchronous cache for chaser-cf Cloudflare sessions backed by a
/// [`PersistenceStore`].
struct ChaserSessionCache {
    /// Shared persistence store.
    store: Arc<dyn PersistenceStore>,
    /// Proactive refresh margin before `cf_clearance` expiry.
    refresh_margin: Duration,
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
        Self::new_with_proxy_url(
            config,
            config.proxy.network_url(),
            Arc::new(arachnea_core::persistence::MemoryPersistenceStore::new()),
        )
    }

    /// Creates a configured chaser-cf session solver using the effective proxy
    /// URL selected by the HTTP client runtime and a shared persistence store.
    pub(crate) fn new_with_proxy_url(
        config: &ArachneaHttpConfig,
        proxy_url: Option<&str>,
        persistence_store: Arc<dyn PersistenceStore>,
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
                "--no-startup-window",
                // Turnstile's proof-of-work and fingerprinting rely on WebGPU /
                // WebGL. In GPU-less environments (VMs, RDP sessions, software
                // rendering) `navigator.gpu.requestAdapter()` otherwise returns
                // null ("No available adapters") and the challenge never
                // completes. SwiftShader is Chrome's software GPU.
                "--enable-unsafe-swiftshader",
                "--ignore-gpu-blocklist",
            ]);

        let engine = Self {
            config: chaser_config,
            proxy: chaser_proxy_from_url(proxy_url)?,
            session_cache: None,
        };
        Ok(engine
            .with_session_cache(persistence_store)
            .with_session_cache_refresh_margin(config.cookie_refresh_margin))
    }

    /// Creates an engine from native chaser-cf configuration.
    ///
    /// The shared browser is initialized from the provided configuration only
    /// when it has not been built yet; later engines reuse the same browser.
    ///
    /// # Errors
    ///
    /// Returns `ChaserCfFailure` when chaser-cf cannot be constructed.
    pub async fn from_chaser_config(config: ChaserConfig) -> Result<Self, ArachneaHttpError> {
        let mut slot = global_browser_slot().write().await;
        if slot.is_none() {
            let manager = Arc::new(
                BrowserManager::new(&config)
                    .await
                    .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?,
            );
            *slot = Some(manager);
        }
        drop(slot);
        Ok(Self {
            config,
            proxy: None,
            session_cache: None,
        })
    }

    /// Enables a persistent session cache backed by the provided store.
    pub fn with_session_cache(mut self, store: Arc<dyn PersistenceStore>) -> Self {
        self.session_cache = Some(ChaserSessionCache {
            store,
            refresh_margin: DEFAULT_SESSION_CACHE_REFRESH_MARGIN,
        });
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

    /// Returns the process-wide shared chaser-cf browser manager, creating it
    /// lazily when needed.
    async fn browser_manager(&self) -> Result<Arc<BrowserManager>, ArachneaHttpError> {
        let slot = global_browser_slot();
        if let Some(manager) = slot.read().await.as_ref() {
            return Ok(manager.clone());
        }

        let mut slot = slot.write().await;
        if let Some(manager) = slot.as_ref() {
            return Ok(manager.clone());
        }
        let manager = Arc::new(
            BrowserManager::new(&self.config)
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?,
        );
        *slot = Some(manager.clone());
        Ok(manager)
    }

    /// Opens a fresh page on the shared browser for a solver call.
    async fn open_solver_page(
        &self,
        manager: &BrowserManager,
        proxy: Option<&ProxyConfig>,
    ) -> Result<(Page, ChaserPage), ArachneaHttpError> {
        let ctx_id = manager
            .create_context(proxy)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let (page, chaser) = manager
            .new_page(ctx_id, "about:blank")
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        if let Err(err) = setup_proxy_auth(&page, proxy).await {
            if let Err(close_err) = page.close().await {
                warn!(error = %close_err, "failed to close chaser-cf page after proxy authentication failure");
            }
            return Err(err);
        }
        Ok((page, chaser))
    }

    /// Resolves a fresh Cloudflare session directly on the shared browser.
    async fn solve_waf_session(&self, url: &str) -> Result<WafSession, ArachneaHttpError> {
        let manager = self.browser_manager().await?;
        let _permit = manager
            .acquire_permit()
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let (page, chaser) = self.open_solver_page(&manager, self.proxy.as_ref()).await?;
        let result = async {
            chaser
                .goto(url)
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;

            wait_for_clearance(&page, &chaser, 90).await;

            let raw_cookies = page
                .get_cookies()
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
            let cookies = raw_cookies
                .into_iter()
                .map(|c| ChaserCookie {
                    name: c.name,
                    value: c.value,
                    domain: Some(c.domain),
                    path: Some(c.path),
                    expires: Some(c.expires),
                    http_only: Some(c.http_only),
                    secure: Some(c.secure),
                    same_site: c.same_site.map(|s| format!("{s:?}")),
                })
                .collect();

            let user_agent = chaser
                .evaluate("navigator.userAgent")
                .await
                .ok()
                .and_then(|v| v?.as_str().map(str::to_owned))
                .unwrap_or_default();

            let mut headers = HashMap::new();
            headers.insert("user-agent".to_string(), user_agent);

            let session = WafSession::new(cookies, headers);
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
        .await;

        if let Err(err) = page.close().await {
            warn!(error = %err, "failed to close chaser-cf solver page");
        }
        result
    }

    /// Fetches the page HTML directly on the shared browser.
    async fn fetch_page_source(&self, url: &str) -> Result<String, ArachneaHttpError> {
        let manager = self.browser_manager().await?;
        let _permit = manager
            .acquire_permit()
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let (page, chaser) = self.open_solver_page(&manager, self.proxy.as_ref()).await?;
        let result = async {
            chaser
                .goto(url)
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;

            wait_for_clearance(&page, &chaser, 30).await;

            chaser
                .content()
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))
        }
        .await;

        if let Err(err) = page.close().await {
            warn!(error = %err, "failed to close chaser-cf source page");
        }
        result
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
    async fn cached_session_headers(&self, url: &str) -> Option<HeaderMap> {
        let cache = self.session_cache.as_ref()?;
        match cache.headers_for_url(url).await {
            Ok(headers) => headers,
            Err(err) => {
                warn!(error = %err, "failed to read chaser-cf session cache");
                None
            }
        }
    }

    /// Stores a newly solved Cloudflare session in the optional cache.
    async fn store_session_cache(&self, url: &str, session: &WafSession) {
        let Some(cache) = &self.session_cache else {
            return;
        };
        if let Err(err) = cache.store_session(url, session).await {
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
            if let Some(headers) = self.cached_session_headers(&request.url).await {
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
        self.store_session_cache(&request.url, &session).await;
        Ok(EngineResponse {
            url: request.url,
            status: StatusCode::OK,
            headers,
            body: Bytes::new(),
        })
    }
}

#[async_trait]
impl HttpEngine for ChaserCfEngine {
    fn name(&self) -> &'static str {
        ENGINE_NAME
    }

    /// Executes a GET/HEAD request through the shared browser solver.
    ///
    /// Challenge handling is implemented directly: a fresh page navigates to the
    /// target, waits for `cf_clearance`, then returns the page HTML. The session
    /// cookies and browser user-agent are the only values exposed to the shared
    /// HTTP cache.
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
        self.store_session_cache(&request.url, &session).await;

        let body = if request.method == Method::GET {
            let html = self.fetch_page_source(&request.url).await?;
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

    /// Opens an interactive page session on the process-wide shared browser.
    ///
    /// The same Chrome instance used for Cloudflare session solving is reused
    /// here, so no second browser process is spawned. The operator loads the
    /// page and clicks the button that injects the Cloudflare captcha;
    /// `click_and_wait` then solves the captcha automatically and
    /// `read_turnstile_token` polls for the resulting token.
    async fn open_browser_page_session(
        &self,
    ) -> Result<Box<dyn BrowserPageSession>, ArachneaHttpError> {
        let manager = self.browser_manager().await?;
        let ctx_id = match manager.create_context(self.proxy.as_ref()).await {
            Ok(ctx_id) => ctx_id,
            Err(err) => {
                return Err(ArachneaHttpError::ChaserCfFailure(err.to_string()));
            }
        };
        let (page, chaser) = match manager.new_page(ctx_id, "about:blank").await {
            Ok(pair) => pair,
            Err(err) => {
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
                    return Err(ArachneaHttpError::ChaserCfFailure(err.to_string()));
                }
            }
        }
        // The site may call window.open() (player popup / ad) once the
        // captcha is solved. A new tab would steal focus from the solver
        // window: background tabs get their timers throttled, which stalls
        // Turnstile's proof-of-work until the tab is foregrounded again. The
        // init script runs in every frame before the page scripts, so the
        // popup is never opened and the solver tab keeps the focus.
        const POPUP_BLOCK_SCRIPT: &str = r#"
(() => {
    const record = (url) => {
        try {
            sessionStorage.setItem('chaser-cf-blocked-popup', String(url || '').slice(0, 512));
        } catch (_) {}
    };
    const original = window.open;
    if (typeof original === 'function') {
        window.open = function (url, target, features) {
            record(url);
            return null;
        };
    }
    document.addEventListener('click', (event) => {
        const anchor = event.target && event.target.closest ? event.target.closest('a') : null;
        if (anchor && anchor.target === '_blank') {
            record(anchor.href);
            event.preventDefault();
            event.stopPropagation();
        }
    }, true);
})();
"#;
        if let Err(err) = page.add_init_script(POPUP_BLOCK_SCRIPT).await {
            warn!(
                error = %err,
                "chaser-cf: failed to install popup blocker init script"
            );
        }
        info!(
            proxy = self.proxy.is_some(),
            "chaser-cf browser session opened"
        );
        Ok(Box::new(ChaserCfPageSession { page, chaser }))
    }
}

/// Interactive chaser-cf page session on the process-wide shared browser.
///
/// The page is retained for the session lifetime, which is what makes the
/// automatic Cloudflare flow possible in one window:
///
/// 1. `navigate` loads the target page.
/// 2. `click_and_wait` clicks the button that dynamically injects the
///    Cloudflare captcha and solves it automatically.
/// 3. `read_turnstile_token` polls for the resulting token.
/// 4. Once resolved, `metadata` exposes the resulting cookies and user-agent
///    and the captcha-driven iframe can be read through `fetch`.
struct ChaserCfPageSession {
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
        let url = self.page.url().await.ok().flatten().unwrap_or(request.url);
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
        debug_dump(&self.chaser, "after-click").await;

        let wait_script = format!(
            "(() => document.querySelector({selector}) !== null)()",
            selector = serde_json::to_string(&request.wait_for_selector)
                .map_err(|err| ArachneaHttpError::PageInteractionFailed(err.to_string()))?
        );
        let started = Instant::now();
        let mut deadline = started + CLICK_WAIT_TIMEOUT;
        let mut solve_started = false;
        let mut last_front = started - Duration::from_secs(1);
        loop {
            // If a popup still manages to open, the solver tab falls in the
            // background and its timers get throttled, stalling the captcha.
            // Re-activating the tab periodically counteracts that.
            if last_front.elapsed() >= Duration::from_millis(500) {
                self.page.bring_to_front().await.ok();
                last_front = Instant::now();
            }
            if !solve_started && challenge_is_present(&self.chaser).await {
                // The clicked button injected a Cloudflare captcha into the
                // page. Solve it automatically (wait for `cf_clearance` or a
                // Turnstile token and click the challenge) instead of relying
                // on a manual solve. The deadline is extended because the
                // target selector only appears once the captcha is solved.
                solve_started = true;
                deadline = started + CLICK_SOLVE_TIMEOUT + CLICK_WAIT_TIMEOUT;
                info!(
                    selector = %request.wait_for_selector,
                    "chaser-cf: captcha detected after page action click, solving"
                );
                wait_for_injected_captcha(&self.page, &self.chaser, CLICK_SOLVE_TIMEOUT).await;
                info!("chaser-cf: captcha solve finished");
            }

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
                info!("chaser-cf: page action result ready ({} bytes)", body.len());
                return Ok(PageClickResponse { body });
            }
            if Instant::now() >= deadline {
                warn!(
                    selector = %request.wait_for_selector,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "chaser-cf: timed out waiting for the page action result"
                );
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
        // The browser manager is shared process-wide and stays alive; dropping
        // the page alone does not close its Chrome tab.
        let ChaserCfPageSession { page, chaser: _ } = *self;
        if let Err(err) = page.close().await {
            warn!(error = %err, "failed to close chaser-cf browser session page");
        }
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

/// Serializable shape of the checkbox position returned by the in-frame
/// locator script.
#[derive(Debug, Deserialize)]
struct FrameClickPoint {
    x: f64,
    y: f64,
}

/// Applies HTTP proxy credentials to a page when the proxy requires auth.
async fn setup_proxy_auth(
    page: &Page,
    proxy: Option<&ProxyConfig>,
) -> Result<(), ArachneaHttpError> {
    if let Some(proxy) = proxy {
        if let (Some(username), Some(password)) = (&proxy.username, &proxy.password) {
            page.authenticate(Credentials {
                username: username.clone(),
                password: password.clone(),
            })
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(format!("proxy auth: {err}")))?;
        }
    }
    Ok(())
}

/// Polls until a `cf_clearance` cookie appears or the timeout expires.
///
/// For the first `CLEARANCE_PASSIVE_WAIT_MS` milliseconds nothing is done — the
/// managed challenge JS runs its invisible PoW/fingerprint in this window.
/// Polling the DOM during that phase causes timing anomalies that raise the bot
/// score. After the passive window a Turnstile widget, if present, is clicked
/// through its closed shadow root.
async fn wait_for_clearance(page: &Page, chaser: &ChaserPage, timeout_seconds: u64) {
    let started = Instant::now();
    let timeout = Duration::from_secs(timeout_seconds);
    let mut last_click = started - Duration::from_secs(30);

    loop {
        if has_clearance_cookie(page).await {
            tokio::time::sleep(Duration::from_millis(500)).await;
            return;
        }

        if started.elapsed() >= timeout {
            return;
        }

        if started.elapsed().as_millis() as u64 >= CLEARANCE_PASSIVE_WAIT_MS
            && last_click.elapsed().as_millis() as u64 >= CLEARANCE_CLICK_INTERVAL_MS
        {
            try_click_challenge(chaser).await;
            last_click = Instant::now();
        }

        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}

/// Returns true if the browser has a `cf_clearance` cookie for any domain.
async fn has_clearance_cookie(page: &Page) -> bool {
    page.get_cookies()
        .await
        .map(|cookies| cookies.iter().any(|c| c.name == "cf_clearance"))
        .unwrap_or(false)
}

/// Prints a snapshot of the main document (iframes, turnstile markers) when
/// `CHASER_CF_DEBUG` is set, to pinpoint where the injected captcha lives.
async fn debug_dump(chaser: &ChaserPage, stage: &str) {
    if std::env::var_os("CHASER_CF_DEBUG").is_none() {
        return;
    }
    let script = r#"(function() {
        var frames = [];
        document.querySelectorAll('iframe').forEach(function(f) {
            frames.push({ src: f.src || f.getAttribute('src') || '(none)', cls: f.className || '' });
        });
        return JSON.stringify({
            iframes: frames,
            turnstile: !!document.querySelector('#turnstile-wrapper, .cf-turnstile'),
            cfInputs: !!document.querySelector('input[name="cf-turnstile-response"], input[name="cf-response"]')
        });
    })()"#;
    let info = chaser
        .evaluate(script)
        .await
        .ok()
        .flatten()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "(unreadable)".to_string());
    eprintln!("[chaser-cf][{stage}] main-document snapshot: {info}");
}

/// Collects human-readable descriptions of every `<iframe>` element in the CDP
/// DOM tree, including whether its content document was exposed (OOPIF iframes
/// have no content document in the parent tree).
fn collect_iframes(node: &Node, out: &mut Vec<String>) {
    if node.node_name.eq_ignore_ascii_case("iframe") {
        let src = attr_value(node, "src").unwrap_or("(none)");
        let content = if node.content_document.is_some() {
            "exposed"
        } else {
            "not-exposed"
        };
        let doc_url = node.document_url.as_deref().unwrap_or("(none)");
        out.push(format!(
            "src={src:?} content_doc={content} doc_url={doc_url:?} frame_id={:?}",
            node.frame_id
        ));
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        collect_iframes(child, out);
    }
    for sr in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for child in sr.children.as_deref().unwrap_or(&[]) {
            collect_iframes(child, out);
        }
    }
    if let Some(content) = node.content_document.as_deref() {
        collect_iframes(content, out);
    }
}

/// Returns true when a Cloudflare challenge iframe or Turnstile widget has been
/// injected into the page by a page action, meaning the captcha needs solving.
async fn challenge_is_present(chaser: &ChaserPage) -> bool {
    let script = r#"(function() {
        if (document.querySelector('iframe[src*="challenges.cloudflare.com"]')) return true;
        if (document.querySelector('input[name="cf-turnstile-response"], input[name="cf-response"]')) return true;
        if (document.querySelector('#turnstile-wrapper, .cf-turnstile')) return true;
        return false;
    })()"#;
    chaser
        .evaluate(script)
        .await
        .ok()
        .flatten()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// Returns true when a solved Turnstile token is readable from the main page.
async fn has_turnstile_token(chaser: &ChaserPage) -> bool {
    let script = r#"(function() {
        if (window.turnstile && typeof window.turnstile.getResponse === 'function') {
            var t = window.turnstile.getResponse();
            if (t && t.length > 10) return true;
        }
        var el = document.querySelector('[name="cf-response"], [name="cf-turnstile-response"]');
        if (el && el.value && el.value.length > 10) return true;
        return false;
    })()"#;
    chaser
        .evaluate(script)
        .await
        .ok()
        .flatten()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// Polls until the captcha injected into the page by a page action is solved,
/// or the timeout expires.
///
/// Completion is signalled by a Turnstile token, or by the challenge widget
/// being removed from the DOM (which sites do once the solve callback fires).
/// A `cf_clearance` cookie is *not* a reliable signal here: the browser
/// session may already hold one from a previous run via the session cache,
/// which would short-circuit the wait without ever solving the injected
/// widget. While waiting, the challenge is clicked automatically (see
/// [`wait_for_clearance`] for the passive window).
async fn wait_for_injected_captcha(page: &Page, chaser: &ChaserPage, timeout: Duration) {
    let started = Instant::now();
    let mut last_click = started - Duration::from_secs(30);
    let mut last_front = started - Duration::from_secs(1);
    let mut absent_since: Option<Instant> = None;

    loop {
        if has_turnstile_token(chaser).await {
            info!("chaser-cf: captcha solved, turnstile token present");
            return;
        }
        if challenge_is_present(chaser).await {
            absent_since = None;
        } else {
            let absent = absent_since.unwrap_or(Instant::now());
            absent_since = Some(absent);
            if absent.elapsed() >= Duration::from_millis(1500) {
                info!("chaser-cf: captcha solved, widget removed from page");
                return;
            }
        }
        if started.elapsed() >= timeout {
            warn!(
                elapsed_ms = started.elapsed().as_millis() as u64,
                "chaser-cf: captcha solve timed out"
            );
            return;
        }

        if last_front.elapsed() >= Duration::from_millis(500) {
            page.bring_to_front().await.ok();
            last_front = Instant::now();
        }
        if started.elapsed().as_millis() as u64 >= CLEARANCE_PASSIVE_WAIT_MS
            && last_click.elapsed().as_millis() as u64 >= CLEARANCE_CLICK_INTERVAL_MS
        {
            try_click_challenge(chaser).await;
            last_click = Instant::now();
        }

        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}

/// Clicks the Turnstile challenge element by traversing its closed shadow root
/// via CDP, using a human-like cursor arc.
///
/// Three layouts are handled:
/// 1. The Cloudflare challenge page itself — the checkbox lives in a shadow
///    root of the page.
/// 2. A `turnstile.render(...)` widget injected into the main frame after a
///    page action (e.g. clicking a button) — the widget container owns the
///    `cf-turnstile-response` input and the shadow host is a descendant.
/// 3. A captcha injected into an `<iframe>` after a page action — the frame
///    owner element is located and the checkbox inside the frame is clicked at
///    its viewport position.
async fn try_click_challenge(chaser: &ChaserPage) {
    let page = chaser.raw_page();

    let doc = match page
        .execute(GetDocumentParams {
            depth: Some(-1),
            pierce: Some(true),
        })
        .await
    {
        Ok(r) => r,
        Err(_) => return,
    };
    let root = &doc.result.root;

    if std::env::var_os("CHASER_CF_DEBUG").is_some() {
        let mut frames = Vec::new();
        collect_iframes(root, &mut frames);
        eprintln!(
            "[chaser-cf][try_click_challenge] cdp iframes: {}",
            if frames.is_empty() {
                "(none)".to_string()
            } else {
                frames.join(" | ")
            }
        );
        eprintln!(
            "[chaser-cf][try_click_challenge] turnstile checkbox node: {:?}",
            find_turnstile_checkbox(root)
        );
        eprintln!(
            "[chaser-cf][try_click_challenge] challenge frame: {:?}",
            find_challenge_frame_node(root)
                .and_then(|node| attr_value(node, "src").map(str::to_owned))
        );
    }

    // Widget rendered directly in the main frame (either the Cloudflare
    // challenge page itself or a `turnstile.render(...)` widget injected by a
    // page action). The checkbox is inside a closed shadow root; click it at
    // the checkbox position.
    if let Some(target_id) = find_turnstile_checkbox(root) {
        info!(
            target_id = ?target_id,
            "chaser-cf: turnstile checkbox surface found, clicking"
        );
        if click_turnstile_surface(page, target_id).await {
            return;
        }
    }

    // Fallback: the Turnstile widget itself is an iframe served by
    // `challenges.cloudflare.com` rendered inside the shadow root. Clicking the
    // iframe at the checkbox position toggles the checkbox.
    if let Some(iframe_id) = find_turnstile_iframe(root) {
        info!(
            target_id = ?iframe_id,
            "chaser-cf: turnstile widget iframe found, clicking"
        );
        if click_turnstile_surface(page, iframe_id).await {
            return;
        }
    }

    // Captcha injected inside a Cloudflare iframe: click the frame owner
    // element at the checkbox position so the coordinates stay valid in the
    // main viewport (cross-frame node IDs are not clickable).
    if let Some(frame_node) = find_challenge_frame_node(root) {
        info!(
            src = attr_value(frame_node, "src").unwrap_or("(none)"),
            "chaser-cf: challenge iframe found, clicking frame owner"
        );
        click_challenge_frame(page, frame_node).await;
    }
}

/// Clicks the Turnstile widget surface at the checkbox position (near its left
/// edge, vertically centered), where the checkbox circle is rendered. This
/// works both for a precise checkbox element and for the widget `<iframe>`
/// itself.
async fn click_turnstile_surface(page: &Page, target_id: NodeId) -> bool {
    let box_model = match page
        .execute(GetBoxModelParams {
            node_id: Some(target_id),
            backend_node_id: None,
            object_id: None,
        })
        .await
    {
        Ok(r) => r,
        Err(_) => return false,
    };

    let content = box_model.result.model.content.inner();
    if content.len() < 8 {
        return false;
    }

    let left = content[0];
    let top = content[1];
    let width = content[2] - content[0];
    let height = content[5] - content[1];
    if width < 20.0 || height < 20.0 {
        return false;
    }

    let cx = left + (width * 0.5).min(26.0);
    let cy = top + height / 2.0;

    human_click(page, cx, cy).await;
    true
}

/// Clicks the Cloudflare challenge checkbox hosted by the iframe `frame_node`.
///
/// The checkbox position is resolved inside the frame's own execution context
/// when available, otherwise it falls back to the top-left corner of the widget
/// where the Turnstile checkbox is rendered.
async fn click_challenge_frame(page: &Page, frame_node: &Node) {
    let box_model = match page
        .execute(GetBoxModelParams {
            node_id: Some(frame_node.node_id),
            backend_node_id: None,
            object_id: None,
        })
        .await
    {
        Ok(r) => r,
        Err(_) => return,
    };

    let content = box_model.result.model.content.inner();
    if content.len() < 8 {
        return;
    }

    // Content-box top-left corner of the iframe in main-viewport coordinates.
    let origin_x = content[0];
    let origin_y = content[1];

    let (bx, by) = match challenge_checkbox_point(page, frame_node).await {
        Some(point) => point,
        None => fallback_checkbox_offset(),
    };

    human_click(page, origin_x + bx, origin_y + by).await;
}

/// Locates the Turnstile checkbox inside `frame_node`'s document and returns
/// its center relative to the frame's viewport.
async fn challenge_checkbox_point(page: &Page, frame_node: &Node) -> Option<(f64, f64)> {
    let frame_id = frame_node.frame_id.clone()?;
    let context_id = page
        .frame_execution_context(frame_id)
        .await
        .ok()
        .flatten()?;

    let script = r#"(function() {
        var candidates = [
            '.ctp-checkbox-label',
            '.ctp-checkbox',
            '#turnstile-wrapper label',
            'input[type="checkbox"]'
        ];
        for (var i = 0; i < candidates.length; i++) {
            var el = document.querySelector(candidates[i]);
            if (el) {
                var r = el.getBoundingClientRect();
                return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
            }
        }
        return null;
    })()"#;

    let params = EvaluateParams::builder()
        .expression(script)
        .context_id(context_id)
        .return_by_value(true)
        .build()
        .ok()?;
    let result = page.evaluate_expression(params).await.ok()?;
    let point: Option<FrameClickPoint> = result.into_value().ok()?;
    point.map(|point| (point.x, point.y))
}

/// Fallback checkbox offset used when the frame's execution context cannot be
/// reached: the Turnstile checkbox is rendered at the top-left of its iframe.
fn fallback_checkbox_offset() -> (f64, f64) {
    (18.0, 18.0)
}

/// Walks the CDP DOM tree and returns the frame owner node hosting a Cloudflare
/// challenge iframe.
fn find_challenge_frame_node(node: &Node) -> Option<&Node> {
    if is_challenge_frame(node) {
        return Some(node);
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        if let Some(found) = find_challenge_frame_node(child) {
            return Some(found);
        }
    }
    for sr in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for child in sr.children.as_deref().unwrap_or(&[]) {
            if let Some(found) = find_challenge_frame_node(child) {
                return Some(found);
            }
        }
    }
    if let Some(content) = node.content_document.as_deref() {
        if let Some(found) = find_challenge_frame_node(content) {
            return Some(found);
        }
    }
    None
}

/// Returns whether the node is an `<iframe>` that hosts a Cloudflare challenge.
fn is_challenge_frame(node: &Node) -> bool {
    if !node.node_name.eq_ignore_ascii_case("iframe") {
        return false;
    }
    // The `src` attribute carries the challenge URL even for cross-origin
    // OOPIF iframes, whose content document is not exposed in the parent DOM
    // tree and whose `document_url` is the parent document's URL.
    if let Some(src) = attr_value(node, "src") {
        if src.contains("challenges.cloudflare.com") {
            return true;
        }
    }
    if let Some(url) = node.document_url.as_deref() {
        if url.contains("challenges.cloudflare.com") {
            return true;
        }
    }
    if let Some(content) = node.content_document.as_deref() {
        if content
            .document_url
            .as_deref()
            .is_some_and(|url| url.contains("challenges.cloudflare.com"))
        {
            return true;
        }
        if subtree_has_response_input(content) {
            return true;
        }
    }
    false
}

/// Returns whether the subtree contains a Cloudflare Turnstile or managed
/// challenge response input.
fn subtree_has_response_input(node: &Node) -> bool {
    if node.node_name.eq_ignore_ascii_case("input")
        && attr_value(node, "name")
            .is_some_and(|name| name == "cf-turnstile-response" || name == "cf-chl-response")
    {
        return true;
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        if subtree_has_response_input(child) {
            return true;
        }
    }
    for sr in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for child in sr.children.as_deref().unwrap_or(&[]) {
            if subtree_has_response_input(child) {
                return true;
            }
        }
    }
    if let Some(content) = node.content_document.as_deref() {
        if subtree_has_response_input(content) {
            return true;
        }
    }
    false
}

/// Returns the value of the named attribute when present.
fn attr_value<'a>(node: &'a Node, wanted: &str) -> Option<&'a str> {
    node.attributes
        .as_deref()
        .unwrap_or(&[])
        .chunks(2)
        .find(|pair| pair.len() == 2 && pair[0] == wanted)
        .and_then(|pair| pair.get(1))
        .map(String::as_str)
}

/// Dispatches a human-like click at `(tx, ty)` using a ghost-cursor style cubic
/// Bezier arc from a random off-screen origin.
async fn human_click(page: &Page, tx: f64, ty: f64) {
    // Compute all random values in a synchronous block so the thread RNG is
    // dropped before any await point (ThreadRng is !Send).
    let (tx, ty, curve_points, post_pause_ms) = {
        let mut rng = rand::rng();

        let tx = tx + rng.random_range(-5.0..=5.0_f64);
        let ty = ty + rng.random_range(-4.0..=4.0_f64);

        // Ghost-cursor style cubic Bezier from a random off-screen origin.
        let p0x = tx + rng.random_range(-200.0..=-60.0_f64);
        let p0y = ty + rng.random_range(-120.0..=120.0_f64);
        let p1x = p0x + (tx - p0x) * rng.random_range(0.2..0.5_f64) + rng.random_range(-30.0..30.0);
        let p1y = p0y + (ty - p0y) * rng.random_range(0.1..0.4_f64) + rng.random_range(-40.0..40.0);
        let p2x = p0x + (tx - p0x) * rng.random_range(0.5..0.8_f64) + rng.random_range(-20.0..20.0);
        let p2y = p0y + (ty - p0y) * rng.random_range(0.5..0.9_f64) + rng.random_range(-20.0..20.0);

        let steps: u8 = rng.random_range(12..22);
        let mut points: Vec<(f64, f64, u64)> = Vec::with_capacity(steps as usize);
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let u = 1.0 - t;
            let bx =
                u * u * u * p0x + 3.0 * u * u * t * p1x + 3.0 * u * t * t * p2x + t * t * t * tx;
            let by =
                u * u * u * p0y + 3.0 * u * u * t * p1y + 3.0 * u * t * t * p2y + t * t * t * ty;
            let speed = (4.0 * t * (1.0 - t)).max(0.1);
            let step_ms = (rng.random_range(8.0..22.0_f64) / speed) as u64;
            points.push((bx, by, step_ms.min(80)));
        }

        let post_pause_ms = rng.random_range(40..120_u64);
        (tx, ty, points, post_pause_ms)
    };

    for (bx, by, step_ms) in curve_points {
        let _ = page.move_mouse(Point::new(bx, by)).await;
        tokio::time::sleep(Duration::from_millis(step_ms)).await;
    }

    tokio::time::sleep(Duration::from_millis(post_pause_ms)).await;
    let _ = page.click(Point::new(tx, ty)).await;
}

/// Walks the CDP DOM tree and returns the NodeId to click for the Turnstile
/// checkbox.
///
/// Two layouts are handled:
/// 1. The Cloudflare challenge page itself, where the shadow host also owns
///    the `cf-turnstile-response` input.
/// 2. An explicit `turnstile.render('#xf_lock', ...)` widget injected by a page
///    action, where the container holds the response input and the shadow host
///    is a nested descendant.
///
/// A precise checkbox element (`ctp-checkbox` / `input[type=checkbox]`) inside
/// any closed shadow root is preferred, since those class names are only used
/// by Turnstile. Otherwise the widget container — the node that owns the
/// `cf-turnstile-response` input — is located and its visible shadow-root
/// surface is returned (the whole Turnstile surface toggles on click).
fn find_turnstile_checkbox(node: &Node) -> Option<NodeId> {
    if let Some(id) = find_precise_checkbox(node) {
        return Some(id);
    }
    if let Some(container) = find_widget_container(node) {
        return checkbox_surface_within(container);
    }
    None
}

/// Returns the id of any Turnstile widget `<iframe>` (whose src is served by
/// `challenges.cloudflare.com`) present anywhere in the tree, including inside
/// closed shadow roots. Clicking the widget iframe toggles its checkbox.
fn find_turnstile_iframe(node: &Node) -> Option<NodeId> {
    if node.node_name.eq_ignore_ascii_case("iframe")
        && attr_value(node, "src")
            .map(|src| src.contains("challenges.cloudflare.com"))
            .unwrap_or(false)
    {
        return Some(node.node_id);
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        if let Some(id) = find_turnstile_iframe(child) {
            return Some(id);
        }
    }
    for sr in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for sr_child in sr.children.as_deref().unwrap_or(&[]) {
            if let Some(id) = find_turnstile_iframe(sr_child) {
                return Some(id);
            }
        }
    }
    if let Some(content) = node.content_document.as_deref() {
        if let Some(id) = find_turnstile_iframe(content) {
            return Some(id);
        }
    }
    None
}

/// Returns the id of the first Turnstile checkbox element found in any closed
/// shadow root of the tree.
fn find_precise_checkbox(node: &Node) -> Option<NodeId> {
    for sr in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for sr_child in sr.children.as_deref().unwrap_or(&[]) {
            if let Some(id) = checkbox_node_in_subtree(sr_child) {
                return Some(id);
            }
        }
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        if let Some(id) = find_precise_checkbox(child) {
            return Some(id);
        }
    }
    if let Some(content) = node.content_document.as_deref() {
        if let Some(id) = find_precise_checkbox(content) {
            return Some(id);
        }
    }
    None
}

/// Returns the node whose direct children include the hidden
/// `cf-turnstile-response` input — the Turnstile widget container.
fn find_widget_container(node: &Node) -> Option<&Node> {
    let has_input = node.children.as_deref().unwrap_or(&[]).iter().any(|child| {
        child.node_name.eq_ignore_ascii_case("input")
            && attr_value(child, "name") == Some("cf-turnstile-response")
    });
    if has_input {
        return Some(node);
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        if let Some(found) = find_widget_container(child) {
            return Some(found);
        }
    }
    if let Some(content) = node.content_document.as_deref() {
        if let Some(found) = find_widget_container(content) {
            return Some(found);
        }
    }
    None
}

/// Returns the visible shadow-root surface inside the widget container's
/// subtree (the element whose box toggles the Turnstile checkbox).
fn checkbox_surface_within(node: &Node) -> Option<NodeId> {
    for sr in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for sr_child in sr.children.as_deref().unwrap_or(&[]) {
            if !is_style_element(sr_child) && !is_hidden_node(sr_child) {
                return Some(sr_child.node_id);
            }
        }
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        if let Some(id) = checkbox_surface_within(child) {
            return Some(id);
        }
    }
    None
}

/// Returns the node id if `node` or one of its descendants looks like the
/// Turnstile checkbox element.
fn checkbox_node_in_subtree(node: &Node) -> Option<NodeId> {
    if is_checkbox_node(node) {
        return Some(node.node_id);
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        if let Some(id) = checkbox_node_in_subtree(child) {
            return Some(id);
        }
    }
    for sr in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for sr_child in sr.children.as_deref().unwrap_or(&[]) {
            if let Some(id) = checkbox_node_in_subtree(sr_child) {
                return Some(id);
            }
        }
    }
    None
}

/// Returns whether the node is the Turnstile checkbox surface.
fn is_checkbox_node(node: &Node) -> bool {
    if node.node_name.eq_ignore_ascii_case("input") && attr_value(node, "type") == Some("checkbox")
    {
        return true;
    }
    attr_value(node, "class")
        .map(|class| class.contains("ctp-checkbox"))
        .unwrap_or(false)
}

/// Returns whether the node is a `<style>` or `<link>` element.
fn is_style_element(node: &Node) -> bool {
    node.node_name.eq_ignore_ascii_case("style") || node.node_name.eq_ignore_ascii_case("link")
}

/// Returns whether the node is hidden via an inline `display: none` style.
fn is_hidden_node(node: &Node) -> bool {
    node.attributes
        .as_deref()
        .unwrap_or(&[])
        .chunks(2)
        .any(|p| p.len() == 2 && p[0] == "style" && p[1].contains("display: none"))
}

impl ChaserSessionCache {
    /// Returns cached headers for a still-valid Cloudflare session.
    async fn headers_for_url(&self, url: &str) -> Result<Option<HeaderMap>, ArachneaHttpError> {
        let origin = cache_origin_key(url)?;
        let transaction = self
            .store
            .transaction(CLOUDFLARE_SESSION_NAMESPACE)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let record = transaction
            .get(&origin)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let Some(record) = record else {
            return Ok(None);
        };
        if record.format_version != CLOUDFLARE_SESSION_FORMAT_VERSION {
            return Ok(None);
        }
        let session: CachedChaserSession = record
            .fields
            .map(|fields| serde_json::from_value(serde_json::Value::Object(fields)))
            .ok_or_else(|| {
                ArachneaHttpError::ChaserCfFailure(
                    "chaser-cf session record has no fields".to_string(),
                )
            })?
            .map_err(|err| {
                ArachneaHttpError::ChaserCfFailure(format!(
                    "failed to deserialize chaser-cf session: {err}"
                ))
            })?;
        if !session.is_usable(self.refresh_margin) {
            transaction
                .delete(&origin)
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
            transaction
                .commit()
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
            return Ok(None);
        }
        cached_session_headers(&session).map(Some)
    }

    /// Stores a newly solved Cloudflare session.
    async fn store_session(
        &self,
        url: &str,
        session: &WafSession,
    ) -> Result<(), ArachneaHttpError> {
        let origin = cache_origin_key(url)?;
        let Some(session) = CachedChaserSession::from_waf_session(session) else {
            return Ok(());
        };
        let fields = serde_json::to_value(&session)
            .map_err(|err| {
                ArachneaHttpError::ChaserCfFailure(format!(
                    "failed to serialize chaser-cf session: {err}"
                ))
            })?
            .as_object()
            .cloned()
            .ok_or_else(|| {
                ArachneaHttpError::ChaserCfFailure(
                    "failed to serialize chaser-cf session fields".to_string(),
                )
            })?;
        let record = PersistedRecord {
            format_version: CLOUDFLARE_SESSION_FORMAT_VERSION,
            fields: Some(fields),
            expires_at: session
                .clearance_expires_at
                .map(|expires_at| UNIX_EPOCH + Duration::from_secs(expires_at)),
            updated_at: SystemTime::now(),
        };
        let transaction = self
            .store
            .transaction(CLOUDFLARE_SESSION_NAMESPACE)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        transaction
            .put(origin, record)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        transaction
            .commit()
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))
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
            cookies: session
                .cookies
                .iter()
                .map(|cookie| StructuredCookie {
                    name: cookie.name.clone(),
                    value: cookie.value.clone(),
                    domain: cookie.domain.clone(),
                    path: cookie.path.clone(),
                    expires: cookie.expires,
                    http_only: cookie.http_only,
                    secure: cookie.secure,
                    same_site: cookie.same_site.clone(),
                })
                .collect(),
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
            None => self.stored_at.saturating_add(CACHE_TTL_NO_EXPIRY.as_secs()) > unix_timestamp(),
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

fn cached_session_headers(session: &CachedChaserSession) -> Result<HeaderMap, ArachneaHttpError> {
    let mut headers = HeaderMap::new();
    for cookie in &session.cookies {
        let chaser_cookie = ChaserCookie {
            name: cookie.name.clone(),
            value: cookie.value.clone(),
            domain: cookie.domain.clone(),
            path: cookie.path.clone(),
            expires: cookie.expires,
            http_only: cookie.http_only,
            secure: cookie.secure,
            same_site: cookie.same_site.clone(),
        };
        headers.append(
            SET_COOKIE,
            HeaderValue::from_str(&set_cookie_header(&chaser_cookie))
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
    use arachnea_core::persistence::MemoryPersistenceStore;

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

    #[tokio::test]
    async fn persistent_session_cache_returns_headers_for_valid_clearance() {
        let store = Arc::new(MemoryPersistenceStore::new());
        let cache = ChaserSessionCache {
            store,
            refresh_margin: Duration::from_secs(300),
        };
        cache
            .store_session(
                "https://example.com/search",
                &waf_session(unix_timestamp() + 3_600),
            )
            .await
            .expect("session stores");

        let headers = cache
            .headers_for_url("https://example.com/other")
            .await
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

    #[tokio::test]
    async fn persistent_session_cache_ignores_expiring_clearance() {
        let store = Arc::new(MemoryPersistenceStore::new());
        let cache = ChaserSessionCache {
            store,
            refresh_margin: Duration::from_secs(300),
        };
        cache
            .store_session(
                "https://example.com/search",
                &waf_session(unix_timestamp() + 30),
            )
            .await
            .expect("session stores");

        assert!(cache
            .headers_for_url("https://example.com/other")
            .await
            .expect("cache lookup succeeds")
            .is_none());
    }
}
