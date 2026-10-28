//! Standalone Wry event loop and solver window shutdown.

use super::*;

/// Runs a standalone Wry solver window on a Tao event loop.
///
/// The window opens `incognito` when requested or whenever a proxy is used. It
/// denies popup windows and tracks the latest navigation URL. The function
/// blocks until clearance, timeout, or window close.
///
/// # Arguments
///
/// - `url`: Cloudflare-protected URL to load.
/// - `user_agent`: Optional browser user-agent override.
/// - `timeout`: Maximum solve duration.
/// - `poll_interval`: Cookie polling interval.
/// - `clearance_stabilization`: Delay required after clearance before closing.
/// - `window_width`: Initial window width.
/// - `window_height`: Initial window height.
/// - `devtools`: Whether to open developer tools when the build permits it.
/// - `auto_close_after_clearance`: Whether to close once clearance is stable.
/// - `incognito`: Whether to use an incognito webview.
/// - `collect_page_source`: Whether to capture page HTML after solving.
/// - `proxy`: Optional native webview proxy configuration.
///
/// # Returns
///
/// Solver headers and optional page HTML.
///
/// # Errors
///
/// Returns webview, event-loop, proxy, or URL failures. Timeout before clearance
/// is an error.
pub(super) fn solve_cloudflare_with_wry_event_loop(
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
        .with_incognito(incognito || proxy.is_some())
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

    if let Some(proxy) = proxy {
        #[cfg(target_os = "macos")]
        {
            use wry::WebViewBuilderExtMacos;
            builder = builder.with_webview_configuration(proxy.macos_configuration()?);
        }
        #[cfg(not(target_os = "macos"))]
        {
            builder = builder.with_proxy_config(proxy.wry_config());
        }
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

    if let Some(proxy) = proxy {
        proxy.configure_wry(&webview)?;
    }

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
            *control_flow = ControlFlow::WaitUntil(Instant::now() + crate::HTTP_TAURI_EVENT_LOOP_POLL_INTERVAL);
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
///
/// This is a no-op when shutdown has already started.
///
/// # Arguments
///
/// - `webview`: Webview slot to drop.
/// - `window`: Window slot to hide and drop.
/// - `window_destroy_started_at`: Shutdown marker; set on the first call.
/// - `control_flow`: Event-loop control flow, switched to a short poll so pending
///   destroy messages can be processed.
pub(super) fn start_solver_window_shutdown(
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
    *control_flow =
        ControlFlow::WaitUntil(Instant::now() + crate::HTTP_TAURI_EVENT_LOOP_POLL_INTERVAL);
}

/// Builds the Tao event loop used by the interactive solver.
pub(super) fn solver_event_loop() -> EventLoop<()> {
    let mut builder = EventLoopBuilder::new();
    #[cfg(target_os = "windows")]
    builder.with_any_thread(true);
    builder.build()
}
