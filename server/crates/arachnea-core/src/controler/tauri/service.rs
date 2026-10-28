//! Controller state, command registration and application-owned context adaptation.

use super::{
    assets::TauriWebAssets, dispatcher::TauriMainThreadDispatcher, TauriBrowsingMode,
    TauriControlerConfiguration,
};
use crate::controler::{
    ControlerService, JsonControlerFunction, MainThreadDispatcher, SerializedControlerFunction,
    SharedWebAssets, StreamControlerFunction,
};
use std::sync::Arc;

/// Controller backend for a Tauri application.
///
/// This struct implements the ControlerService trait for Tauri applications,
/// providing a bridge between the Arachnea controller system and Tauri's
/// IPC and webview systems.
pub struct TauriControlerService {
    /// The Tauri context (consumed during launch).
    context: Option<::tauri::Context<::tauri::Wry>>,
    /// The custom URI scheme for web assets.
    pub(super) web_scheme: String,
    /// The API path prefix for stream routes.
    pub(super) api_prefix: String,
    /// Presentation used for public frontend browsing contexts.
    pub(super) browsing_mode: TauriBrowsingMode,
    /// Registered serialized function handlers.
    pub(super) handlers: Vec<(String, SerializedControlerFunction)>,
    /// Registered header-aware JSON function handlers.
    pub(super) json_handlers: Vec<(String, JsonControlerFunction)>,
    /// Registered stream function handlers with their entry points.
    pub(super) stream_handlers: Vec<(String, StreamControlerFunction, String)>,
    /// Configured web assets mounts.
    ///
    /// The first registered mount is the root frontend; additional mounts serve
    /// dedicated bundles (e.g. the administration app) below their own path.
    pub(super) web_assets: Vec<TauriWebAssets>,
    /// The main thread dispatcher for this service.
    pub(super) main_thread_dispatcher: Arc<TauriMainThreadDispatcher>,
}

impl TauriControlerService {
    /// Creates an empty Tauri controller service from an application-owned context.
    ///
    /// # Arguments
    /// * `context` - Generated Tauri context owned by the application crate.
    ///
    /// # Returns
    /// A new TauriControlerService with default configuration.
    pub fn new(context: ::tauri::Context<::tauri::Wry>) -> Self {
        Self::with_configuration(TauriControlerConfiguration::new(context))
    }

    /// Creates an empty Tauri controller service from explicit configuration.
    ///
    /// # Arguments
    /// * `configuration` - Tauri controller configuration supplied by the application crate.
    ///
    /// # Returns
    /// A new TauriControlerService with the specified configuration.
    pub fn with_configuration(configuration: TauriControlerConfiguration) -> Self {
        Self {
            context: Some(configuration.context),
            web_scheme: configuration.web_scheme,
            api_prefix: configuration.api_prefix,
            browsing_mode: configuration.browsing_mode,
            handlers: Vec::new(),
            json_handlers: Vec::new(),
            stream_handlers: Vec::new(),
            web_assets: Vec::new(),
            main_thread_dispatcher: TauriMainThreadDispatcher::new(),
        }
    }

    /// Loads frontend assets directly from a directory on disk.
    ///
    /// # Arguments
    /// * `directory_path` - Directory containing the frontend assets to expose.
    /// * `path` - Relative path under the Tauri root where the assets should be mounted.
    #[allow(dead_code)]
    pub fn register_web_directory(&mut self, directory_path: &str, path: &str) {
        self.web_assets
            .push(TauriWebAssets::from_directory(directory_path, path));
    }

    /// Loads frontend assets from the executable embedded resources.
    ///
    /// # Arguments
    /// * `assets` - Embedded frontend asset provider supplied by the application crate.
    /// * `path` - Relative path under the Tauri root where the assets should be mounted.
    #[allow(dead_code)]
    pub fn register_embedded_web_assets(&mut self, assets: SharedWebAssets, path: &str) {
        self.web_assets
            .push(TauriWebAssets::from_embedded(assets, path));
    }

    /// Builds the final Tauri context with configured web assets.
    ///
    /// This method is called during launch and consumes the internal context.
    /// The first main window is pointed at the root mount (or the first one
    /// when no root bundle is registered); dedicated windows are created on
    /// demand by the administration IPC handler.
    ///
    /// # Returns
    /// The configured Tauri context ready for application launch.
    pub(super) fn build_context(&mut self) -> ::tauri::Context<::tauri::Wry> {
        let mut context = self
            .context
            .take()
            .expect("Tauri controller launched more than once.");
        if let Some(web_assets) = self
            .web_assets
            .iter()
            .find(|assets| assets.mount_path.is_empty())
            .or_else(|| self.web_assets.first())
        {
            if let Some(window) = context.config_mut().app.windows.first_mut() {
                window.url = ::tauri::WebviewUrl::CustomProtocol(
                    web_assets
                        .window_url(&self.web_scheme)
                        .parse()
                        .expect("Invalid Tauri frontend URL."),
                );
            }
        }
        context
    }
}

impl ControlerService for TauriControlerService {
    fn register_serialized_function(&mut self, command: &str, call: SerializedControlerFunction) {
        self.handlers.push((command.to_string(), call));
    }

    fn register_json_function(&mut self, command: &str, call: JsonControlerFunction) {
        self.json_handlers.push((command.to_string(), call));
    }

    fn register_stream_function(&mut self, command: &str, call: StreamControlerFunction) {
        // Build the entry-point URL prefix for this command in the Tauri scheme.
        // Custom-scheme webview URLs are served from the `localhost` host (see
        // `window_url`), so the normalized API prefix and command form the URL
        // path (`arachnea-stream://localhost/api/proxy`).
        let entry_point = format!(
            "{}://localhost{}/{}",
            self.web_scheme.trim_end_matches(':'),
            self.api_prefix.trim_end_matches('/'),
            command.trim_matches('/')
        );
        self.stream_handlers
            .push((command.to_string(), call, entry_point));
    }

    fn stream_public_path(&self, command: &str) -> String {
        let mut path = self.api_prefix.trim_end_matches('/').to_string();
        let command = command.trim_matches('/');

        if !command.is_empty() {
            path.push('/');
            path.push_str(command);
        }

        path
    }

    fn register_web_directory(&mut self, directory_path: &str, path: &str) {
        TauriControlerService::register_web_directory(self, directory_path, path);
    }

    fn register_embedded_web_assets(&mut self, assets: SharedWebAssets, path: &str) {
        TauriControlerService::register_embedded_web_assets(self, assets, path);
    }

    fn main_thread_dispatcher(&self) -> Option<Arc<dyn MainThreadDispatcher>> {
        let dispatcher: Arc<dyn MainThreadDispatcher> = self.main_thread_dispatcher.clone();
        Some(dispatcher)
    }

    fn launch(&mut self) {
        super::runtime::launch(self);
    }
}
