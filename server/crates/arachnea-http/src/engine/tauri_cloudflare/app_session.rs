//! Application-owned Tauri solver windows and asynchronous observation.

use super::*;

/// Starts a solve using the application-owned Tauri window when available,
/// otherwise the standalone Wry event-loop path.
///
/// # Arguments
///
/// - `context`: Main-thread context that may contain a Tauri `AppHandle`.
/// - `url`: Cloudflare-protected URL to load.
/// - `user_agent`: Optional browser user-agent override.
/// - `options`: Solve timing, window settings and post-clearance collection policy.
/// - `proxy`: Optional native webview proxy configuration.
///
/// # Returns
///
/// Either a live Tauri app session to observe asynchronously, or a completed Wry
/// solve result.
///
/// # Errors
///
/// Returns Tauri, Wry, proxy, or URL failures from the selected path.
pub(super) fn start_cloudflare_solve_on_main_thread(
    context: &mut MainThreadContext,
    url: &str,
    user_agent: Option<&str>,
    options: CloudflareSolveOptions,
    proxy: Option<&WebviewProxy>,
) -> Result<MainThreadSolveStart, ArachneaHttpError> {
    if let Some(app_handle) = context.resource::<tauri::AppHandle<tauri::Wry>>().cloned() {
        return create_tauri_app_solve_session(app_handle, url, user_agent, options, proxy)
            .map(|session| MainThreadSolveStart::TauriApp(Box::new(session)));
    }

    solve_cloudflare_with_wry_event_loop(url, user_agent, options, proxy)
        .map(MainThreadSolveStart::Completed)
}

/// Creates an application-owned Tauri solve window and registers its listeners.
///
/// When a proxy is configured, the window initially opens `about:blank` and the
/// caller navigates after native proxy setup completes. Incognito is forced when
/// a proxy is used.
///
/// # Arguments
///
/// - `app_handle`: Owning Tauri application handle.
/// - `url`: Cloudflare-protected URL to load.
/// - `user_agent`: Optional browser user-agent override.
/// - `options`: Window dimensions, developer tools and incognito policy.
/// - `proxy`: Optional native webview proxy configuration.
///
/// # Returns
///
/// A session containing the window, current URL, page-load state, and optional
/// proxy readiness receiver.
///
/// # Errors
///
/// Returns `InvalidUrl`, `TauriCloudflareSolverFailure`, or proxy configuration
/// errors.
pub(super) fn create_tauri_app_solve_session(
    app_handle: tauri::AppHandle<tauri::Wry>,
    url: &str,
    user_agent: Option<&str>,
    options: CloudflareSolveOptions,
    proxy: Option<&WebviewProxy>,
) -> Result<TauriAppSolveSession, ArachneaHttpError> {
    let CloudflareSolveOptions {
        window_width,
        window_height,
        devtools,
        incognito,
        ..
    } = options;
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
    let mut builder = tauri::WebviewWindowBuilder::new(
        &app_handle,
        label,
        tauri::WebviewUrl::External(if proxy.is_some() {
            Url::parse("about:blank").unwrap()
        } else {
            parsed_url.clone()
        }),
    )
    .title("Arachnea Cloudflare verification")
    .inner_size(window_width, window_height)
    .min_inner_size(900.0, 650.0)
    .incognito(incognito || proxy.is_some())
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

    if let Some(proxy) = proxy {
        #[cfg(target_os = "macos")]
        {
            builder = builder.with_webview_configuration(proxy.macos_configuration()?);
        }
        #[cfg(not(target_os = "macos"))]
        {
            builder = builder.proxy_url(proxy.endpoint());
        }
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

    let proxy_ready = if let Some(proxy) = proxy.cloned() {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        if window
            .with_webview(move |webview| {
                let result = proxy
                    .configure_tauri(webview)
                    .map_err(|error| error.to_string());
                let _ = sender.send(result);
            })
            .is_err()
        {
            let _ = window.close();
            use tauri::Listener;
            app_handle.unlisten(page_state_event_id);
            return Err(ArachneaHttpError::TauriCloudflareSolverFailure(
                "cannot configure proxy on the WebView thread".into(),
            ));
        }
        Some(receiver)
    } else {
        None
    };

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
        proxy_ready,
    })
}

/// Observes an application-owned Tauri window until clearance and stabilization.
///
/// Waits for native proxy setup, navigates when needed, polls cookies, requests
/// one post-clearance reload when no new load is observed, and optionally
/// captures the page HTML before closing the window.
///
/// # Arguments
///
/// - `dispatcher`: Dispatcher used for all webview operations.
/// - `session`: Live application-owned solve session.
/// - `url`: Target URL.
/// - `user_agent`: Optional browser user-agent override.
/// - `options`: Solve timing, window settings and post-clearance collection policy.
///
/// # Returns
///
/// Solver headers and optional page HTML.
///
/// # Errors
///
/// Returns Tauri, navigation, proxy, URL, or cookie failures. Timeout before
/// clearance is an error.
pub(super) async fn solve_cloudflare_with_tauri_app_session(
    dispatcher: Arc<dyn arachnea_core::controler::MainThreadDispatcher>,
    mut session: TauriAppSolveSession,
    url: &str,
    user_agent: Option<&str>,
    options: CloudflareSolveOptions,
) -> Result<WrySolveResult, ArachneaHttpError> {
    let CloudflareSolveOptions {
        timeout,
        poll_interval,
        clearance_stabilization,
        auto_close_after_clearance,
        collect_page_source,
        ..
    } = options;
    let started_at = Instant::now();
    if let Some(ready) = session.proxy_ready.take() {
        let result = match tokio::time::timeout(timeout, ready).await {
            Ok(Ok(result)) => result,
            _ => Err("native proxy configuration did not complete".into()),
        };
        if let Err(error) = result {
            close_tauri_solver_window(&dispatcher, &session).await;
            return Err(ArachneaHttpError::TauriCloudflareSolverFailure(error));
        }
        let parsed_url = url::Url::parse(url)
            .map_err(|error| ArachneaHttpError::TauriCloudflareSolverFailure(error.to_string()))?;
        if session.window.navigate(parsed_url).is_err() {
            close_tauri_solver_window(&dispatcher, &session).await;
            return Err(ArachneaHttpError::TauriCloudflareSolverFailure(
                "proxy WebView navigation failed".into(),
            ));
        }
    }
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

/// Reads solver cookies on the main thread and builds response headers.
///
/// Uses the latest navigated URL, falling back to `fallback_url`, then retries
/// with all window cookies when URL-scoped cookie lookup fails.
///
/// # Arguments
///
/// - `dispatcher`: Dispatcher used to access the webview.
/// - `window`: Tauri window to inspect.
/// - `current_url`: Last observed navigation URL.
/// - `fallback_url`: URL used when `current_url` cannot be locked.
/// - `user_agent`: Optional browser user-agent to expose to the caller.
///
/// # Returns
///
/// `Set-Cookie` headers and an optional solver user-agent header.
///
/// # Errors
///
/// Returns URL, Tauri, cancellation, or header failures and `CookieAbsent` when
/// `cf_clearance` is missing.
pub(super) async fn poll_tauri_solver_cookies(
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

/// Unregisters the page-state listener and closes the solver window.
///
/// Dispatch, listener, and close failures are ignored. This waits for the
/// main-thread task to finish when it is accepted.
///
/// # Arguments
///
/// - `dispatcher`: Dispatcher used for webview operations.
/// - `session`: Session whose window and listener should be released.
pub(super) async fn close_tauri_solver_window(
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

/// Captures the current user-agent and optional page HTML through a Tauri event.
///
/// The capture waits up to [`crate::HTTP_TAURI_PAGE_STATE_TIMEOUT`] for the page
/// state event. A missing result after that deadline is not an error.
///
/// # Arguments
///
/// - `dispatcher`: Dispatcher used to evaluate the bridge script.
/// - `session`: Live solve session providing the event name and result slot.
/// - `collect_page_source`: Whether to capture HTML in addition to the user-agent.
///
/// # Returns
///
/// The captured page state, or `None` when no event arrives before the deadline.
///
/// # Errors
///
/// Returns Tauri, serialization, cancellation, or page-state event failures.
pub(super) async fn capture_tauri_solver_page_state(
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
        include_str!("../../../resources/tauri_cloudflare/capture_tauri_solver_page_state.js"),
        html_expression = html_expression,
        event_name = event_name
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

    let deadline = Instant::now() + crate::HTTP_TAURI_PAGE_STATE_TIMEOUT;
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
        tokio::time::sleep(crate::HTTP_TAURI_PAGE_STATE_POLL_INTERVAL).await;
    }
}

/// Reloads the solver window on the main thread.
///
/// # Arguments
///
/// - `dispatcher`: Dispatcher used for the webview operation.
/// - `window`: Window to reload.
///
/// # Errors
///
/// Returns Tauri or cancellation failures.
pub(super) async fn reload_tauri_solver_window(
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
