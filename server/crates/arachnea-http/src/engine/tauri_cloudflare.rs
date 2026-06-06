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

/// Registry name for the interactive Tauri/Wry Cloudflare solver.
pub const ENGINE_NAME: &str = "tauri-cloudflare-solver";

/// Default interval used to inspect the WebView cookie jar.
const DEFAULT_COOKIE_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Default delay after `cf_clearance` appears before the WebView closes.
const DEFAULT_CLEARANCE_STABILIZATION: Duration = Duration::from_millis(500);

/// Delay before forcing a post-clearance reload when the page does not navigate by itself.
const DEFAULT_POST_CLEARANCE_RELOAD_DELAY: Duration = Duration::from_secs(10);

/// Minimum post-clearance wait used by Tauri desktop WebviewWindow sessions.
const DEFAULT_TAURI_APP_CLEARANCE_STABILIZATION: Duration = Duration::from_secs(3);

/// Minimum fallback reload delay used by Tauri desktop WebviewWindow sessions.
const DEFAULT_TAURI_APP_POST_CLEARANCE_RELOAD_DELAY: Duration = Duration::from_secs(12);

/// Default interactive solver window width.
const DEFAULT_WINDOW_WIDTH: f64 = 1_280.0;

/// Default interactive solver window height.
const DEFAULT_WINDOW_HEIGHT: f64 = 900.0;

/// Time window where a repeated refresh reuses the last interactive result.
const DEFAULT_REFRESH_CACHE_TTL: Duration = Duration::from_secs(30);

/// Maximum time to keep pumping the solver loop after dropping the window.
const DEFAULT_WINDOW_DESTROY_GRACE: Duration = Duration::from_secs(1);

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

struct TauriAppSolveSession {
    app_handle: tauri::AppHandle<tauri::Wry>,
    window: tauri::WebviewWindow<tauri::Wry>,
    current_url: Arc<Mutex<String>>,
    page_load_finished_count: Arc<AtomicU64>,
    page_state_event: String,
    page_state_event_id: tauri::EventId,
    page_state_result: Arc<Mutex<Option<Result<TauriAppPageState, String>>>>,
    closed: Arc<AtomicBool>,
}

#[derive(Deserialize)]
struct TauriAppPageState {
    #[serde(default)]
    html: String,
    #[serde(default, rename = "userAgent")]
    user_agent: String,
}

enum MainThreadSolveStart {
    Completed(WrySolveResult),
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
    /// - `config`: Client configuration used for timeout and user-agent.
    ///
    /// # Returns
    ///
    /// A configured Tauri/Wry solver adapter.
    pub fn new(config: &ArachneaHttpConfig) -> Self {
        Self {
            timeout: config.request_timeout,
            user_agent: None,
            poll_interval: DEFAULT_COOKIE_POLL_INTERVAL,
            clearance_stabilization: DEFAULT_CLEARANCE_STABILIZATION,
            window_width: DEFAULT_WINDOW_WIDTH,
            window_height: DEFAULT_WINDOW_HEIGHT,
            devtools: false,
            auto_close_after_clearance: true,
            incognito: true,
            refresh_cache: Arc::new(Mutex::new(HashMap::new())),
            latest_refresh: Arc::new(Mutex::new(None)),
        }
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
            user_agent: Some(user_agent),
            poll_interval,
            clearance_stabilization: DEFAULT_CLEARANCE_STABILIZATION,
            window_width: DEFAULT_WINDOW_WIDTH,
            window_height: DEFAULT_WINDOW_HEIGHT,
            devtools: false,
            auto_close_after_clearance: true,
            incognito: true,
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
) -> Result<WrySolveResult, ArachneaHttpError> {
    if let Some(dispatcher) = global_main_thread_dispatcher() {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let url = url.to_string();
        let user_agent = user_agent.map(str::to_string);
        let task_user_agent = user_agent.clone();
        let task_url = url.clone();
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
        )
    }
}

fn start_cloudflare_solve_on_main_thread(
    context: &mut MainThreadContext,
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
) -> Result<MainThreadSolveStart, ArachneaHttpError> {
    if let Some(app_handle) = context.resource::<tauri::AppHandle<tauri::Wry>>().cloned() {
        return create_tauri_app_solve_session(
            app_handle,
            url,
            user_agent,
            window_width,
            window_height,
            devtools,
            incognito,
        )
        .map(MainThreadSolveStart::TauriApp);
    }

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
    )
    .map(MainThreadSolveStart::Completed)
}

fn create_tauri_app_solve_session(
    app_handle: tauri::AppHandle<tauri::Wry>,
    url: &str,
    user_agent: Option<&str>,
    window_width: f64,
    window_height: f64,
    devtools: bool,
    incognito: bool,
) -> Result<TauriAppSolveSession, ArachneaHttpError> {
    let parsed_url =
        Url::parse(url).map_err(|err| ArachneaHttpError::InvalidUrl(err.to_string()))?;
    let current_url = Arc::new(Mutex::new(url.to_string()));
    let navigation_url = Arc::clone(&current_url);
    let page_load_url = Arc::clone(&current_url);
    let page_load_finished_count = Arc::new(AtomicU64::new(0));
    let page_load_finished_counter = Arc::clone(&page_load_finished_count);
    let window_id = TAURI_SOLVER_WINDOW_ID.fetch_add(1, Ordering::Relaxed);
    let label = format!("arachnea-cloudflare-solver-{window_id}");
    let page_state_event = format!("arachnea-cloudflare-solver-page-state-{window_id}");
    let page_state_result: Arc<Mutex<Option<Result<TauriAppPageState, String>>>> =
        Arc::new(Mutex::new(None));
    let page_state_listener_result = Arc::clone(&page_state_result);
    let page_state_event_id = {
        use tauri::Listener;

        app_handle.listen(page_state_event.clone(), move |event| {
            let result = serde_json::from_str::<TauriAppPageState>(event.payload())
                .map_err(|err| err.to_string());
            if let Ok(mut value) = page_state_listener_result.lock() {
                *value = Some(result);
            }
        })
    };
    let mut builder = tauri::WebviewWindowBuilder::new(
        &app_handle,
        label,
        tauri::WebviewUrl::External(parsed_url),
    )
    .title("Arachnea Cloudflare verification")
    .inner_size(window_width, window_height)
    .min_inner_size(900.0, 650.0)
    .incognito(incognito)
    .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
    .on_navigation(move |url| {
        if let Ok(mut current_url) = navigation_url.lock() {
            *current_url = url.to_string();
        }
        true
    })
    .on_page_load(move |_, payload| {
        if let Ok(mut current_url) = page_load_url.lock() {
            *current_url = payload.url().to_string();
        }
        if payload.event() == tauri::webview::PageLoadEvent::Finished {
            page_load_finished_counter.fetch_add(1, Ordering::SeqCst);
        }
    });
    if let Some(user_agent) = user_agent {
        builder = builder.user_agent(user_agent);
    }

    let window = builder
        .build()
        .map_err(|err| ArachneaHttpError::TauriCloudflareSolverFailure(err.to_string()))?;
    let closed = Arc::new(AtomicBool::new(false));
    let closed_for_event = Arc::clone(&closed);
    window.on_window_event(move |event| {
        if matches!(
            event,
            tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
        ) {
            closed_for_event.store(true, Ordering::SeqCst);
        }
    });

    #[cfg(any(debug_assertions, feature = "tauri-cloudflare-solver-devtools"))]
    if devtools {
        window.open_devtools();
    }
    #[cfg(not(any(debug_assertions, feature = "tauri-cloudflare-solver-devtools")))]
    let _ = devtools;

    Ok(TauriAppSolveSession {
        app_handle,
        window,
        current_url,
        page_load_finished_count,
        page_state_event,
        page_state_event_id,
        page_state_result,
        closed,
    })
}

async fn solve_cloudflare_with_tauri_app_session(
    dispatcher: Arc<dyn arachnea_core::controler::MainThreadDispatcher>,
    session: TauriAppSolveSession,
    url: &str,
    user_agent: Option<&str>,
    timeout: Duration,
    poll_interval: Duration,
    clearance_stabilization: Duration,
    auto_close_after_clearance: bool,
    collect_page_source: bool,
) -> Result<WrySolveResult, ArachneaHttpError> {
    let started_at = Instant::now();
    let mut clearance_seen_at: Option<Instant> = None;
    let mut clearance_headers: Option<HeaderMap> = None;
    let mut post_clearance_load_baseline: Option<u64> = None;
    let mut post_clearance_reload_requested = false;
    let mut post_clearance_finished_at: Option<Instant> = None;
    let tauri_clearance_stabilization =
        clearance_stabilization.max(DEFAULT_TAURI_APP_CLEARANCE_STABILIZATION);
    let tauri_post_clearance_reload_delay =
        DEFAULT_POST_CLEARANCE_RELOAD_DELAY.max(DEFAULT_TAURI_APP_POST_CLEARANCE_RELOAD_DELAY);
    let mut page_body = Bytes::new();

    loop {
        tokio::time::sleep(poll_interval).await;

        if session.closed.load(Ordering::SeqCst) {
            let result = clearance_headers.take().map(Ok).unwrap_or_else(|| {
                Err(ArachneaHttpError::TauriCloudflareSolverFailure(
                    "Cloudflare verification window was closed before cf_clearance was available"
                        .to_string(),
                ))
            })?;
            close_tauri_solver_window(&dispatcher, &session).await;
            return Ok(WrySolveResult {
                headers: result,
                body: page_body,
            });
        }

        if started_at.elapsed() >= timeout {
            let result = clearance_headers.take().map(Ok).unwrap_or_else(|| {
                Err(ArachneaHttpError::TauriCloudflareSolverFailure(
                    "Cloudflare verification timed out before cf_clearance was available"
                        .to_string(),
                ))
            })?;
            close_tauri_solver_window(&dispatcher, &session).await;
            return Ok(WrySolveResult {
                headers: result,
                body: page_body,
            });
        }

        match poll_tauri_solver_cookies(
            &dispatcher,
            session.window.clone(),
            Arc::clone(&session.current_url),
            url.to_string(),
            user_agent.map(str::to_string),
        )
        .await
        {
            Ok(headers) => {
                let seen_at = *clearance_seen_at.get_or_insert_with(Instant::now);
                clearance_headers = Some(headers);

                let page_load_finished_count =
                    session.page_load_finished_count.load(Ordering::SeqCst);
                let load_baseline =
                    *post_clearance_load_baseline.get_or_insert(page_load_finished_count);
                if page_load_finished_count > load_baseline {
                    post_clearance_finished_at.get_or_insert_with(Instant::now);
                } else if !post_clearance_reload_requested
                    && seen_at.elapsed() >= tauri_post_clearance_reload_delay
                {
                    reload_tauri_solver_window(&dispatcher, session.window.clone()).await?;
                    post_clearance_load_baseline =
                        Some(session.page_load_finished_count.load(Ordering::SeqCst));
                    post_clearance_reload_requested = true;
                    continue;
                }

                let page_stable_after_clearance = post_clearance_finished_at
                    .map(|finished_at| finished_at.elapsed() >= tauri_clearance_stabilization)
                    .unwrap_or(false);
                if auto_close_after_clearance && page_stable_after_clearance {
                    let mut headers = clearance_headers.take().ok_or_else(|| {
                        ArachneaHttpError::TauriCloudflareSolverFailure(
                            "Cloudflare verification ended before headers were available"
                                .to_string(),
                        )
                    })?;
                    if let Some(page_state) =
                        capture_tauri_solver_page_state(&dispatcher, &session, collect_page_source)
                            .await?
                    {
                        if !page_state.user_agent.trim().is_empty() {
                            insert_solver_user_agent_header(&mut headers, &page_state.user_agent)?;
                        }
                        if collect_page_source && !page_state.html.is_empty() {
                            page_body = Bytes::from(page_state.html);
                        }
                    }
                    close_tauri_solver_window(&dispatcher, &session).await;
                    return Ok(WrySolveResult {
                        headers,
                        body: page_body,
                    });
                }
            }
            Err(ArachneaHttpError::CookieAbsent { .. }) => {
                clearance_seen_at = None;
                clearance_headers = None;
                post_clearance_load_baseline = None;
                post_clearance_reload_requested = false;
                post_clearance_finished_at = None;
            }
            Err(err) => {
                close_tauri_solver_window(&dispatcher, &session).await;
                return Err(err);
            }
        }
    }
}

async fn poll_tauri_solver_cookies(
    dispatcher: &Arc<dyn arachnea_core::controler::MainThreadDispatcher>,
    window: tauri::WebviewWindow<tauri::Wry>,
    current_url: Arc<Mutex<String>>,
    fallback_url: String,
    user_agent: Option<String>,
) -> Result<HeaderMap, ArachneaHttpError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    dispatcher
        .dispatch_main_thread_task(Box::new(move |_| {
            let cookie_url = current_url
                .lock()
                .map(|value| value.clone())
                .unwrap_or(fallback_url);
            let result = Url::parse(&cookie_url)
                .map_err(|err| ArachneaHttpError::InvalidUrl(err.to_string()))
                .and_then(|url| {
                    window
                        .cookies_for_url(url)
                        .or_else(|_| window.cookies())
                        .map_err(|err| {
                            ArachneaHttpError::TauriCloudflareSolverFailure(err.to_string())
                        })
                })
                .and_then(|cookies| response_headers(&cookies, user_agent.as_deref()));
            let _ = sender.send(result);
        }))
        .map_err(|err| ArachneaHttpError::TauriCloudflareSolverFailure(err.to_string()))?;
    receiver.await.map_err(|_| {
        ArachneaHttpError::TauriCloudflareSolverFailure(
            "main-thread Tauri cookie poll was cancelled".to_string(),
        )
    })?
}

async fn close_tauri_solver_window(
    dispatcher: &Arc<dyn arachnea_core::controler::MainThreadDispatcher>,
    session: &TauriAppSolveSession,
) {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let app_handle = session.app_handle.clone();
    let page_state_event_id = session.page_state_event_id;
    let window = session.window.clone();
    let _ = dispatcher.dispatch_main_thread_task(Box::new(move |_| {
        use tauri::Listener;

        app_handle.unlisten(page_state_event_id);
        let _ = window.close();
        let _ = sender.send(());
    }));
    let _ = receiver.await;
}

async fn capture_tauri_solver_page_state(
    dispatcher: &Arc<dyn arachnea_core::controler::MainThreadDispatcher>,
    session: &TauriAppSolveSession,
    collect_page_source: bool,
) -> Result<Option<TauriAppPageState>, ArachneaHttpError> {
    if let Ok(mut result) = session.page_state_result.lock() {
        *result = None;
    }

    let event_name = serde_json::to_string(&session.page_state_event)
        .map_err(|err| ArachneaHttpError::TauriCloudflareSolverFailure(err.to_string()))?;
    let html_expression = if collect_page_source {
        "document.documentElement ? document.documentElement.outerHTML : ''"
    } else {
        "''"
    };
    let script = format!(
        r#"(function(){{
  const payload = {{
    html: {html_expression},
    userAgent: navigator.userAgent || ''
  }};
  const eventName = {event_name};
  if (window.__TAURI__ && window.__TAURI__.event && window.__TAURI__.event.emit) {{
    window.__TAURI__.event.emit(eventName, payload);
  }} else if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {{
    window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {{ event: eventName, payload }});
  }}
}})();"#
    );

    let (sender, receiver) = tokio::sync::oneshot::channel();
    let window = session.window.clone();
    dispatcher
        .dispatch_main_thread_task(Box::new(move |_| {
            let result = window
                .eval(script)
                .map_err(|err| ArachneaHttpError::TauriCloudflareSolverFailure(err.to_string()));
            let _ = sender.send(result);
        }))
        .map_err(|err| ArachneaHttpError::TauriCloudflareSolverFailure(err.to_string()))?;
    receiver.await.map_err(|_| {
        ArachneaHttpError::TauriCloudflareSolverFailure(
            "main-thread Tauri page-state capture was cancelled".to_string(),
        )
    })??;

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(result) = session
            .page_state_result
            .lock()
            .ok()
            .and_then(|mut result| result.take())
        {
            return result
                .map(Some)
                .map_err(ArachneaHttpError::TauriCloudflareSolverFailure);
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn reload_tauri_solver_window(
    dispatcher: &Arc<dyn arachnea_core::controler::MainThreadDispatcher>,
    window: tauri::WebviewWindow<tauri::Wry>,
) -> Result<(), ArachneaHttpError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    dispatcher
        .dispatch_main_thread_task(Box::new(move |_| {
            let result = window
                .reload()
                .map_err(|err| ArachneaHttpError::TauriCloudflareSolverFailure(err.to_string()));
            let _ = sender.send(result);
        }))
        .map_err(|err| ArachneaHttpError::TauriCloudflareSolverFailure(err.to_string()))?;
    receiver.await.map_err(|_| {
        ArachneaHttpError::TauriCloudflareSolverFailure(
            "main-thread Tauri reload was cancelled".to_string(),
        )
    })?
}

fn solve_cloudflare_with_wry_event_loop(
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
) -> Result<WrySolveResult, ArachneaHttpError> {
    let mut event_loop = solver_event_loop();
    let window = WindowBuilder::new()
        .with_title("Arachnea Cloudflare verification")
        .with_inner_size(LogicalSize::new(window_width, window_height))
        .with_min_inner_size(LogicalSize::new(900.0, 650.0))
        .build(&event_loop)
        .map_err(|err| ArachneaHttpError::TauriCloudflareSolverFailure(err.to_string()))?;

    let current_url = Arc::new(Mutex::new(url.to_string()));
    let navigation_url = current_url.clone();
    let mut builder = WebViewBuilder::new()
        .with_incognito(incognito)
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_navigation_handler(move |url| {
            if let Ok(mut current_url) = navigation_url.lock() {
                *current_url = url;
            }
            true
        });
    if let Some(user_agent) = user_agent {
        builder = builder.with_user_agent(user_agent.to_string());
    }

    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    ))]
    let webview = builder
        .build(&window)
        .map_err(|err| ArachneaHttpError::TauriCloudflareSolverFailure(err.to_string()))?;

    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    )))]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;

        let vbox = window.default_vbox().ok_or_else(|| {
            ArachneaHttpError::TauriCloudflareSolverFailure(
                "GTK default container is unavailable".to_string(),
            )
        })?;
        builder
            .build_gtk(vbox)
            .map_err(|err| ArachneaHttpError::TauriCloudflareSolverFailure(err.to_string()))?
    };

    #[cfg(any(debug_assertions, feature = "tauri-cloudflare-solver-devtools"))]
    if devtools {
        webview.open_devtools();
    }
    #[cfg(not(any(debug_assertions, feature = "tauri-cloudflare-solver-devtools")))]
    let _ = devtools;

    let mut window = Some(window);
    let mut webview = Some(webview);
    let started_at = Instant::now();
    let mut next_cookie_check = Instant::now() + poll_interval;
    let mut clearance_seen_at: Option<Instant> = None;
    let mut clearance_headers: Option<HeaderMap> = None;
    let mut page_body: Option<Bytes> = None;
    let mut page_source_requested = false;
    let mut navigation_started = false;
    let page_source_result: Arc<Mutex<Option<Result<Bytes, String>>>> = Arc::new(Mutex::new(None));
    let observed_user_agent: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let mut user_agent_requested = false;
    let mut outcome: Option<Result<HeaderMap, ArachneaHttpError>> = None;
    let mut window_destroy_started_at: Option<Instant> = None;

    event_loop.run_return(|event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + poll_interval);

        if let Some(destroy_started_at) = window_destroy_started_at {
            *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(50));
            if matches!(
                event,
                Event::WindowEvent {
                    event: WindowEvent::Destroyed,
                    ..
                }
            ) || destroy_started_at.elapsed() >= DEFAULT_WINDOW_DESTROY_GRACE
            {
                *control_flow = ControlFlow::Exit;
            }
            return;
        }

        let Some(webview_ref) = webview.as_ref() else {
            *control_flow = ControlFlow::Exit;
            return;
        };

        if !navigation_started {
            navigation_started = true;
            if let Err(err) = webview_ref.load_url(url) {
                outcome = Some(Err(ArachneaHttpError::TauriCloudflareSolverFailure(
                    err.to_string(),
                )));
                start_solver_window_shutdown(
                    &mut webview,
                    &mut window,
                    &mut window_destroy_started_at,
                    control_flow,
                );
                return;
            }
        }

        if let Some(result) = page_source_result.lock().ok().and_then(|mut value| value.take()) {
            match result {
                Ok(body) => page_body = Some(body),
                Err(err) => {
                    outcome = Some(Err(ArachneaHttpError::TauriCloudflareSolverFailure(err)));
                    start_solver_window_shutdown(
                        &mut webview,
                        &mut window,
                        &mut window_destroy_started_at,
                        control_flow,
                    );
                    return;
                }
            }
        }

        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            outcome = Some(clearance_headers.take().map(Ok).unwrap_or_else(|| {
                Err(ArachneaHttpError::TauriCloudflareSolverFailure(
                    "Cloudflare verification window was closed before cf_clearance was available"
                        .to_string(),
                ))
            }));
            start_solver_window_shutdown(
                &mut webview,
                &mut window,
                &mut window_destroy_started_at,
                control_flow,
            );
            return;
        }

        if started_at.elapsed() >= timeout {
            outcome = Some(clearance_headers.take().map(Ok).unwrap_or_else(|| {
                Err(ArachneaHttpError::TauriCloudflareSolverFailure(
                    "Cloudflare verification timed out before cf_clearance was available"
                        .to_string(),
                ))
            }));
            start_solver_window_shutdown(
                &mut webview,
                &mut window,
                &mut window_destroy_started_at,
                control_flow,
            );
            return;
        }

        if !matches!(event, Event::MainEventsCleared) || Instant::now() < next_cookie_check {
            return;
        }

        if !user_agent_requested {
            user_agent_requested = true;
            let observed_user_agent = observed_user_agent.clone();
            let _ = webview_ref.evaluate_script_with_callback("navigator.userAgent", move |value| {
                let user_agent = serde_json::from_str::<String>(&value).unwrap_or(value);
                if let Ok(mut observed) = observed_user_agent.lock() {
                    *observed = Some(user_agent);
                }
            });
        }

        next_cookie_check = Instant::now() + poll_interval;
        let cookie_url = current_url
            .lock()
            .map(|value| value.clone())
            .unwrap_or_else(|_| url.to_string());
        match webview_ref
            .cookies_for_url(&cookie_url)
            .or_else(|_| webview_ref.cookies())
        {
            Ok(cookies) => {
                let solver_user_agent = observed_user_agent
                    .lock()
                    .ok()
                    .and_then(|value| value.clone())
                    .or_else(|| user_agent.map(str::to_string));
                match response_headers(&cookies, solver_user_agent.as_deref()) {
                Ok(headers) => {
                    let seen_at = *clearance_seen_at.get_or_insert_with(Instant::now);
                    clearance_headers = Some(headers);
                    if collect_page_source
                        && seen_at.elapsed() >= clearance_stabilization
                        && !page_source_requested
                    {
                        page_source_requested = true;
                        let page_source_result = page_source_result.clone();
                        if let Err(err) = webview_ref.evaluate_script_with_callback(
                            "(function(){return document.documentElement ? document.documentElement.outerHTML : '';})()",
                            move |value| {
                                let body = serde_json::from_str::<String>(&value)
                                    .unwrap_or(value);
                                if let Ok(mut result) = page_source_result.lock() {
                                    *result = Some(Ok(Bytes::from(body)));
                                }
                            },
                        ) {
                            outcome = Some(Err(ArachneaHttpError::TauriCloudflareSolverFailure(
                                err.to_string(),
                            )));
                            start_solver_window_shutdown(
                                &mut webview,
                                &mut window,
                                &mut window_destroy_started_at,
                                control_flow,
                            );
                            return;
                        }
                    }
                    let body_ready = !collect_page_source || page_body.is_some();
                    if auto_close_after_clearance
                        && body_ready
                        && seen_at.elapsed() >= clearance_stabilization
                    {
                        outcome = clearance_headers.take().map(Ok);
                        start_solver_window_shutdown(
                            &mut webview,
                            &mut window,
                            &mut window_destroy_started_at,
                            control_flow,
                        );
                    }
                }
                Err(ArachneaHttpError::CookieAbsent { .. }) => {
                    clearance_seen_at = None;
                    clearance_headers = None;
                }
                Err(err) => {
                    outcome = Some(Err(err));
                    start_solver_window_shutdown(
                        &mut webview,
                        &mut window,
                        &mut window_destroy_started_at,
                        control_flow,
                    );
                }
                }
            }
            Err(err) => {
                outcome = Some(Err(ArachneaHttpError::TauriCloudflareSolverFailure(
                    err.to_string(),
                )));
                start_solver_window_shutdown(
                    &mut webview,
                    &mut window,
                    &mut window_destroy_started_at,
                    control_flow,
                );
            }
        }
    });

    drop(webview);
    drop(window);

    outcome
        .unwrap_or_else(|| {
            Err(ArachneaHttpError::TauriCloudflareSolverFailure(
                "Cloudflare verification ended before cf_clearance was available".to_string(),
            ))
        })
        .map(|headers| WrySolveResult {
            headers,
            body: page_body.unwrap_or_default(),
        })
}

/// Starts an orderly teardown for the solver WebView and its native window.
///
/// Tao destroys Windows windows by posting a message that is processed by the
/// event loop, so the solver must keep pumping briefly after dropping the
/// window instead of exiting the loop immediately.
fn start_solver_window_shutdown(
    webview: &mut Option<wry::WebView>,
    window: &mut Option<tao::window::Window>,
    window_destroy_started_at: &mut Option<Instant>,
    control_flow: &mut ControlFlow,
) {
    if window_destroy_started_at.is_some() {
        return;
    }
    if let Some(window) = window.as_ref() {
        window.set_visible(false);
    }
    drop(webview.take());
    drop(window.take());
    *window_destroy_started_at = Some(Instant::now());
    *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(50));
}

/// Builds the Tao event loop used by the interactive solver.
fn solver_event_loop() -> EventLoop<()> {
    let mut builder = EventLoopBuilder::new();
    #[cfg(target_os = "windows")]
    builder.with_any_thread(true);
    builder.build()
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
