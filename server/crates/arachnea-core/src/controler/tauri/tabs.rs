//! Serial desktop tab lifecycle management for independent child webviews.

use std::sync::mpsc::{self, Sender};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use tauri::{
    ipc::{Invoke, InvokeBody, InvokeError},
    webview::{NewWindowResponse, WebviewBuilder},
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Rect, WebviewUrl, Window,
    WindowEvent, Wry,
};
use tauri_plugin_opener::OpenerExt;

/// Logical height reserved for the desktop tab strip.
const STRIP_HEIGHT: f64 = 44.0;
/// Event carrying the authoritative tab snapshot to the shell.
const CHANGED_EVENT: &str = "desktop-tabs-changed";

type Job = Box<dyn FnOnce(&mut TabHost) + Send>;

/// Queue keeping native webview operations off the UI callback thread.
struct TabQueue(Sender<Job>);

/// Public description of one independent page.
#[derive(Clone, Serialize)]
struct Tab {
    /// Unique child webview label.
    id: String,
    /// Document title, initially inherited from the native window.
    title: String,
}

/// Snapshot consumed by the tab strip.
#[derive(Clone, Serialize)]
struct Snapshot {
    /// Tabs in creation order.
    tabs: Vec<Tab>,
    /// Currently visible page.
    active: String,
}

/// Unobscured native content rectangle in the parent view's logical coordinates.
struct ContentLayout {
    origin: LogicalPosition<f64>,
    size: LogicalSize<f64>,
}

/// Reads the usable content rectangle rather than assuming the parent starts below decorations.
///
/// # Arguments
/// * `window` - Native host whose child webviews will be positioned.
fn content_layout(window: &Window<Wry>) -> Result<ContentLayout> {
    #[cfg(target_os = "macos")]
    {
        let (sender, receiver) = mpsc::sync_channel(1);
        let native_window = window.clone();
        window.run_on_main_thread(move || {
            let result = (|| -> Result<ContentLayout> {
                // Tauri owns both objects; access is restricted to the AppKit main thread.
                let view = unsafe { &*native_window.ns_view()?.cast::<objc2_app_kit::NSView>() };
                let window =
                    unsafe { &*native_window.ns_window()?.cast::<objc2_app_kit::NSWindow>() };
                let bounds = view.bounds();
                let content = view.convertRect_fromView(window.contentLayoutRect(), None);
                let top = if view.isFlipped() {
                    content.origin.y - bounds.origin.y
                } else {
                    bounds.origin.y + bounds.size.height - content.origin.y - content.size.height
                };
                Ok(ContentLayout {
                    origin: LogicalPosition::new(
                        (content.origin.x - bounds.origin.x).max(0.0),
                        top.max(0.0),
                    ),
                    size: LogicalSize::new(
                        content.size.width.max(1.0),
                        content.size.height.max(1.0),
                    ),
                })
            })();
            let _ = sender.send(result);
        })?;
        receiver
            .recv()
            .context("Failed to read native content layout")?
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(ContentLayout {
            origin: LogicalPosition::new(0.0, 0.0),
            size: window.inner_size()?.to_logical(window.scale_factor()?),
        })
    }
}

/// State owned exclusively by the tab worker thread.
struct TabHost {
    window: Window<Wry>,
    shell: String,
    home: WebviewUrl,
    scheme: String,
    queue: Sender<Job>,
    tabs: Vec<Tab>,
    active: String,
    sequence: u64,
    config: tauri::utils::config::WindowConfig,
    running: bool,
}

/// Creates the configured native window and starts its serialized tab worker.
///
/// # Arguments
/// * `app` - Running application handle.
/// * `config` - Application-owned native window and frontend configuration.
/// * `scheme` - Registered frontend URI scheme.
///
/// # Errors
/// Returns native window creation errors.
pub(super) fn setup(
    app: &AppHandle<Wry>,
    config: tauri::utils::config::WindowConfig,
    scheme: String,
) -> Result<()> {
    let window = tauri::window::WindowBuilder::from_config(app, &config)?.build()?;
    let (tx, rx) = mpsc::channel::<Job>();
    app.manage(TabQueue(tx.clone()));
    let resize_tx = tx.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            let _ = resize_tx.send(Box::new(|host| host.running = false));
        }
        if matches!(
            event,
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }
        ) {
            let _ = resize_tx.send(Box::new(|host| {
                if let Err(error) = host.resize() {
                    tracing::warn!(%error, "failed to resize desktop tabs");
                }
            }));
        }
    });
    std::thread::spawn(move || {
        let mut host = TabHost {
            shell: format!("{}-tabs", config.label),
            home: config.url.clone(),
            window,
            scheme,
            queue: tx,
            tabs: Vec::new(),
            active: String::new(),
            sequence: 0,
            config: config.clone(),
            running: true,
        };
        let initialize = (|| -> Result<()> {
            let layout = content_layout(&host.window)?;
            let size = layout.size;
            let mut shell_config = config;
            shell_config.label = host.shell.clone();
            host.window.add_child(
                WebviewBuilder::from_config(&shell_config)
                    .initialization_script("window.__DESKTOP_TAB_SHELL__ = true;"),
                layout.origin,
                LogicalSize::new(size.width, STRIP_HEIGHT),
            )?;
            host.open(host.home.clone())?;
            Ok(())
        })();
        if let Err(error) = initialize {
            tracing::error!(%error, "failed to initialize desktop tabs");
        }
        while let Ok(job) = rx.recv() {
            job(&mut host);
            if !host.running {
                break;
            }
        }
    });
    Ok(())
}

impl TabHost {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            tabs: self.tabs.clone(),
            active: self.active.clone(),
        }
    }

    fn publish(&self) {
        if let Err(error) =
            self.window
                .app_handle()
                .emit_to(&self.shell, CHANGED_EVENT, self.snapshot())
        {
            tracing::warn!(%error, "failed to publish desktop tabs");
        }
    }

    /// Keeps every page ready at the current content bounds, including hidden pages.
    fn resize(&self) -> Result<()> {
        let layout = content_layout(&self.window)?;
        let size = layout.size;
        let strip_height = if self.tabs.len() > 1 {
            STRIP_HEIGHT
        } else {
            0.0
        };
        let scale = self.window.scale_factor()?;
        if let Some(shell) = self.window.app_handle().get_webview(&self.shell) {
            shell.set_bounds(Rect {
                position: layout.origin.to_physical::<i32>(scale).into(),
                size: LogicalSize::new(size.width, STRIP_HEIGHT)
                    .to_physical::<u32>(scale)
                    .into(),
            })?;
            if strip_height > 0.0 {
                shell.show()?;
            } else {
                shell.hide()?;
            }
        }
        for tab in &self.tabs {
            if let Some(page) = self.window.app_handle().get_webview(&tab.id) {
                page.set_bounds(Rect {
                    position: LogicalPosition::new(layout.origin.x, layout.origin.y + strip_height)
                        .to_physical::<i32>(scale)
                        .into(),
                    size: LogicalSize::new(size.width, (size.height - strip_height).max(1.0))
                        .to_physical::<u32>(scale)
                        .into(),
                })?;
            }
        }
        Ok(())
    }

    /// Creates a real page webview, retaining its state until explicitly closed.
    ///
    /// # Arguments
    /// * `url` - Initial page URL served by the application.
    fn open(&mut self, url: WebviewUrl) -> Result<()> {
        self.sequence += 1;
        let id = format!("{}-tab-{}", self.window.label(), self.sequence);
        let title = self.window.title()?;
        let requests = self.queue.clone();
        let titles = self.queue.clone();
        let layout = content_layout(&self.window)?;
        let size = layout.size;
        let mut config = self.config.clone();
        config.label = id.clone();
        config.url = url;
        let builder = WebviewBuilder::from_config(&config)
            .on_new_window(move |url, _| {
                let _ = requests.send(Box::new(move |host| {
                    if let Err(error) = host.open_requested(url) {
                        tracing::warn!(%error, "failed to open requested tab");
                    }
                }));
                NewWindowResponse::Deny
            })
            .on_document_title_changed(move |webview, title| {
                let id = webview.label().to_string();
                let _ = titles.send(Box::new(move |host| {
                    if let Some(tab) = host.tabs.iter_mut().find(|tab| tab.id == id) {
                        if !title.trim().is_empty() {
                            tab.title = title;
                            host.publish();
                        }
                    }
                }));
            });
        let page = self
            .window
            .add_child(
                builder,
                LogicalPosition::new(layout.origin.x, layout.origin.y + STRIP_HEIGHT),
                LogicalSize::new(size.width, (size.height - STRIP_HEIGHT).max(1.0)),
            )
            .context("Failed to create tab webview")?;
        page.hide()?;
        self.tabs.push(Tab {
            id: id.clone(),
            title,
        });
        self.activate(&id)?;
        Ok(())
    }

    /// Routes a native popup request to an internal tab or the external system handler.
    ///
    /// # Arguments
    /// * `url` - Absolute target reported by the requesting webview.
    fn open_requested(&mut self, url: tauri::Url) -> Result<()> {
        let internal = (url.scheme() == self.scheme && url.host_str() == Some("localhost"))
            || (matches!(url.scheme(), "http" | "https")
                && url.host_str() == Some(format!("{}.localhost", self.scheme).as_str()));
        if internal {
            self.open(WebviewUrl::CustomProtocol(url))?;
        } else if matches!(url.scheme(), "http" | "https" | "mailto" | "tel") {
            self.window.opener().open_url(url.as_str(), None::<&str>)?;
        }
        Ok(())
    }

    /// Shows and focuses one page while hiding the previously active page.
    ///
    /// # Arguments
    /// * `id` - Registered tab label.
    fn activate(&mut self, id: &str) -> Result<()> {
        if !self.tabs.iter().any(|tab| tab.id == id) {
            return Err(anyhow!("Unknown desktop tab"));
        }
        let app = self.window.app_handle();
        let page = app.get_webview(id).context("Tab webview is unavailable")?;
        self.resize()?;
        page.show()?;
        if self.active != id {
            if let Some(previous) = app.get_webview(&self.active) {
                previous.hide()?;
            }
        }
        page.set_focus()?;
        self.active = id.to_string();
        self.publish();
        Ok(())
    }

    /// Destroys a page, selecting a neighbor or creating a fresh last home tab.
    ///
    /// # Arguments
    /// * `id` - Registered tab label to close.
    fn close(&mut self, id: &str) -> Result<()> {
        let index = self
            .tabs
            .iter()
            .position(|tab| tab.id == id)
            .context("Unknown desktop tab")?;
        if self.tabs.len() == 1 {
            self.open(self.home.clone())?;
        }
        if self.active == id {
            let replacement = if index > 0 { index - 1 } else { 1 };
            self.activate(&self.tabs[replacement].id.clone())?;
        }
        if let Some(page) = self.window.app_handle().get_webview(id) {
            page.close()?;
        }
        self.tabs.remove(index);
        self.resize()?;
        self.publish();
        Ok(())
    }
}

/// Routes tab commands from the shell to the worker, returning contextual IPC errors.
///
/// # Arguments
/// * `invoke` - Command invocation with an optional tab `id`.
pub(super) fn handle_invoke(invoke: Invoke<Wry>) -> bool {
    let app = invoke.message.webview().app_handle().clone();
    let Some(queue) = app.try_state::<TabQueue>() else {
        invoke.resolver.respond(Err::<(), _>(InvokeError::from(
            "Desktop tabs are unavailable",
        )));
        return true;
    };
    let source = invoke.message.webview().label().to_string();
    let command = invoke.message.command().to_string();
    let id = match invoke.message.payload() {
        InvokeBody::Json(value) => value
            .get("id")
            .and_then(|id| id.as_str())
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    };
    let fallback_resolver = invoke.resolver.clone();
    let job: Job = Box::new(move |host| {
        let result = (|| -> Result<serde_json::Value> {
            if source != host.shell {
                return Err(anyhow!(
                    "Desktop tab commands are restricted to the tab shell"
                ));
            }
            match command.as_str() {
                "desktop_tabs_snapshot" => {}
                "desktop_tabs_open" => host.open(host.home.clone())?,
                "desktop_tabs_activate" => host.activate(&id)?,
                "desktop_tabs_close" => host.close(&id)?,
                _ => return Err(anyhow!("Unknown desktop tab command")),
            }
            Ok(serde_json::to_value(host.snapshot())?)
        })();
        invoke
            .resolver
            .respond(result.map_err(InvokeError::from_anyhow));
    });
    if let Err(error) = queue.0.send(job) {
        tracing::error!(%error, "desktop tab worker is unavailable");
        fallback_resolver.respond(Err::<(), _>(InvokeError::from(
            "Desktop tab worker is unavailable",
        )));
    }
    true
}
