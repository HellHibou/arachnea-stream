//! Tauri command dispatch and asynchronous administration-window creation.

use super::tabs;
use crate::controler::{
    ControlerFunctionInput, ControlerJsonInput, ControlerJsonOutput, JsonControlerFunction,
    RequestControlerContext, SerializedControlerFunction,
};
use crate::{
    HEADER_ETAG, HEADER_IF_NONE_MATCH, NATIVE_CONDITIONAL_REPLY_FIELD, NATIVE_ETAG_FIELD,
    TAURI_ADMIN_WEB_MOUNT as ADMIN_WEB_MOUNT,
    TAURI_ADMIN_WINDOW_DEFAULT_ROUTE as ADMIN_WINDOW_DEFAULT_ROUTE,
    TAURI_ADMIN_WINDOW_LABEL as ADMIN_WINDOW_LABEL,
    TAURI_ADMIN_WINDOW_OPEN_COMMAND as ADMIN_WINDOW_OPEN_COMMAND,
};
use ::tauri::{
    ipc::{Invoke, InvokeBody, InvokeError},
    AppHandle, Emitter, EventTarget, Manager, WebviewUrl, WebviewWindowBuilder, Wry,
};
use anyhow::anyhow;
use serde_json::{json, Map, Value};

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
        let webview = invoke.message.webview();
        let caller = super::caller::capture(&webview, &invoke_web_scheme);
        let Some(caller) = caller else {
            invoke
                .resolver
                .respond(Err::<(), InvokeError>(InvokeError::from(
                    "Untrusted native caller",
                )));
            return true;
        };
        if invoke.message.command().starts_with("admin/")
            && (caller.label != ADMIN_WINDOW_LABEL
                || !caller.url.contains(&format!("/{ADMIN_WEB_MOUNT}/")))
        {
            invoke
                .resolver
                .respond(Err::<(), InvokeError>(InvokeError::from(
                    "Administration window required",
                )));
            return true;
        }
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
            // Native IPC carries no HTTP headers: the conditional validator is
            // lifted from the reserved payload field into the request context.
            let handler = std::sync::Arc::clone(handler);
            let mut payload =
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
            let native_etag = payload
                .as_object()
                .and_then(|fields| fields.get(NATIVE_ETAG_FIELD))
                .and_then(Value::as_str)
                .map(str::to_string);
            if let Some(fields) = payload.as_object_mut() {
                fields.remove(NATIVE_ETAG_FIELD);
            }

            let app = invoke.message.webview().app_handle().clone();
            invoke.resolver.respond_async(async move {
                let source_label = caller.label.clone();
                let mut context = RequestControlerContext::default().with_native_webview(caller);
                if let Some(etag) = native_etag {
                    context = context.with_header(HEADER_IF_NONE_MATCH, etag);
                }
                handler(ControlerJsonInput {
                    payload: ControlerFunctionInput::Json(payload),
                    context,
                })
                .await
                .map(|output| {
                    if let Some(event) = output.headers.get(crate::CONTROLLER_NATIVE_EVENT_HEADER) {
                        let _ = app.emit_filter(event, (), |target| {
                            matches!(target, EventTarget::Webview { label } if label != &source_label)
                        });
                    }
                    native_conditional_reply(output)
                })
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

/// Wraps a conditional native reply so the frontend can cache and replay it.
///
/// Replies without an ETag and without a `304` status are returned unchanged,
/// keeping every existing Tauri command response compatible.
///
/// # Arguments
/// * `output` - Header-aware controller output to relay through IPC.
fn native_conditional_reply(output: ControlerJsonOutput) -> Value {
    let etag = output.headers.get(HEADER_ETAG).cloned();
    if etag.is_none() && output.status != 304 {
        return output.value;
    }

    let mut reply = Map::new();
    reply.insert("not_modified".to_string(), json!(output.status == 304));
    reply.insert("etag".to_string(), etag.map_or(Value::Null, Value::String));
    reply.insert("value".to_string(), output.value);
    let mut wrapper = Map::new();
    wrapper.insert(
        NATIVE_CONDITIONAL_REPLY_FIELD.to_string(),
        Value::Object(reply),
    );
    Value::Object(wrapper)
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
