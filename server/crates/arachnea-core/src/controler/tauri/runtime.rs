//! Assembly and launch of the native Tauri application runtime.

use super::{ipc, protocol, tabs, windows, TauriBrowsingMode, TauriControlerService};
use crate::controler::{install_global_main_thread_dispatcher, MainThreadDispatcher};
use std::sync::Arc;

/// Installs the dispatcher, protocols and IPC before running the application.
///
/// # Arguments
/// * `service` - Controller whose application context and handlers are consumed once.
pub(super) fn launch(service: &mut TauriControlerService) {
    let handlers = std::mem::take(&mut service.handlers);
    let json_handlers = std::mem::take(&mut service.json_handlers);
    let stream_handlers = std::mem::take(&mut service.stream_handlers);
    let web_assets = service.web_assets.clone();
    let web_scheme = service.web_scheme.clone();
    let api_prefix = service.api_prefix.clone();
    let mut context = service.build_context();
    let main_window_config = context
        .config_mut()
        .app
        .windows
        .first_mut()
        .and_then(|window| {
            if !window.create {
                return None;
            }
            let config = window.clone();
            window.create = false;
            Some(config)
        });
    let setup_web_scheme = web_scheme.clone();
    let browsing_mode = service.browsing_mode;
    let setup_dispatcher = Arc::clone(&service.main_thread_dispatcher);
    let global_dispatcher: Arc<dyn MainThreadDispatcher> = service.main_thread_dispatcher.clone();
    install_global_main_thread_dispatcher(global_dispatcher)
        .expect("Failed to install the Tauri main-thread dispatcher.");
    let builder = ::tauri::Builder::default()
        .setup(move |app| {
            setup_dispatcher.attach_app_handle(app.handle().clone())?;
            if let Some(config) = main_window_config {
                match browsing_mode {
                    TauriBrowsingMode::Tabs => tabs::setup(app.handle(), config, setup_web_scheme)?,
                    TauriBrowsingMode::Windows => {
                        windows::setup(app.handle(), config, setup_web_scheme)?
                    }
                }
            }
            Ok(())
        })
        .plugin(
            ::tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(true)
                .build(),
        );

    let invoke_web_scheme = web_scheme.clone();
    let builder = protocol::register(builder, web_assets, stream_handlers, web_scheme, api_prefix);
    builder
        .invoke_handler(ipc::handler(handlers, json_handlers, invoke_web_scheme))
        .run(context)
        .expect("Error launching the Tauri application.");
}
