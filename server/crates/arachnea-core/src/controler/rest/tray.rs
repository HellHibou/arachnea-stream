//! Server tray integration points shared by the controller backends.
//!
//! This module defines the contract between the REST controller and a tray icon
//! implementation (configuration, handle and factory traits, plus graphical
//! environment detection), and also provides the concrete Tauri-based runtime
//! used in server mode. The concrete tray runs headless (no main window) and
//! exposes a context menu able to open the server URL, show the dedicated log
//! window and shut the server down.
//!
//! The Tauri runtime is supplied entirely by the caller through the context
//! argument of [`spawn_tauri_server_tray`](crate::controler::rest::tray::spawn_tauri_server_tray) (or through the concrete
//! [`ServerTrayIconService`](crate::controler::rest::tray::ServerTrayIconService), whose context is generated at the
//! `create_application_controler!` call site); this crate never runs
//! `tauri-build` nor embeds frontend assets, because the `tauri::Context` can
//! only be produced by the crate that owns `tauri.conf.json` and the app icon.
//! The tray icon falls back to the context's embedded default window icon. All
//! the tray logic (menu, log window, browser and shutdown wiring) lives here.

use std::sync::{Arc, Mutex};

use tauri::{
    menu::{MenuBuilder, MenuEvent, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Wry,
};

use crate::logger::LogCache;

use super::shutdown::ShutdownSignal;

/// Configuration handed to a server tray implementation when it is spawned.
#[derive(Clone)]
pub struct ServerTrayConfiguration {
    /// Public URL of the running HTTP server, opened by the browser action.
    pub server_url: String,
    /// Public URL of the administration interface, opened by the admin action.
    pub admin_url: String,
    /// Full server URL displayed in the tray menu.
    pub server_display_url: String,
    /// Network access mode displayed in the tray menu.
    pub network_mode: String,
    /// Shared log cache feeding the dedicated log window.
    pub log_cache: Arc<LogCache>,
    /// Shared shutdown signal triggered by the tray close action.
    pub shutdown: ShutdownSignal,
    /// Optional callback reloading the application configuration.
    ///
    /// Invoked on a background thread by the "Reload configuration" tray
    /// action; the returned summary is written to the application log so
    /// reload problems stay diagnosable from the log window and the admin
    /// interface.
    pub reload_configuration: Option<Arc<dyn Fn() -> String + Send + Sync>>,
    /// Optional sink receiving the live tray handle once created.
    ///
    /// The REST server supervisor uses it to update the tray information in
    /// place after a hot settings application (on macOS the tray blocks in
    /// its `run` call, so the handle is only reachable through this sink).
    pub handle_sink: Option<Arc<std::sync::OnceLock<Arc<dyn ServerTrayHandle>>>>,
}

/// In-place update of the tray information after a hot settings application.
#[derive(Clone, Debug)]
pub struct ServerTrayUpdate {
    /// Public URL of the running HTTP server, opened by the browser action.
    pub server_url: String,
    /// Public URL of the administration interface, opened by the admin action.
    pub admin_url: String,
    /// Full server URL displayed in the tray menu.
    pub server_display_url: String,
    /// Network access mode displayed in the tray menu.
    pub network_mode: String,
}

/// Handle to a live server tray, used to trigger lifecycle actions.
///
/// Returned by [`ServerTrayFactory::spawn_tray`] so the REST controller can
/// drive the tray from the server lifecycle without depending on the concrete
/// Tauri implementation.
pub trait ServerTrayHandle: Send + Sync + 'static {
    /// Requests a graceful shutdown of the server application.
    fn request_shutdown(&self);

    /// Shows the dedicated log window, creating it on first use.
    fn show_log_window(&self);

    /// Opens the default browser on the configured server URL.
    fn open_browser(&self);

    /// Opens the default browser on the configured administration URL.
    fn open_admin(&self);

    /// Reloads the application configuration on a background thread.
    ///
    /// The reload runs off the Tauri UI thread so a slow reload never blocks
    /// the tray; the outcome summary is written to the application log.
    fn reload_configuration(&self);

    /// Updates the stored URLs and information menu items in place.
    ///
    /// Called after a hot application of the server settings so the tray keeps
    /// reflecting the configuration actually active. The default
    /// implementation does nothing.
    ///
    /// # Arguments
    /// * `_update` - New server URLs and display information.
    fn update_configuration(&self, _update: &ServerTrayUpdate) {}
}

/// Factory responsible for creating the server tray icon.
///
/// Implementations hold everything that is tray-specific (Tauri context, icon,
/// menu layout) while remaining generic over the controller backend.
pub trait ServerTrayFactory: Send + Sync + 'static {
    /// Spawns a server tray and returns its handle.
    ///
    /// # Arguments
    /// * `configuration` - Configuration for the tray to create.
    ///
    /// # Returns
    /// `Some(handle)` when the tray was created successfully, `None` when the
    /// platform refuses to expose a tray icon.
    fn spawn_tray(
        &self,
        configuration: ServerTrayConfiguration,
    ) -> Option<Arc<dyn ServerTrayHandle>>;

    /// Builds and runs the tray on the calling thread, blocking until it exits.
    ///
    /// macOS requires the Tauri event loop to be created and run on the process
    /// main thread, so server mode hands the main thread to this method when a
    /// tray is shown. On platforms that allow event loops on background threads
    /// the default implementation simply delegates to
    /// [`ServerTrayFactory::spawn_tray`] and returns immediately.
    ///
    /// # Arguments
    /// * `configuration` - Configuration for the tray to create.
    ///
    /// # Returns
    /// `Some(handle)` when the tray was created successfully, `None` when the
    /// platform refuses to expose a tray icon.
    fn run_on_main_thread(
        &self,
        configuration: ServerTrayConfiguration,
    ) -> Option<Arc<dyn ServerTrayHandle>> {
        self.spawn_tray(configuration)
    }
}

/// Returns whether a graphical environment is available on this machine.
///
/// The result is used to decide whether a tray icon can be shown:
/// - Windows and macOS always have a graphical session.
/// - Other Unix systems (Linux/BSD) require either an X11 (`DISPLAY`) or a
///   Wayland (`WAYLAND_DISPLAY`) display, which is absent on headless servers.
///
/// # Returns
/// `true` when a GUI is available, `false` otherwise.
pub fn gui_available() -> bool {
    #[cfg(target_os = "windows")]
    {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        return true;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let has_display = std::env::var("DISPLAY")
            .map(|value| !value.is_empty())
            .unwrap_or(false);
        let has_wayland = std::env::var("WAYLAND_DISPLAY")
            .map(|value| !value.is_empty())
            .unwrap_or(false);
        return has_display || has_wayland;
    }
    #[allow(unreachable_code)]
    false
}

/// Custom URI scheme serving the dedicated log window page.
use crate::REST_LOG_WINDOW_SCHEME as LOG_SCHEME;

/// Identifier of the tray icon.
use crate::REST_TRAY_ID as TRAY_ID;

use crate::REST_MENU_NETWORK_INFO as MENU_NETWORK_INFO;
use crate::REST_MENU_OPEN_ADMIN as MENU_OPEN_ADMIN;
use crate::REST_MENU_OPEN_BROWSER as MENU_OPEN_BROWSER;
use crate::REST_MENU_RELOAD as MENU_RELOAD;
use crate::REST_MENU_SERVER_INFO as MENU_SERVER_INFO;
use crate::REST_MENU_SHOW_LOGS as MENU_SHOW_LOGS;
/// Menu item ids.
use crate::REST_MENU_SHUTDOWN as MENU_SHUTDOWN;

/// Log window label.
use crate::REST_LOG_WINDOW_LABEL as LOG_WINDOW_LABEL;

/// Reads the buffered log records for the dedicated log window.
///
/// # Returns
/// The most recent structured log records kept in the shared log cache.
#[tauri::command]
fn get_logs() -> Vec<crate::logger::LogRecord> {
    crate::logger::global_log_cache().snapshot()
}

/// Subscribes the calling window to live log records via a Tauri IPC channel.
///
/// Each new structured log record pushed to the shared log cache is forwarded
/// to the provided channel. Subscribers are cleared when the dedicated log
/// window is closed (see [`ServerTrayHandleImpl::show_log_window`]).
///
/// # Arguments
/// * `channel` - The IPC channel receiving the live [`crate::logger::LogRecord`]s.
#[tauri::command]
fn subscribe_logs(channel: tauri::ipc::Channel<crate::logger::LogRecord>) -> Result<(), String> {
    let cache = crate::logger::global_log_cache();
    cache.add_subscriber(move |record| {
        let _ = channel.send(record.clone());
    });
    Ok(())
}

/// Self-contained HTML page displayed by the dedicated log window.
///
/// The page reads the buffered log records once, then subscribes to live
/// records through a `tauri::ipc::Channel` passed to the `subscribe_logs`
/// command. Each record is rendered as a table row (Date / Niveau / Chemin /
/// Message); the level badge is colored via CSS, the Date and Niveau columns
/// keep a fixed width, Chemin and Message are resizable, and a message keeps
/// its line breaks (`\n`) without automatic wrapping. Multi-line messages show
/// only their first line until expanded via the inline toggle icon.
use crate::REST_LOG_WINDOW_HTML as LOG_HTML;

/// Shared handle used to drive the live tray from the REST controller.
struct ServerTrayHandleImpl {
    /// Public server URL opened by the browser action.
    server_url: Mutex<String>,
    /// Public administration URL opened by the admin action.
    admin_url: Mutex<String>,
    /// Shared signal requesting the HTTP server to shut down gracefully.
    shutdown: ShutdownSignal,
    /// The running Tauri application handle.
    app_handle: Mutex<Option<AppHandle<Wry>>>,
    /// The dedicated log window, created on first use.
    log_window: Mutex<Option<WebviewWindow>>,
    /// Application window title from the calling crate's tauri.conf.json.
    app_title: String,
    /// Optional configuration reload callback invoked on a background thread.
    reload_configuration: Option<Arc<dyn Fn() -> String + Send + Sync>>,
    /// Handles of the disabled information menu items, kept so their text can
    /// be updated in place when the server settings are applied hot.
    info_items: Mutex<Option<(tauri::menu::MenuItem<Wry>, tauri::menu::MenuItem<Wry>)>>,
}

impl ServerTrayHandleImpl {
    /// Creates a tray handle.
    ///
    /// # Arguments
    /// * `server_url` - Public URL of the running server.
    /// * `admin_url` - Public URL of the administration interface.
    /// * `shutdown` - Shared shutdown signal.
    /// * `app_title` - Application window title.
    /// * `reload_configuration` - Optional configuration reload callback.
    ///
    /// # Returns
    /// A new tray handle.
    fn new(
        server_url: String,
        admin_url: String,
        shutdown: ShutdownSignal,
        app_title: String,
        reload_configuration: Option<Arc<dyn Fn() -> String + Send + Sync>>,
    ) -> Self {
        Self {
            server_url: Mutex::new(server_url),
            admin_url: Mutex::new(admin_url),
            shutdown,
            app_handle: Mutex::new(None),
            log_window: Mutex::new(None),
            app_title,
            reload_configuration,
            info_items: Mutex::new(None),
        }
    }

    /// Stores the Tauri application handle once the app is running.
    ///
    /// # Arguments
    /// * `app_handle` - The Tauri application handle.
    fn set_app_handle(&self, app_handle: AppHandle<Wry>) {
        *self.app_handle.lock().expect("tray app handle poisoned") = Some(app_handle);
    }

    /// Stores the handles of the information menu items for in-place updates.
    ///
    /// # Arguments
    /// * `server_info` - Menu item displaying the server URL or state.
    /// * `network_info` - Menu item displaying the active network mode.
    fn set_info_items(
        &self,
        server_info: tauri::menu::MenuItem<Wry>,
        network_info: tauri::menu::MenuItem<Wry>,
    ) {
        *self.info_items.lock().expect("tray info items poisoned") =
            Some((server_info, network_info));
    }

    /// Dispatches a tray menu event to the matching action.
    ///
    /// # Arguments
    /// * `app` - The Tauri application handle.
    /// * `event` - The menu event received from the tray.
    fn on_menu_event(&self, app: &AppHandle<Wry>, event: MenuEvent) {
        match event.id().as_ref() {
            MENU_SHUTDOWN => self.request_shutdown(),
            MENU_SHOW_LOGS => self.show_log_window(),
            MENU_OPEN_BROWSER => self.open_browser(),
            MENU_OPEN_ADMIN => self.open_admin(),
            MENU_RELOAD => self.reload_configuration(),
            _ => {
                let _ = app;
            }
        }
    }
}

impl ServerTrayHandle for ServerTrayHandleImpl {
    fn request_shutdown(&self) {
        self.shutdown.request();
        if let Some(app) = self
            .app_handle
            .lock()
            .expect("tray app handle poisoned")
            .clone()
        {
            let _ = app.exit(0);
        }
    }

    fn show_log_window(&self) {
        // Show only: if the window still exists, bring it to front; otherwise
        // create it. The cached handle must not be trusted because the user may
        // have closed the window (closing never exits the application), leaving
        // a stale handle on which `show()` is a no-op. Query the live window
        // from the app instead.
        let app = match self
            .app_handle
            .lock()
            .expect("tray app handle poisoned")
            .clone()
        {
            Some(app) => app,
            None => return,
        };

        if let Some(window) = app.get_webview_window(LOG_WINDOW_LABEL) {
            let _ = window.show();
            let _ = window.set_focus();
            return;
        }

        let url: tauri::Url = format!("{LOG_SCHEME}://localhost/").parse().unwrap();
        let window =
            WebviewWindowBuilder::new(&app, LOG_WINDOW_LABEL, WebviewUrl::CustomProtocol(url))
                .title(format!("{} - Logs", self.app_title))
                .inner_size(crate::REST_LOG_WINDOW_WIDTH, crate::REST_LOG_WINDOW_HEIGHT)
                .build();
        match window {
            Ok(window) => {
                // Clear the global log subscribers when the (single) logs
                // window is closed so its live channel is released; the next
                // open re-subscribes.
                window.on_window_event(|event| {
                    if let tauri::WindowEvent::CloseRequested { .. } = event {
                        crate::logger::global_log_cache().clear_subscribers();
                    }
                });
                *self.log_window.lock().expect("tray log window poisoned") = Some(window);
            }
            Err(error) => {
                tracing::warn!(error = %error, "failed to open the dedicated log window");
            }
        }
    }

    fn open_browser(&self) {
        let server_url = self
            .server_url
            .lock()
            .expect("tray server url poisoned")
            .clone();
        let _ = tauri_plugin_opener::open_url(server_url, None::<&str>);
    }

    fn open_admin(&self) {
        let admin_url = self
            .admin_url
            .lock()
            .expect("tray admin url poisoned")
            .clone();
        let _ = tauri_plugin_opener::open_url(admin_url, None::<&str>);
    }

    fn reload_configuration(&self) {
        let Some(callback) = &self.reload_configuration else {
            tracing::warn!("No reload callback is configured for the tray action.");
            return;
        };
        let callback = Arc::clone(callback);
        std::thread::spawn(move || {
            let summary = callback();
            tracing::info!("Tray configuration reload: {summary}");
        });
    }

    fn update_configuration(&self, update: &ServerTrayUpdate) {
        *self.server_url.lock().expect("tray server url poisoned") = update.server_url.clone();
        *self.admin_url.lock().expect("tray admin url poisoned") = update.admin_url.clone();

        // Menu operations are bound to the Tauri main thread; marshal the text
        // updates there (required on macOS where the tray runs on the main
        // thread, prudent on the other platforms).
        let app = match self
            .app_handle
            .lock()
            .expect("tray app handle poisoned")
            .clone()
        {
            Some(app) => app,
            None => return,
        };
        let info_items = self
            .info_items
            .lock()
            .expect("tray info items poisoned")
            .clone();
        let Some((server_info, network_info)) = info_items else {
            return;
        };
        let server_text = format!("Server: {}", update.server_display_url);
        let network_text = format!("Network: {}", update.network_mode);
        let _ = app.run_on_main_thread(move || {
            let _ = server_info.set_text(server_text);
            let _ = network_info.set_text(network_text);
        });
    }
}

/// Scaffold shared by the threaded and the main-thread server tray run paths.
///
/// This owns everything needed to run the headless tray: the prepared
/// [`tauri::Builder`], the [`tauri::Context`] whose windows were cleared, and
/// the shared [`ServerTrayHandleImpl`] used to drive the tray afterwards.
struct TauriServerTrayScaffold {
    /// Ready-to-run Tauri builder.
    builder: tauri::Builder<tauri::Wry>,
    /// Tauri context mutated for a headless tray (no application windows).
    context: tauri::Context<tauri::Wry>,
    /// Shared handle controlling the tray from the REST controller.
    handle: Arc<ServerTrayHandleImpl>,
}

/// Builds the headless Tauri server tray scaffold shared by both run paths.
///
/// # Arguments
/// * `context` - Tauri context generated by the calling application crate.
/// * `icon` - Optional application icon; falls back to the context's default
///   window icon when `None`.
/// * `configuration` - Configuration for the tray to create.
///
/// # Returns
/// A scaffold ready to be run either on a background thread or on the calling
/// thread (macOS).
fn build_tauri_server_tray(
    context: tauri::Context<Wry>,
    icon: Option<tauri::image::Image<'static>>,
    configuration: ServerTrayConfiguration,
) -> TauriServerTrayScaffold {
    let server_display_url = configuration.server_display_url;
    let network_mode = configuration.network_mode;
    let app_title = context
        .config()
        .app
        .windows
        .first()
        .map(|w| w.title.clone())
        .unwrap_or_else(|| "Arachnéa".to_string());
    let handle = Arc::new(ServerTrayHandleImpl::new(
        configuration.server_url,
        configuration.admin_url,
        configuration.shutdown,
        app_title,
        configuration.reload_configuration,
    ));

    // Publish the live handle into the optional sink so the REST server
    // supervisor can update the tray information in place after a hot
    // settings application (on macOS the handle is only reachable through
    // this sink because `run` blocks the calling thread).
    if let Some(sink) = &configuration.handle_sink {
        let published: Arc<dyn ServerTrayHandle> = handle.clone();
        let _ = sink.set(published);
    }

    let handle_setup = Arc::clone(&handle);
    let handle_events = Arc::clone(&handle);
    let embedded_icon = icon;

    let mut context = context;
    context.config_mut().app.windows.clear();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::Builder::new().build())
        .invoke_handler(tauri::generate_handler![get_logs, subscribe_logs])
        .register_uri_scheme_protocol(LOG_SCHEME, move |_app, _request| {
            tauri::http::Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                .body(LOG_HTML.as_bytes().to_vec())
                .expect("Failed to build the log window response.")
        })
        .setup(move |app| {
            let server_info =
                MenuItemBuilder::with_id(MENU_SERVER_INFO, format!("Server: {server_display_url}"))
                    .enabled(false)
                    .build(app)?;
            let network_info =
                MenuItemBuilder::with_id(MENU_NETWORK_INFO, format!("Network: {network_mode}"))
                    .enabled(false)
                    .build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&server_info)
                .item(&network_info)
                .separator()
                .text(MENU_OPEN_BROWSER, "Open in browser")
                .text(MENU_OPEN_ADMIN, "Open administration")
                .text(MENU_RELOAD, "Reload configuration")
                .text(MENU_SHOW_LOGS, "Show log")
                .text(MENU_SHUTDOWN, "Shutdown server")
                .build()?;

            let mut tray = TrayIconBuilder::with_id(TRAY_ID)
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(move |app, event| handle_events.on_menu_event(app, event));
            if let Some(icon) = app
                .default_window_icon()
                .cloned()
                .or_else(|| embedded_icon.clone())
            {
                tray = tray.icon(icon);
            }
            tray.build(app)?;

            handle_setup.set_app_handle(app.handle().clone());
            handle_setup.set_info_items(server_info, network_info);
            Ok(())
        });

    TauriServerTrayScaffold {
        builder,
        context,
        handle,
    }
}

/// Builds and runs a headless Tauri server tray application from a scaffold.
///
/// # Arguments
/// * `scaffold` - The prepared tray scaffold to run, blocking until the tray
///   application exits.
fn run_tauri_server_tray_app(scaffold: TauriServerTrayScaffold) {
    let shutdown = scaffold.handle.shutdown.clone();
    match scaffold.builder.build(scaffold.context) {
        Ok(app) => {
            app.run(move |_handle, event| {
                // Closing the dedicated log window must not exit the tray
                // application. An explicit shutdown first requests server
                // shutdown, then exits the application event loop.
                if let tauri::RunEvent::ExitRequested { api, .. } = event {
                    if !shutdown.is_requested() {
                        api.prevent_exit();
                    }
                }
            });
        }
        Err(error) => {
            tracing::error!(error = %error, "failed to start the server tray");
        }
    }
}

/// Spawns the Tauri-based server tray into the given runtime context.
///
/// This builds a headless Tauri application bound to the provided `context`
/// (which must be generated by the caller's crate, since only it runs
/// `tauri-build` and owns `tauri.conf.json`), installs a tray icon with a
/// context menu, and runs its event loop on a dedicated thread so it does not
/// block the REST controller main loop.
///
/// On macOS the Tauri event loop must run on the main thread, so this threaded
/// entry is not used in server mode (which routes
/// [`run_tauri_server_tray_on_main_thread`] instead); when invoked directly on
/// macOS it falls back to that main-thread runner, so callers should only call
/// it from the main thread there.
///
/// # Arguments
/// * `context` - Tauri context generated by the calling application crate.
/// * `icon` - Optional application icon; falls back to the context's default
///   window icon when `None`.
/// * `configuration` - Configuration for the tray to create.
///
/// # Returns
/// `Some(handle)` when the tray was created successfully, `None` when the
/// platform refuses to expose a tray icon.
pub fn spawn_tauri_server_tray(
    context: tauri::Context<Wry>,
    icon: Option<tauri::image::Image<'static>>,
    configuration: ServerTrayConfiguration,
) -> Option<Arc<dyn ServerTrayHandle>> {
    #[cfg(target_os = "macos")]
    {
        // macOS requires the event loop on the (process) main thread, which a
        // background thread cannot provide. Route to the blocking runner.
        return run_tauri_server_tray_on_main_thread(context, icon, configuration);
    }

    #[cfg(not(target_os = "macos"))]
    {
        let scaffold = build_tauri_server_tray(context, icon, configuration);
        let handle: Arc<dyn ServerTrayHandle> = scaffold.handle.clone();

        // The tray runs on a dedicated thread so it does not block the REST
        // controller main loop. On Windows and Linux the underlying event loop
        // must be explicitly allowed to run outside the main thread.
        #[cfg(any(windows, target_os = "linux"))]
        let scaffold = TauriServerTrayScaffold {
            builder: scaffold.builder.any_thread(),
            context: scaffold.context,
            handle: scaffold.handle,
        };

        std::thread::spawn(move || run_tauri_server_tray_app(scaffold));

        Some(handle)
    }
}

/// Runs the Tauri-based server tray on the calling thread until it exits.
///
/// macOS requires the Tauri (`tao`) event loop to be created and run on the
/// process main thread; it cannot be created on a spawned background thread.
/// Server mode therefore hands the process main thread to this function (while
/// the HTTP server and the logical main-thread loop run on their own threads)
/// and blocks here until the tray application exits.
///
/// This function must be invoked from the main thread.
///
/// # Arguments
/// * `context` - Tauri context generated by the calling application crate.
/// * `icon` - Optional application icon; falls back to the context's default
///   window icon when `None`.
/// * `configuration` - Configuration for the tray to create.
///
/// # Returns
/// `Some(handle)` when the tray was created successfully, `None` when the
/// platform refuses to expose a tray icon.
#[cfg(target_os = "macos")]
pub fn run_tauri_server_tray_on_main_thread(
    context: tauri::Context<Wry>,
    icon: Option<tauri::image::Image<'static>>,
    configuration: ServerTrayConfiguration,
) -> Option<Arc<dyn ServerTrayHandle>> {
    let scaffold = build_tauri_server_tray(context, icon, configuration);
    let handle: Arc<dyn ServerTrayHandle> = scaffold.handle.clone();
    run_tauri_server_tray_app(scaffold);
    Some(handle)
}

/// Concrete Tauri-based server tray factory supplied by `arachnea-core`.
///
/// The application crate generates the `tauri::Context` (only it can, because it
/// runs `tauri-build` and owns `tauri.conf.json`) at the
/// `create_application_controler!` call site. It is stored here so the REST
/// controller can spawn the tray later, without depending on the application
/// crate for the runtime wiring: on a background thread on Windows/Linux and on
/// the main thread on macOS (which requires the Tauri event loop there). No
/// explicit icon is supplied: the tray falls back to the context's embedded
/// default window icon (derived from `bundle.icon` in `tauri.conf.json`).
pub struct ServerTrayIconService {
    /// Tauri context generated by the calling application crate.
    context: Mutex<Option<tauri::Context<Wry>>>,
}

impl ServerTrayIconService {
    /// Creates a server tray factory from a generated Tauri context.
    ///
    /// # Arguments
    /// * `context` - Tauri context generated by the application crate.
    ///
    /// # Returns
    /// A new server tray factory.
    pub fn new(context: tauri::Context<Wry>) -> Self {
        Self {
            context: Mutex::new(Some(context)),
        }
    }
}

impl ServerTrayFactory for ServerTrayIconService {
    fn spawn_tray(
        &self,
        configuration: ServerTrayConfiguration,
    ) -> Option<Arc<dyn ServerTrayHandle>> {
        // The REST controller spawns the tray at most once per launch, so the
        // owned (non-cloneable) Tauri context is moved out on first use.
        let context = self.context.lock().expect("tray context poisoned").take()?;
        spawn_tauri_server_tray(context, None, configuration)
    }

    /// macOS requires the Tauri event loop on the process main thread, so the
    /// tray is run (blocking) on the calling thread instead of a background one.
    ///
    /// # Arguments
    /// * `configuration` - Menu and URL state used to build the tray.
    ///
    /// # Returns
    /// The live tray handle, or `None` when the context was already consumed.
    #[cfg(target_os = "macos")]
    fn run_on_main_thread(
        &self,
        configuration: ServerTrayConfiguration,
    ) -> Option<Arc<dyn ServerTrayHandle>> {
        let context = self.context.lock().expect("tray context poisoned").take()?;
        run_tauri_server_tray_on_main_thread(context, None, configuration)
    }
}
