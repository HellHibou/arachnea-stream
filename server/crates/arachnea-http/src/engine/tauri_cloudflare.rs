mod app_session;
mod event_loop;
use app_session::*;
use event_loop::*;

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use arachnea_core::controler::{global_main_thread_dispatcher, MainThreadContext};
use async_trait::async_trait;
use bytes::Bytes;
use cookie::Cookie;
use http::{header::SET_COOKIE, HeaderMap, HeaderValue, Method, StatusCode};
use serde::Deserialize;
#[cfg(target_os = "windows")]
use tao::platform::windows::EventLoopBuilderExtWindows;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder},
    platform::run_return::EventLoopExtRunReturn,
    window::WindowBuilder,
};
use url::Url;
use wry::{NewWindowResponse, WebViewBuilder};

use crate::{
    config::ArachneaHttpConfig,
    engine::{EngineRequest, EngineResponse, HttpEngine, SOLVER_USER_AGENT_HEADER},
    error::ArachneaHttpError,
};

#[path = "tauri_cloudflare_proxy.rs"]
mod proxy;
use proxy::WebviewProxy;

/// Registry name for the interactive Tauri/Wry Cloudflare solver.
pub use crate::HTTP_TAURI_ENGINE_NAME as ENGINE_NAME;

/// Default interval used to inspect the WebView cookie jar.
use crate::HTTP_TAURI_COOKIE_POLL_INTERVAL as DEFAULT_COOKIE_POLL_INTERVAL;

/// Default delay after `cf_clearance` appears before the WebView closes.
use crate::HTTP_TAURI_CLEARANCE_STABILIZATION as DEFAULT_CLEARANCE_STABILIZATION;

/// Delay before forcing a post-clearance reload when the page does not navigate by itself.
use crate::HTTP_TAURI_POST_CLEARANCE_RELOAD_DELAY as DEFAULT_POST_CLEARANCE_RELOAD_DELAY;

/// Minimum post-clearance wait used by Tauri desktop WebviewWindow sessions.
use crate::HTTP_TAURI_APP_CLEARANCE_STABILIZATION as DEFAULT_TAURI_APP_CLEARANCE_STABILIZATION;

/// Minimum fallback reload delay used by Tauri desktop WebviewWindow sessions.
use crate::HTTP_TAURI_APP_POST_CLEARANCE_RELOAD_DELAY as DEFAULT_TAURI_APP_POST_CLEARANCE_RELOAD_DELAY;

/// Default interactive solver window width.
use crate::HTTP_TAURI_WINDOW_WIDTH as DEFAULT_WINDOW_WIDTH;

/// Default interactive solver window height.
use crate::HTTP_TAURI_WINDOW_HEIGHT as DEFAULT_WINDOW_HEIGHT;

/// Time window where a repeated refresh reuses the last interactive result.
use crate::HTTP_TAURI_REFRESH_CACHE_TTL as DEFAULT_REFRESH_CACHE_TTL;

/// Maximum time to keep pumping the solver loop after dropping the window.
use crate::HTTP_TAURI_WINDOW_DESTROY_GRACE as DEFAULT_WINDOW_DESTROY_GRACE;

/// Monotonic window counter used to generate unique Tauri WebviewWindow labels.
static TAURI_SOLVER_WINDOW_ID: AtomicU64 = AtomicU64::new(1);

/// Recently extracted WebView cookies for one origin.
#[derive(Clone)]
struct CachedRefresh {
    /// Headers synthesized from the WebView cookie jar.
    headers: HeaderMap,
    /// Time when the headers were extracted.
    stored_at: Instant,
}

/// Result extracted from an interactive WebView solve.
struct WrySolveResult {
    /// Headers synthesized from the WebView cookie jar.
    headers: HeaderMap,
    /// Optional page body collected from the WebView.
    body: Bytes,
}

/// Tauri desktop app solve session for interactive Cloudflare verification.
///
/// Holds a reference to an existing Tauri WebviewWindow and the state needed
/// to poll cookies, detect page loads, and collect the final page state.
struct TauriAppSolveSession {
    /// Tauri application handle used to dispatch main-thread operations.
    app_handle: tauri::AppHandle<tauri::Wry>,
    /// The Tauri WebviewWindow showing the Cloudflare-protected page.
    window: tauri::WebviewWindow<tauri::Wry>,
    /// Current URL tracked via navigation events.
    current_url: Arc<Mutex<String>>,
    /// Count of completed page loads since the session started.
    page_load_finished_count: Arc<AtomicU64>,
    /// Event name used to receive page state from JavaScript.
    page_state_event: String,
    /// Listener ID used to unregister when the session ends.
    page_state_event_id: tauri::EventId,
    /// Latest page state received via the Tauri event system.
    page_state_result: Arc<Mutex<Option<Result<TauriAppPageState, String>>>>,
    /// Whether the window has been closed by the user.
    closed: Arc<AtomicBool>,
    /// Completion of native proxy setup before the first external navigation.
    proxy_ready: Option<tokio::sync::oneshot::Receiver<Result<(), String>>>,
}

/// Page state snapshot collected from the Tauri WebView via JavaScript.
#[derive(Deserialize)]
struct TauriAppPageState {
    /// HTML source when page source collection is enabled.
    #[serde(default)]
    html: String,
    /// Browser user-agent string reported by navigator.userAgent.
    #[serde(default, rename = "userAgent")]
    user_agent: String,
}

/// Result of starting a Cloudflare solve on the main thread.
///
/// The solve either completes synchronously (standalone Wry event loop) or
/// returns a session handle for an existing Tauri application window.
enum MainThreadSolveStart {
    /// Solve completed synchronously via the standalone Wry event loop.
    Completed(WrySolveResult),
    /// Solve started as a Tauri desktop app session that needs async polling.
    TauriApp(TauriAppSolveSession),
}

/// Interactive Cloudflare solver backed by a Tauri-compatible Wry WebView.
///
/// Wry/Tao UI objects are portable only when they are created and driven on
/// the process main thread. When a controller installs a main-thread
/// dispatcher, this solver routes UI work through it. Tauri desktop runtimes
/// reuse the existing Tauri event loop with a `WebviewWindow`; REST runtimes
/// can run the Wry event loop from the controller's main-thread dispatcher. On
/// Windows, the standalone Wry path also opts in to Tao's any-thread event loop
/// support so server handlers can open the verification window from a Tokio
/// worker thread.
#[derive(Clone)]
pub struct TauriCloudflareSolverEngine {
    /// Maximum time the user can spend solving the challenge.
    timeout: Duration,
    /// Immutable routing endpoint; invalid standalone configurations fail before navigation.
    proxy: Result<Option<WebviewProxy>, String>,
    /// Optional user-agent override configured in the WebView.
    user_agent: Option<String>,
    /// Interval between cookie jar inspections.
    poll_interval: Duration,
    /// Time to keep the WebView open after the clearance cookie appears.
    clearance_stabilization: Duration,
    /// Initial logical window width.
    window_width: f64,
    /// Initial logical window height.
    window_height: f64,
    /// Whether the WebView devtools should open automatically.
    devtools: bool,
    /// Whether the WebView should close automatically after clearance appears.
    auto_close_after_clearance: bool,
    /// Whether the WebView should use a non-persistent browser data store.
    incognito: bool,
    /// Recently solved origins used to debounce repeated refresh requests.
    refresh_cache: Arc<Mutex<HashMap<String, CachedRefresh>>>,
    /// Last successful interactive result, regardless of origin.
    latest_refresh: Arc<Mutex<Option<CachedRefresh>>>,
}

impl Default for TauriCloudflareSolverEngine {
    fn default() -> Self {
        let solver_config = ArachneaHttpConfig::builder().build().unwrap();
        TauriCloudflareSolverEngine::new(&solver_config)
    }
}

impl TauriCloudflareSolverEngine {
    /// Creates an interactive Cloudflare solver.
    ///
    /// # Parameters
    ///
    /// - `config`: Client timing and explicit network-proxy configuration.
    ///
    /// Invalid proxy settings are reported before a request can navigate. An
    /// in-process core requires the HTTP facade's prepared loopback URL.
    ///
    /// # Returns
    ///
    /// A configured Tauri/Wry solver adapter.
    pub fn new(config: &ArachneaHttpConfig) -> Self {
        Self {
            timeout: config.request_timeout,
            proxy: match &config.proxy {
                crate::HttpProxyConfig::Disabled => Ok(None),
                crate::HttpProxyConfig::Network(url) => WebviewProxy::parse(url).map(Some).map_err(|error| error.to_string()),
                #[cfg(feature = "arachnea-proxy")]
                crate::HttpProxyConfig::Arachnea(_) => Err("Tauri/Wry requires the prepared loopback URL; construct it through ArachneaHttpClient".into()),
            },
            user_agent: None,
            poll_interval: DEFAULT_COOKIE_POLL_INTERVAL,
            clearance_stabilization: DEFAULT_CLEARANCE_STABILIZATION,
            window_width: DEFAULT_WINDOW_WIDTH,
            window_height: DEFAULT_WINDOW_HEIGHT,
            devtools: crate::HTTP_TAURI_DEFAULT_DEVTOOLS,
            auto_close_after_clearance: crate::HTTP_TAURI_DEFAULT_AUTO_CLOSE_AFTER_CLEARANCE,
            incognito: crate::HTTP_TAURI_DEFAULT_INCOGNITO,
            refresh_cache: Arc::new(Mutex::new(HashMap::new())),
            latest_refresh: Arc::new(Mutex::new(None)),
        }
    }

    /// Creates a solver using the effective proxy URL prepared by the HTTP facade.
    ///
    /// # Arguments
    /// - `config`: Solver timing and browser configuration.
    /// - `proxy_url`: Network endpoint including optional proxy credentials.
    ///
    /// # Errors
    /// Returns an error if the endpoint or native platform cannot support proxy routing.
    pub fn new_with_proxy_url(
        config: &ArachneaHttpConfig,
        proxy_url: Option<&str>,
    ) -> Result<Self, ArachneaHttpError> {
        if proxy_url.is_none() && !matches!(config.proxy, crate::HttpProxyConfig::Disabled) {
            return Err(ArachneaHttpError::InvalidConfiguration(
                "Tauri/Wry requires the effective proxy URL before navigation".into(),
            ));
        }
        let proxy = proxy_url.map(WebviewProxy::parse).transpose()?;
        let mut engine = Self::new(config);
        engine.proxy = Ok(proxy);
        Ok(engine)
    }

    /// Creates an interactive Cloudflare solver with explicit timing values.
    ///
    /// # Parameters
    ///
    /// - `timeout`: Maximum time the user can spend solving the challenge.
    /// - `user_agent`: User-agent override used by the WebView.
    /// - `poll_interval`: Interval between cookie jar inspections.
    ///
    /// # Returns
    ///
    /// A configured Tauri/Wry solver adapter.
    pub fn with_options(timeout: Duration, user_agent: String, poll_interval: Duration) -> Self {
        Self {
            timeout,
            proxy: Ok(None),
            user_agent: Some(user_agent),
            poll_interval,
            clearance_stabilization: DEFAULT_CLEARANCE_STABILIZATION,
            window_width: DEFAULT_WINDOW_WIDTH,
            window_height: DEFAULT_WINDOW_HEIGHT,
            devtools: crate::HTTP_TAURI_DEFAULT_DEVTOOLS,
            auto_close_after_clearance: crate::HTTP_TAURI_DEFAULT_AUTO_CLOSE_AFTER_CLEARANCE,
            incognito: crate::HTTP_TAURI_DEFAULT_INCOGNITO,
            refresh_cache: Arc::new(Mutex::new(HashMap::new())),
            latest_refresh: Arc::new(Mutex::new(None)),
        }
    }

    /// Sets a user-agent override for the WebView.
    ///
    /// By default the solver keeps the native WebView user-agent. Overriding it
    /// can make browser fingerprint checks less coherent, so this should only
    /// be used when the caller knows the target accepts the configured profile.
    ///
    /// # Parameters
    ///
    /// - `user_agent`: User-agent override used by the WebView.
    ///
    /// # Returns
    ///
    /// The updated solver adapter.
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }

    /// Sets whether the WebView devtools should open automatically.
    ///
    /// # Parameters
    ///
    /// - `enabled`: Whether devtools should be opened for the solver WebView.
    ///
    /// # Returns
    ///
    /// The updated solver adapter.
    pub fn with_devtools(mut self, enabled: bool) -> Self {
        self.devtools = enabled;
        self
    }

    /// Sets whether the WebView closes automatically after `cf_clearance`.
    ///
    /// # Parameters
    ///
    /// - `enabled`: Whether the solver should close the WebView after the
    ///   stabilization delay.
    ///
    /// # Returns
    ///
    /// The updated solver adapter.
    pub fn with_auto_close_after_clearance(mut self, enabled: bool) -> Self {
        self.auto_close_after_clearance = enabled;
        self
    }

    /// Sets the delay after `cf_clearance` appears before auto-close.
    ///
    /// # Parameters
    ///
    /// - `duration`: Delay used before closing the WebView automatically.
    ///
    /// # Returns
    ///
    /// The updated solver adapter.
    pub fn with_clearance_stabilization(mut self, duration: Duration) -> Self {
        self.clearance_stabilization = duration;
        self
    }

    /// Sets whether the WebView should use a non-persistent data store.
    ///
    /// The solver enables this by default so the verification window does not
    /// depend on cookies persisted by the platform WebView. Cloudflare can
    /// still reuse server-side trust signals, browser attestation, or network
    /// reputation after a successful challenge.
    ///
    /// # Parameters
    ///
    /// - `enabled`: Whether the WebView should use incognito storage.
    ///
    /// # Returns
    ///
    /// The updated solver adapter.
    pub fn with_incognito(mut self, enabled: bool) -> Self {
        self.incognito = enabled;
        self
    }
}

#[async_trait]
impl HttpEngine for TauriCloudflareSolverEngine {
    /// Returns the interactive solver engine name.
    ///
    /// # Returns
    ///
    /// The static engine name.
    fn name(&self) -> &'static str {
        ENGINE_NAME
    }

    /// Sends a GET or HEAD request with the interactive WebView.
    ///
    /// # Parameters
    ///
    /// - `request`: Normalized request to solve.
    ///
    /// # Returns
    ///
    /// A normalized response carrying synthesized `Set-Cookie` headers and,
    /// for GET requests, the final page HTML collected from the WebView.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` for non-GET/HEAD requests, or
    /// `TauriCloudflareSolverFailure` when the WebView flow fails.
    async fn send(&self, request: EngineRequest) -> Result<EngineResponse, ArachneaHttpError> {
        if request.method != Method::GET && request.method != Method::HEAD {
            return Err(ArachneaHttpError::UnsupportedEngineOperation {
                engine: self.name(),
                operation: "non-GET WebView requests",
            });
        }

        let origin = refresh_cache_key(&request.url)?;
        let result = solve_cloudflare_interactive(
            &request.url,
            self.user_agent.as_deref(),
            self.timeout,
            self.poll_interval,
            self.clearance_stabilization,
            self.window_width,
            self.window_height,
            self.devtools,
            self.auto_close_after_clearance,
            self.incognito,
            request.method == Method::GET,
            self.proxy
                .as_ref()
                .map_err(|message| ArachneaHttpError::InvalidConfiguration(message.clone()))?
                .as_ref(),
        )
        .await?;
        self.store_cached_headers(origin, result.headers.clone());

        Ok(EngineResponse {
            url: request.url,
            status: StatusCode::OK,
            headers: result.headers,
            body: result.body,
        })
    }

    /// Opens a WebView and waits until the user obtains `cf_clearance`.
    ///
    /// This method runs the Wry event loop on the calling thread. For portable
    /// behavior, call it from the process main thread.
    ///
    /// # Parameters
    ///
    /// - `request`: Normalized refresh request.
    ///
    /// # Returns
    ///
    /// A normalized response carrying synthesized `Set-Cookie` headers and the
    /// WebView user-agent metadata.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` for non-GET/HEAD requests, or
    /// `TauriCloudflareSolverFailure` when the user closes the window, the
    /// timeout expires, the WebView fails, or no clearance cookie is produced.
    async fn refresh_cloudflare(
        &self,
        request: EngineRequest,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        self.refresh_cloudflare_with_cache_policy(request, true)
            .await
    }

    /// Opens a WebView and bypasses recent interactive refresh caches.
    ///
    /// # Parameters
    ///
    /// - `request`: Normalized refresh request.
    ///
    /// # Returns
    ///
    /// A normalized response carrying synthesized `Set-Cookie` headers and the
    /// WebView user-agent metadata.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` for non-GET/HEAD requests, or
    /// `TauriCloudflareSolverFailure` when the interactive flow fails.
    async fn refresh_cloudflare_fresh(
        &self,
        request: EngineRequest,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        self.refresh_cloudflare_with_cache_policy(request, false)
            .await
    }
}

impl TauriCloudflareSolverEngine {
    /// Refreshes Cloudflare cookies, optionally reusing recent WebView results.
    ///
    /// # Parameters
    ///
    /// - `request`: Normalized refresh request.
    /// - `use_recent_cache`: Whether a recent WebView result can satisfy the
    ///   refresh without opening another window.
    ///
    /// # Returns
    ///
    /// A normalized response carrying synthesized `Set-Cookie` headers and the
    /// WebView user-agent metadata.
    ///
    /// # Errors
    ///
    /// Returns unsupported operation or WebView failures.
    async fn refresh_cloudflare_with_cache_policy(
        &self,
        request: EngineRequest,
        use_recent_cache: bool,
    ) -> Result<EngineResponse, ArachneaHttpError> {
        if request.method != Method::GET && request.method != Method::HEAD {
            return Err(ArachneaHttpError::UnsupportedEngineOperation {
                engine: self.name(),
                operation: "non-GET Cloudflare refresh requests",
            });
        }

        let origin = refresh_cache_key(&request.url)?;
        if use_recent_cache {
            if let Some(headers) = self.cached_headers(&origin) {
                return Ok(EngineResponse {
                    url: request.url,
                    status: StatusCode::OK,
                    headers,
                    body: Bytes::new(),
                });
            }
        }

        let headers = solve_cloudflare_interactive(
            &request.url,
            self.user_agent.as_deref(),
            self.timeout,
            self.poll_interval,
            self.clearance_stabilization,
            self.window_width,
            self.window_height,
            self.devtools,
            self.auto_close_after_clearance,
            self.incognito,
            false,
            self.proxy
                .as_ref()
                .map_err(|message| ArachneaHttpError::InvalidConfiguration(message.clone()))?
                .as_ref(),
        )
        .await?
        .headers;
        self.store_cached_headers(origin, headers.clone());

        Ok(EngineResponse {
            url: request.url,
            status: StatusCode::OK,
            headers,
            body: Bytes::new(),
        })
    }

    /// Returns a recent refresh result for an origin.
    ///
    /// # Parameters
    ///
    /// - `origin`: Origin cache key.
    ///
    /// # Returns
    ///
    /// Cached headers when a recent interactive solve already completed.
    fn cached_headers(&self, origin: &str) -> Option<HeaderMap> {
        if let Some(headers) = self.cached_origin_headers(origin) {
            return Some(headers);
        }
        self.cached_latest_headers()
    }

    /// Stores the latest interactive refresh result for an origin.
    ///
    /// # Parameters
    ///
    /// - `origin`: Origin cache key.
    /// - `headers`: Headers synthesized from WebView cookies.
    fn store_cached_headers(&self, origin: String, headers: HeaderMap) {
        if let Ok(mut cache) = self.refresh_cache.lock() {
            cache.insert(
                origin,
                CachedRefresh {
                    headers: headers.clone(),
                    stored_at: Instant::now(),
                },
            );
        }
        if let Ok(mut latest) = self.latest_refresh.lock() {
            *latest = Some(CachedRefresh {
                headers,
                stored_at: Instant::now(),
            });
        }
    }

    /// Returns a recent refresh result for the exact origin.
    ///
    /// # Parameters
    ///
    /// - `origin`: Origin cache key.
    ///
    /// # Returns
    ///
    /// Cached headers when the exact origin has already been solved recently.
    fn cached_origin_headers(&self, origin: &str) -> Option<HeaderMap> {
        let cache = self.refresh_cache.lock().ok()?;
        let cached = cache.get(origin)?;
        (cached.stored_at.elapsed() <= DEFAULT_REFRESH_CACHE_TTL).then(|| cached.headers.clone())
    }

    /// Returns the latest recent refresh result for immediate duplicate asks.
    ///
    /// # Returns
    ///
    /// Cached headers when any origin has just completed an interactive solve.
    fn cached_latest_headers(&self) -> Option<HeaderMap> {
        let latest = self.latest_refresh.lock().ok()?;
        let cached = latest.as_ref()?;
        (cached.stored_at.elapsed() <= DEFAULT_REFRESH_CACHE_TTL).then(|| cached.headers.clone())
    }
}

/// Runs the blocking Wry event loop until a Cloudflare clearance cookie exists.
///
/// # Parameters
///
/// - `url`: Absolute URL to load in the WebView.
/// - `user_agent`: Optional user-agent override applied to the WebView.
/// - `timeout`: Maximum time to wait for manual challenge completion.
/// - `poll_interval`: Interval between cookie jar inspections.
/// - `clearance_stabilization`: Delay after `cf_clearance` appears before
///   closing the WebView.
/// - `window_width`: Initial logical window width.
/// - `window_height`: Initial logical window height.
/// - `devtools`: Whether the WebView devtools should open automatically.
/// - `auto_close_after_clearance`: Whether the WebView should close
///   automatically after the stabilization delay.
/// - `incognito`: Whether the WebView should use a non-persistent data store.
/// - `collect_page_source`: Whether the final page HTML should be collected.
///
/// # Returns
///
/// The headers and optional body extracted from the WebView.
///
/// # Errors
///
/// Returns `TauriCloudflareSolverFailure` when WebView creation fails, the
/// window is closed, the timeout expires, or `cf_clearance` is not found.
async fn solve_cloudflare_interactive(
    url: &str,
    user_agent: Option<&str>,
    timeout: Duration,
    poll_interval: Duration,
    clearance_stabilization: Duration,
    window_width: f64,
    window_height: f64,
    devtools: bool,
    auto_close_after_clearance: bool,
    incognito: bool,
    collect_page_source: bool,
    proxy: Option<&WebviewProxy>,
) -> Result<WrySolveResult, ArachneaHttpError> {
    if let Some(dispatcher) = global_main_thread_dispatcher() {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let url = url.to_string();
        let user_agent = user_agent.map(str::to_string);
        let task_user_agent = user_agent.clone();
        let task_url = url.clone();
        let task_proxy = proxy.cloned();
        dispatcher
            .dispatch_main_thread_task(Box::new(move |context| {
                let result = start_cloudflare_solve_on_main_thread(
                    context,
                    &task_url,
                    task_user_agent.as_deref(),
                    timeout,
                    poll_interval,
                    clearance_stabilization,
                    window_width,
                    window_height,
                    devtools,
                    auto_close_after_clearance,
                    incognito,
                    collect_page_source,
                    task_proxy.as_ref(),
                );
                let _ = sender.send(result);
            }))
            .map_err(|err| ArachneaHttpError::TauriCloudflareSolverFailure(err.to_string()))?;

        match receiver.await.map_err(|_| {
            ArachneaHttpError::TauriCloudflareSolverFailure(
                "main-thread Cloudflare solver task was cancelled".to_string(),
            )
        })?? {
            MainThreadSolveStart::Completed(result) => Ok(result),
            MainThreadSolveStart::TauriApp(session) => {
                solve_cloudflare_with_tauri_app_session(
                    dispatcher,
                    session,
                    &url,
                    user_agent.as_deref(),
                    timeout,
                    poll_interval,
                    clearance_stabilization,
                    auto_close_after_clearance,
                    collect_page_source,
                )
                .await
            }
        }
    } else {
        solve_cloudflare_with_wry_event_loop(
            url,
            user_agent,
            timeout,
            poll_interval,
            clearance_stabilization,
            window_width,
            window_height,
            devtools,
            auto_close_after_clearance,
            incognito,
            collect_page_source,
            proxy,
        )
    }
}

/// Builds response headers containing cookies extracted from the WebView.
///
/// # Parameters
///
/// - `cookies`: Cookies extracted from the WebView session.
/// - `user_agent`: Optional user-agent observed in the WebView.
///
/// # Returns
///
/// A header map containing one `Set-Cookie` header per cookie and solver
/// user-agent metadata.
///
/// # Errors
///
/// Returns `CookieAbsent` when `cf_clearance` is missing, or `InvalidHeader`
/// when a synthesized header cannot be represented as an HTTP header.
fn response_headers(
    cookies: &[Cookie<'static>],
    user_agent: Option<&str>,
) -> Result<HeaderMap, ArachneaHttpError> {
    if !cookies.iter().any(|cookie| cookie.name() == "cf_clearance") {
        return Err(ArachneaHttpError::CookieAbsent {
            origin: "interactive WebView session".to_string(),
            name: "cf_clearance".to_string(),
        });
    }

    let mut headers = HeaderMap::new();
    for cookie in cookies {
        headers.append(
            SET_COOKIE,
            HeaderValue::from_str(&cookie.to_string())
                .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
        );
    }
    if let Some(user_agent) = user_agent {
        insert_solver_user_agent_header(&mut headers, user_agent)?;
    }
    Ok(headers)
}

fn insert_solver_user_agent_header(
    headers: &mut HeaderMap,
    user_agent: &str,
) -> Result<(), ArachneaHttpError> {
    headers.insert(
        SOLVER_USER_AGENT_HEADER,
        HeaderValue::from_str(user_agent)
            .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
    );
    Ok(())
}

/// Builds an origin cache key for refresh debouncing.
///
/// # Parameters
///
/// - `value`: Absolute URL being refreshed.
///
/// # Returns
///
/// A normalized origin string.
///
/// # Errors
///
/// Returns `InvalidUrl` when the URL cannot be parsed or lacks a host.
fn refresh_cache_key(value: &str) -> Result<String, ArachneaHttpError> {
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
