mod challenge;
mod page_session;
use challenge::*;
use page_session::*;

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use arachnea_core::persistence::TypedEntityStore;
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
use serde::Deserialize;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use url::Url;

use crate::{
    browser::{
        BrowserPageSession, BrowserSessionMetadata, PageClickRequest, PageClickResponse,
        PageFetchRequest, PageFetchResponse, PageNavigationRequest, PageNavigationResponse,
    },
    chaser_session::{
        memory_session_store, CachedChaserSession, StructuredCookie, CACHE_TTL_NO_EXPIRY,
    },
    config::ArachneaHttpConfig,
    engine::{EngineRequest, EngineResponse, HttpEngine, SOLVER_USER_AGENT_HEADER},
    error::ArachneaHttpError,
};

/// Registry name for the chaser-cf session solver.
pub use crate::HTTP_CHASER_ENGINE_NAME as ENGINE_NAME;

/// Default margin before a cached Cloudflare session is considered stale.
use crate::HTTP_CHASER_SESSION_CACHE_REFRESH_MARGIN as DEFAULT_SESSION_CACHE_REFRESH_MARGIN;

/// Dedicated timeout for chaser-cf browser challenge solving, kept independent
/// from the HTTP request timeout because solving a captcha (including browser
/// startup with lazy init) can take far longer than a regular request.
use crate::HTTP_CHASER_SOLVE_TIMEOUT as CHASER_SOLVE_TIMEOUT;

/// How long a retained interactive page waits for the target selector after a
/// programmatic click before giving up.
use crate::HTTP_CHASER_CLICK_WAIT_TIMEOUT as CLICK_WAIT_TIMEOUT;

/// How long the automatically solved Cloudflare captcha may take after a
/// page action click before `click_and_wait` gives up.
use crate::HTTP_CHASER_CLICK_SOLVE_TIMEOUT as CLICK_SOLVE_TIMEOUT;

/// How long `read_turnstile_token` polls for the token produced by the
/// automatically solved Cloudflare captcha before returning `None`.
use crate::HTTP_CHASER_TURNSTILE_WAIT_TIMEOUT as TURNSTILE_WAIT_TIMEOUT;

/// Passive wait before Cloudflare's invisible challenge JS starts its PoW.
use crate::HTTP_CHASER_CLEARANCE_PASSIVE_WAIT_MS as CLEARANCE_PASSIVE_WAIT_MS;

/// Minimum interval between automated Turnstile challenge clicks.
use crate::HTTP_CHASER_CLEARANCE_CLICK_INTERVAL_MS as CLEARANCE_CLICK_INTERVAL_MS;

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

/// Asynchronous cache for chaser-cf Cloudflare sessions backed by a typed repository.
struct ChaserSessionCache {
    /// Shared typed session repository.
    repository: Arc<dyn CloudflareSessionRepository>,
    /// Proactive refresh margin before `cf_clearance` expiry.
    refresh_margin: Duration,
}

/// Repository for persisted Cloudflare sessions.
#[async_trait]
trait CloudflareSessionRepository: Send + Sync {
    /// Loads the cached session for one origin key.
    ///
    /// # Arguments
    ///
    /// - `origin`: Scheme, host, and explicit port key produced by
    ///   [`cache_origin_key`].
    ///
    /// # Errors
    ///
    /// Returns the persistence backend failure.
    async fn get(&self, origin: &str) -> anyhow::Result<Option<CachedChaserSession>>;

    /// Saves a session under its origin key.
    ///
    /// # Arguments
    ///
    /// - `session`: Session to insert or replace.
    ///
    /// # Errors
    ///
    /// Returns the persistence backend failure.
    async fn save(&self, session: &CachedChaserSession) -> anyhow::Result<()>;

    /// Deletes the cached session for one origin key.
    ///
    /// # Arguments
    ///
    /// - `origin`: Origin key to remove.
    ///
    /// # Errors
    ///
    /// Returns the persistence backend failure.
    async fn delete(&self, origin: &str) -> anyhow::Result<()>;
}

/// Repository adapter backed by a typed entity store.
struct TypedCloudflareSessionRepository {
    store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
}

impl TypedCloudflareSessionRepository {
    /// Wraps a typed session store.
    ///
    /// # Arguments
    ///
    /// - `store`: Store used for `CachedChaserSession` records.
    fn new(store: Arc<dyn TypedEntityStore<CachedChaserSession>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl CloudflareSessionRepository for TypedCloudflareSessionRepository {
    async fn get(&self, origin: &str) -> anyhow::Result<Option<CachedChaserSession>> {
        self.store.get(&origin.to_string()).await
    }
    async fn save(&self, session: &CachedChaserSession) -> anyhow::Result<()> {
        self.store.put(session).await
    }
    async fn delete(&self, origin: &str) -> anyhow::Result<()> {
        self.store.delete(&origin.to_string()).await
    }
}

impl Default for ChaserCfEngine {
    fn default() -> Self {
        let config = ArachneaHttpConfig::builder().build().unwrap();
        Self::new(&config).unwrap()
    }
}

impl ChaserCfEngine {
    /// Creates a configured chaser-cf session solver using the network proxy
    /// URL from `config` and an in-memory session store.
    ///
    /// # Arguments
    ///
    /// - `config`: Client configuration supplying the proxy and refresh margin.
    ///
    /// # Returns
    ///
    /// A solver with a persistent in-memory session cache.
    ///
    /// # Errors
    ///
    /// Returns `ChaserCfFailure` when the in-memory session store is
    /// unavailable, or an error when the configured proxy cannot be shared with
    /// chaser-cf and rquest.
    pub fn new(config: &ArachneaHttpConfig) -> Result<Self, ArachneaHttpError> {
        Self::new_with_proxy_url(
            config,
            config.proxy.network_url(),
            memory_session_store().map_err(|error| {
                ArachneaHttpError::ChaserCfFailure(format!(
                    "session cache store unavailable: {error}"
                ))
            })?,
        )
    }

    /// Creates a configured chaser-cf session solver using the effective proxy
    /// URL selected by the HTTP client runtime and a shared typed session store.
    ///
    /// Chrome configuration comes from `ChaserConfig::from_env()`, then forces a
    /// non-headless, lazily initialized Windows profile with the adapter's
    /// automation and software-GPU flags. Existing `CHASER_EXTRA_ARGS` entries
    /// are preserved.
    ///
    /// # Arguments
    ///
    /// - `config`: Client configuration supplying the refresh margin.
    /// - `proxy_url`: Effective proxy URL, if any. It must include a host and
    ///   port; credentials require a password.
    /// - `session_store`: Store used for cached sessions.
    ///
    /// # Returns
    ///
    /// A solver with caching enabled.
    ///
    /// # Errors
    ///
    /// Returns `InvalidConfiguration` for an invalid proxy URL or incomplete
    /// credentials.
    pub(crate) fn new_with_proxy_url(
        config: &ArachneaHttpConfig,
        proxy_url: Option<&str>,
        session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    ) -> Result<Self, ArachneaHttpError> {
        // Defaults are appended one by one with `add_extra_arg` instead of
        // `with_extra_args`: the latter *replaces* the whole set, which would
        // silently discard the flags `ChaserConfig::from_env` just parsed from
        // `CHASER_EXTRA_ARGS` and turn that documented override into a no-op.
        let chaser_config = ChaserConfig::from_env()
            .with_headless(false)
            .with_timeout(CHASER_SOLVE_TIMEOUT)
            .with_lazy_init(true)
            .with_profile(Profile::Windows)
            .add_extra_arg("--disable-blink-features=AutomationControlled")
            .add_extra_arg("--disable-infobars")
            .add_extra_arg("--no-first-run")
            .add_extra_arg("--no-default-browser-check")
            .add_extra_arg("--no-startup-window")
            // Turnstile's proof-of-work and fingerprinting rely on WebGPU /
            // WebGL. In GPU-less environments (VMs, RDP sessions, software
            // rendering) `navigator.gpu.requestAdapter()` otherwise returns
            // null ("No available adapters") and the challenge never
            // completes. SwiftShader is Chrome's software GPU.
            .add_extra_arg("--enable-unsafe-swiftshader")
            .add_extra_arg("--ignore-gpu-blocklist");

        let engine = Self {
            config: chaser_config,
            proxy: chaser_proxy_from_url(proxy_url)?,
            session_cache: None,
        };
        Ok(engine
            .with_session_cache(session_store)
            .with_session_cache_refresh_margin(config.cookie_refresh_margin))
    }

    /// Creates an engine from native chaser-cf configuration.
    ///
    /// The shared browser is initialized from the provided configuration only
    /// when it has not been built yet; later engines reuse the same browser and
    /// their configuration does not replace the first one. This method also
    /// constructs the browser eagerly.
    ///
    /// # Arguments
    ///
    /// - `config`: Native chaser-cf configuration, including browser startup
    ///   options. It is retained for any later lazy initialization.
    ///
    /// # Returns
    ///
    /// A solver without a persistent session cache and without a proxy route.
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

    /// Enables a persistent session cache backed by the provided typed store.
    ///
    /// Replaces any previously configured cache and resets its refresh margin to
    /// the adapter's default margin.
    ///
    /// # Arguments
    ///
    /// - `session_store`: Typed store for cached sessions.
    ///
    /// # Returns
    ///
    /// The updated solver.
    pub fn with_session_cache(
        mut self,
        session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    ) -> Self {
        self.session_cache = Some(ChaserSessionCache {
            repository: Arc::new(TypedCloudflareSessionRepository::new(session_store)),
            refresh_margin: DEFAULT_SESSION_CACHE_REFRESH_MARGIN,
        });
        self
    }

    /// Updates the refresh margin used by the persistent session cache.
    ///
    /// This has no effect when the cache is disabled.
    ///
    /// # Arguments
    ///
    /// - `margin`: Time before `cf_clearance` expiry at which a cached session is
    ///   treated as unusable.
    ///
    /// # Returns
    ///
    /// The updated solver.
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
    ///
    /// The page is closed by the caller. Proxy authentication setup failure
    /// closes the page before returning.
    ///
    /// # Arguments
    ///
    /// - `manager`: Shared browser manager.
    /// - `proxy`: Optional proxy configuration for the new context.
    ///
    /// # Returns
    ///
    /// The raw page and its high-level chaser page handle.
    ///
    /// # Errors
    ///
    /// Returns `ChaserCfFailure` when context/page creation or proxy
    /// authentication fails.
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
    ///
    /// Waits up to 90 seconds for clearance, then requires a `cf_clearance`
    /// cookie. The page is closed even when solving fails.
    ///
    /// # Arguments
    ///
    /// - `url`: URL to navigate to for obtaining the session.
    ///
    /// # Returns
    ///
    /// Browser cookies and the observed user-agent.
    ///
    /// # Errors
    ///
    /// Returns `CookieAbsent` when `cf_clearance` is missing, or
    /// `ChaserCfFailure` for browser, navigation, or evaluation failures.
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
    ///
    /// Waits up to 30 seconds for clearance before reading the content. The page
    /// is closed even when fetching fails.
    ///
    /// # Arguments
    ///
    /// - `url`: Page URL to navigate to.
    ///
    /// # Returns
    ///
    /// The rendered page HTML.
    ///
    /// # Errors
    ///
    /// Returns `ChaserCfFailure` for browser, navigation, or content failures.
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
    ///
    /// # Arguments
    ///
    /// - `cookies`: Browser cookies to serialize as `Set-Cookie` headers.
    ///
    /// # Returns
    ///
    /// A header map containing one `Set-Cookie` entry per cookie.
    ///
    /// # Errors
    ///
    /// Returns `CookieAbsent` when `cf_clearance` is missing, or `InvalidHeader`
    /// when a cookie cannot be represented as a header value.
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
    ///
    /// An absent or blank user-agent is ignored.
    ///
    /// # Arguments
    ///
    /// - `headers`: Header map to update.
    /// - `session`: Solved session carrying the browser user-agent.
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
    /// Cache read failures are logged and treated as a cache miss.
    ///
    /// # Arguments
    ///
    /// - `url`: URL whose origin selects the cached session.
    ///
    /// # Returns
    ///
    /// `Set-Cookie` and solver user-agent headers, or `None` when the cache is
    /// disabled, misses, or fails.
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
    ///
    /// Cache write failures are logged and ignored. Sessions without
    /// `cf_clearance` are not stored.
    ///
    /// # Arguments
    ///
    /// - `url`: URL whose origin keys the cached session.
    /// - `session`: Freshly solved browser session.
    async fn store_session_cache(&self, url: &str, session: &WafSession) {
        let Some(cache) = &self.session_cache else {
            return;
        };
        if let Err(err) = cache.store_session(url, session).await {
            warn!(error = %err, "failed to write chaser-cf session cache");
        }
    }

    /// Refreshes Cloudflare cookies, optionally reusing a cached session.
    ///
    /// Only `GET` and `HEAD` are accepted; the request body and headers are not
    /// used for solving. A cache hit returns status `200` with an empty body and
    /// `Set-Cookie` headers. A miss solves a fresh session, stores it, and
    /// returns the same shape.
    ///
    /// # Arguments
    ///
    /// - `request`: Request whose URL selects the origin.
    /// - `use_session_cache`: Whether a usable cached session may satisfy the
    ///   refresh.
    ///
    /// # Returns
    ///
    /// A synthetic `200` response carrying solver cookies.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` for methods other than `GET`/`HEAD`,
    /// `CookieAbsent` when solving yields no `cf_clearance`, or solver failures.
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
    ///
    /// The response status is always `200` when solving succeeds. `GET` returns
    /// the rendered HTML with `Content-Type: text/html; charset=utf-8`; `HEAD`
    /// returns an empty body. Request headers and body are not forwarded to the
    /// browser.
    ///
    /// # Arguments
    ///
    /// - `request`: Request whose URL is loaded in the shared browser.
    ///
    /// # Returns
    ///
    /// A synthetic response carrying solver cookies and optional HTML.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` for methods other than `GET`/`HEAD`,
    /// `CookieAbsent` when solving yields no `cf_clearance`, or solver failures.
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
        use crate::HTTP_CHASER_SCRIPT_OPEN_BROWSER_PAGE_SESSION as POPUP_BLOCK_SCRIPT;
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
impl ChaserSessionCache {
    /// Returns cached headers for a still-valid Cloudflare session.
    ///
    /// A cached session that is expired or inside the refresh margin is deleted
    /// before returning a miss.
    ///
    /// # Arguments
    ///
    /// - `url`: URL whose origin keys the session.
    ///
    /// # Returns
    ///
    /// `Set-Cookie` and solver user-agent headers, or `None` on a cache miss.
    ///
    /// # Errors
    ///
    /// Returns `InvalidUrl` for an unusable URL, `InvalidHeader` for an
    /// unrepresentable header, or `ChaserCfFailure` for repository failures.
    async fn headers_for_url(&self, url: &str) -> Result<Option<HeaderMap>, ArachneaHttpError> {
        let origin = cache_origin_key(url)?;
        let session = self
            .repository
            .get(&origin)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let Some(session) = session else {
            return Ok(None);
        };
        if !session.is_usable(self.refresh_margin) {
            self.repository
                .delete(&origin)
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
            return Ok(None);
        }
        cached_session_headers(&session).map(Some)
    }

    /// Stores a newly solved Cloudflare session.
    ///
    /// Sessions without `cf_clearance` are ignored.
    ///
    /// # Arguments
    ///
    /// - `url`: URL whose origin keys the session.
    /// - `session`: Freshly solved browser session.
    ///
    /// # Errors
    ///
    /// Returns `InvalidUrl` for an unusable URL or `ChaserCfFailure` when the
    /// repository write fails.
    async fn store_session(
        &self,
        url: &str,
        session: &WafSession,
    ) -> Result<(), ArachneaHttpError> {
        let origin = cache_origin_key(url)?;
        let Some(mut session) = CachedChaserSession::from_waf_session(session) else {
            return Ok(());
        };
        session.origin = origin;
        self.repository
            .save(&session)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))
    }
}

impl CachedChaserSession {
    /// Converts a solved WAF session into a cache record.
    ///
    /// # Arguments
    ///
    /// - `session`: Solved browser session.
    ///
    /// # Returns
    ///
    /// `None` when the session lacks `cf_clearance`. The returned record has an
    /// empty origin that the caller must replace with the cache key.
    fn from_waf_session(session: &WafSession) -> Option<Self> {
        if !session
            .cookies
            .iter()
            .any(|cookie| cookie.name == "cf_clearance")
        {
            return None;
        }
        Some(Self {
            origin: String::new(),
            cookies: session.cookies.iter().map(StructuredCookie::from).collect(),
            user_agent: session
                .headers
                .get("user-agent")
                .filter(|value| !value.trim().is_empty())
                .cloned(),
            clearance_expires_at: clearance_expires_at(&session.cookies),
            stored_at: unix_timestamp(),
        })
    }

    /// Returns whether the cached session is still usable.
    ///
    /// A known `cf_clearance` expiry must be later than now plus
    /// `refresh_margin`. Without a known expiry, the session is usable for
    /// [`CACHE_TTL_NO_EXPIRY`] from `stored_at`.
    ///
    /// # Arguments
    ///
    /// - `refresh_margin`: Proactive refresh margin.
    fn is_usable(&self, refresh_margin: Duration) -> bool {
        match self.clearance_expires_at {
            Some(expires_at) => {
                expires_at > unix_timestamp().saturating_add(refresh_margin.as_secs())
            }
            None => self.stored_at.saturating_add(CACHE_TTL_NO_EXPIRY.as_secs()) > unix_timestamp(),
        }
    }
}

impl From<&ChaserCookie> for StructuredCookie {
    fn from(cookie: &ChaserCookie) -> Self {
        Self {
            name: cookie.name.clone(),
            value: cookie.value.clone(),
            domain: cookie.domain.clone(),
            path: cookie.path.clone(),
            expires: cookie.expires,
            http_only: cookie.http_only,
            secure: cookie.secure,
            same_site: cookie.same_site.clone(),
        }
    }
}

impl From<&StructuredCookie> for ChaserCookie {
    fn from(cookie: &StructuredCookie) -> Self {
        Self {
            name: cookie.name.clone(),
            value: cookie.value.clone(),
            domain: cookie.domain.clone(),
            path: cookie.path.clone(),
            expires: cookie.expires,
            http_only: cookie.http_only,
            secure: cookie.secure,
            same_site: cookie.same_site.clone(),
        }
    }
}

/// Converts a proxy URL into chaser-cf proxy configuration.
///
/// # Arguments
///
/// - `proxy_url`: Optional proxy URL. It must contain a host and a port
///   (explicit or scheme default).
///
/// # Returns
///
/// `None` when no proxy URL is supplied; otherwise a proxy config preserving the
/// URL scheme and optional username/password credentials.
///
/// # Errors
///
/// Returns `InvalidConfiguration` when the URL is invalid, lacks a host or port,
/// or has a username without a password.
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

/// Builds HTTP headers from a cached session.
///
/// # Arguments
///
/// - `session`: Cached cookies and optional user-agent.
///
/// # Returns
///
/// One `Set-Cookie` header per cached cookie, plus the solver user-agent header
/// when present.
///
/// # Errors
///
/// Returns `InvalidHeader` when a value cannot be represented as a header.
fn cached_session_headers(session: &CachedChaserSession) -> Result<HeaderMap, ArachneaHttpError> {
    let mut headers = HeaderMap::new();
    for cookie in &session.cookies {
        let chaser_cookie = ChaserCookie::from(cookie);
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

/// Serializes one browser cookie as a `Set-Cookie` value.
///
/// # Arguments
///
/// - `cookie`: Browser cookie to serialize.
///
/// # Returns
///
/// A `name=value` string with the available attributes. Invalid or missing
/// expiry values omit `Expires`.
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

/// Returns the `cf_clearance` expiration as a Unix timestamp.
///
/// # Arguments
///
/// - `cookies`: Browser cookies to inspect.
///
/// # Returns
///
/// The first finite, non-negative `cf_clearance` expiry truncated to whole
/// seconds, or `None` when absent or invalid.
fn clearance_expires_at(cookies: &[ChaserCookie]) -> Option<u64> {
    cookies
        .iter()
        .find(|cookie| cookie.name == "cf_clearance")
        .and_then(|cookie| cookie.expires)
        .filter(|expires| expires.is_finite() && *expires >= 0.0)
        .map(|expires| expires as u64)
}

/// Formats a Unix timestamp as an HTTP date.
///
/// # Arguments
///
/// - `expires`: Optional Unix timestamp in seconds.
///
/// # Returns
///
/// An HTTP date, or `None` when the value is absent, non-finite, negative, or out
/// of range.
fn expires_http_date(expires: Option<f64>) -> Option<String> {
    let expires = expires?;
    if !expires.is_finite() || expires < 0.0 {
        return None;
    }
    let duration = Duration::try_from_secs_f64(expires).ok()?;
    let instant = UNIX_EPOCH.checked_add(duration)?;
    Some(httpdate::fmt_http_date(instant))
}

/// Builds the session-cache key for a URL.
///
/// # Arguments
///
/// - `value`: Absolute URL.
///
/// # Returns
///
/// `scheme://host[:explicit-port]/`. User info, path, and query are discarded;
/// default ports are omitted.
///
/// # Errors
///
/// Returns `InvalidUrl` when the URL cannot be parsed or has no host.
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

/// Returns the current Unix timestamp in whole seconds.
///
/// A clock before the Unix epoch maps to `0`.
fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
#[path = "../../tests/engine/chaser_cf.rs"]
mod tests;
