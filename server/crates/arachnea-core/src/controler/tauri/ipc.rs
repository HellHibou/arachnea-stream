//! Tauri command dispatch and asynchronous administration-window creation.

use super::tabs;
use crate::controler::{
    ControlerFunctionInput, ControlerJsonInput, JsonControlerFunction, RequestControlerContext,
    SerializedControlerFunction,
};
use crate::{
    TAURI_ADMIN_WEB_MOUNT as ADMIN_WEB_MOUNT,
    TAURI_ADMIN_WINDOW_DEFAULT_ROUTE as ADMIN_WINDOW_DEFAULT_ROUTE,
    TAURI_ADMIN_WINDOW_LABEL as ADMIN_WINDOW_LABEL,
    TAURI_ADMIN_WINDOW_OPEN_COMMAND as ADMIN_WINDOW_OPEN_COMMAND,
};
use ::tauri::{
    ipc::{Invoke, InvokeBody, InvokeError},
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Wry,
};
use anyhow::anyhow;
use serde_json::Value;

/// Builds the invoke handler while retaining registered callbacks for the runtime.
///
/// # Arguments
/// * `handlers` - Type-erased serialized callbacks.
/// * `json_handlers` - Header-aware JSON callbacks adapted to headerless IPC.
/// * `invoke_web_scheme` - Scheme used to open the administration bundle.
pub(super) fn handler(
    handlers: Vec<(String, SerializedControlerFunction)>,
    json_handlers: Vec<(String, JsonControlerFunction)>,
    invoke_web_scheme: String,
) -> impl Fn(Invoke) -> bool + Send + Sync + 'static {
    move |invoke: Invoke| {
        if invoke.message.command().starts_with("desktop_tabs_") {
            return tabs::handle_invoke(invoke);
        }
        let command = invoke.message.command().to_string();
        if command == ADMIN_WINDOW_OPEN_COMMAND {
            let source_window = invoke.message.webview().window();
            let title = match source_window.title() {
                Ok(title) => title,
                Err(error) => {
                    invoke
                        .resolver
                        .respond(Err::<(), InvokeError>(InvokeError::from_anyhow(anyhow!(
                            "Failed to read source window title: {error}"
                        ))));
                    return true;
                }
            };
            let app = source_window.app_handle().clone();
            let scheme = invoke_web_scheme.trim_end_matches(':');
            // Open the bundle root by default; the admin router resolves
            // it to the first configured service group.
            let default_route = ADMIN_WINDOW_DEFAULT_ROUTE.trim_matches('/');
            let admin_url = if default_route.is_empty() {
                format!("{scheme}://localhost/{ADMIN_WEB_MOUNT}/")
            } else {
                format!("{scheme}://localhost/{ADMIN_WEB_MOUNT}/{default_route}/")
            };
            // build() must not run synchronously inside the IPC handler:
            // it waits on the main-thread event loop, which is currently
            // processing this invoke (hard deadlock on Windows, UI freeze
            // on macOS/Linux). Spawning a thread lets Tauri dispatch the
            // window creation to the main loop on all platforms.
            std::thread::spawn(move || open_admin_window(app, admin_url, title));
            invoke.resolver.respond(Ok::<(), InvokeError>(()));
            return true;
        }
        if let Some((_cmd, handler)) = handlers.iter().find(|(c, _)| *c == command) {
            let handler = std::sync::Arc::clone(handler);
            let payload =
                match invoke.message.payload() {
                    InvokeBody::Json(payload) => payload.clone(),
                    InvokeBody::Raw(_) => {
                        invoke.resolver.respond(Err::<Value, InvokeError>(
                            InvokeError::from_anyhow(anyhow!(
                                "Raw invoke payloads are not supported for command: {}",
                                command
                            )),
                        ));
                        return true;
                    }
                };

            invoke.resolver.respond_async(async move {
                handler(ControlerFunctionInput::Json(payload))
                    .await
                    .map_err(|e| InvokeError::from_anyhow(anyhow!("Handler error: {}", e)))
            });
            true
        } else if let Some((_cmd, handler)) = json_handlers.iter().find(|(c, _)| *c == command) {
            // Tauri IPC carries no HTTP headers and no conditional
            // validation; the desktop frontend never sends ETags.
            let handler = std::sync::Arc::clone(handler);
            let payload =
                match invoke.message.payload() {
                    InvokeBody::Json(payload) => payload.clone(),
                    InvokeBody::Raw(_) => {
                        invoke.resolver.respond(Err::<Value, InvokeError>(
                            InvokeError::from_anyhow(anyhow!(
                                "Raw invoke payloads are not supported for command: {}",
                                command
                            )),
                        ));
                        return true;
                    }
                };

            invoke.resolver.respond_async(async move {
                handler(ControlerJsonInput {
                    payload: ControlerFunctionInput::Json(payload),
                    // Tauri IPC carries no HTTP headers.
                    context: RequestControlerContext::default(),
                })
                .await
                .map(|output| output.value)
                .map_err(|e| InvokeError::from_anyhow(anyhow!("Handler error: {}", e)))
            });
            true
        } else {
            invoke
                .resolver
                .respond(Err::<Value, InvokeError>(InvokeError::from_anyhow(
                    anyhow!("Unknown command: {}", command),
                )));
            false
        }
    }
}

/// Opens the dedicated administration window, reusing it when it still exists.
///
/// The window loads the admin bundle through the custom web scheme (whose
/// protocol handler serves the `admin` mount). Closing the window never exits
/// the application; a later invocation recreates it.
///
/// # Arguments
/// * `app` - The running Tauri application handle.
/// * `admin_url` - Custom-scheme URL of the administration bundle.
/// * `title` - Current title of the window requesting administration.
fn open_admin_window(app: AppHandle<Wry>, admin_url: String, title: String) {
    if let Some(window) = app.get_webview_window(ADMIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }

    let url: ::tauri::Url = admin_url.parse().expect("Invalid Tauri admin URL.");
    let window =
        WebviewWindowBuilder::new(&app, ADMIN_WINDOW_LABEL, WebviewUrl::CustomProtocol(url))
            .title(title)
            .inner_size(
                crate::TAURI_ADMIN_WINDOW_WIDTH,
                crate::TAURI_ADMIN_WINDOW_HEIGHT,
            )
            .build();

    if let Err(error) = window {
        tracing::warn!(target: "arachnea_core::controler::tauri", error = %error, "failed to open the administration window");
    }
}
