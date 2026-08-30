//! Tauri controller backend.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use ::tauri::{
    ipc::{Invoke, InvokeBody, InvokeError},
    utils::assets::AssetKey,
    AppHandle, Assets, Manager, WebviewUrl, WebviewWindowBuilder, Wry,
};
use anyhow::anyhow;
use serde_json::Value;

use super::web_assets::{
    normalize_mount_path, replace_html_base, scope_web_asset_source, strip_mount_path,
    EmbeddedWebAssets, SharedWebAssets, WebAssetSource,
};
use super::{
    install_global_main_thread_dispatcher, main_thread::MainThreadHandlerStore,
    ControlerFunctionInput, ControlerJsonInput, ControlerService, ControlerStreamInput,
    ControlerStreamOutput, JsonControlerFunction, MainThreadContext, MainThreadDispatchError,
    MainThreadDispatcher, MainThreadEvent, MainThreadHandler, MainThreadHandlerId, MainThreadTask,
    RequestControlerContext, ResponseBody, SerializedControlerFunction, StreamControlerFunction,
};

/// The default custom URI scheme used for Tauri web assets.
const DEFAULT_TAURI_WEB_SCHEME: &str = "arachnea";

/// The default API path prefix used by Tauri binary stream routes.
const DEFAULT_TAURI_API_PREFIX: &str = "/api/";

/// Relative `<base>` used when serving the desktop frontend, which lives at the scheme root.
const TAURI_WEB_BASE: &str = "./";

/// Label of the dedicated administration window, created on demand.
const ADMIN_WINDOW_LABEL: &str = "admin";

/// Invoke command opening (or refocusing) the dedicated administration window.
const ADMIN_WINDOW_OPEN_COMMAND: &str = "open_admin_window";

/// Mount path (inside the scheme root) serving the administration bundle.
const ADMIN_WEB_MOUNT: &str = "admin";

/// Configuration required to build a Tauri controller service.
///
/// This struct holds all the configuration needed to create a Tauri controller,
/// including the Tauri context and customization options.
pub struct TauriControlerConfiguration {
    /// The Tauri context owned by the application crate.
    context: ::tauri::Context<::tauri::Wry>,
    /// The custom URI scheme used to serve the frontend.
    web_scheme: String,
    /// The API path prefix used by binary stream routes.
    api_prefix: String,
}

impl TauriControlerConfiguration {
    /// Creates a Tauri controller configuration from an application-owned Tauri context.
    ///
    /// # Arguments
    /// * `context` - Generated Tauri context owned by the application crate.
    ///
    /// # Returns
    /// A new TauriControlerConfiguration with default settings.
    pub fn new(context: ::tauri::Context<::tauri::Wry>) -> Self {
        Self {
            context,
            web_scheme: DEFAULT_TAURI_WEB_SCHEME.to_string(),
            api_prefix: DEFAULT_TAURI_API_PREFIX.to_string(),
        }
    }

    /// Sets the custom URI scheme used to serve the frontend.
    ///
    /// # Arguments
    /// * `web_scheme` - Custom protocol scheme registered with Tauri.
    ///
    /// # Returns
    /// The modified configuration for method chaining.
    pub fn web_scheme(mut self, web_scheme: impl Into<String>) -> Self {
        self.web_scheme = normalize_web_scheme(&web_scheme.into());
        self
    }

    /// Sets the API path prefix used by binary stream routes.
    ///
    /// # Arguments
    /// * `api_prefix` - Path prefix used before registered binary stream commands.
    ///
    /// # Returns
    /// The modified configuration for method chaining.
    pub fn api_prefix(mut self, api_prefix: impl Into<String>) -> Self {
        self.api_prefix = normalize_api_prefix(&api_prefix.into());
        self
    }
}

/// Embedded web assets backed by a Tauri asset bundle.
///
/// This struct implements the EmbeddedWebAssets trait for Tauri's built-in
/// asset system.
pub struct TauriEmbeddedWebAssets {
    /// The Tauri asset bundle owned by the application crate.
    assets: Arc<dyn Assets<::tauri::Wry>>,
}

impl TauriEmbeddedWebAssets {
    /// Creates an embedded web asset provider from Tauri generated assets.
    ///
    /// # Arguments
    /// * `assets` - Generated Tauri asset bundle owned by the application crate.
    ///
    /// # Returns
    /// A new TauriEmbeddedWebAssets instance.
    pub fn new(assets: Arc<dyn Assets<::tauri::Wry>>) -> Self {
        Self { assets }
    }
}

impl EmbeddedWebAssets for TauriEmbeddedWebAssets {
    /// Loads an embedded asset by normalized relative path.
    ///
    /// # Arguments
    /// * `path` - The normalized relative path to the asset.
    ///
    /// # Returns
    /// `Some(Vec<u8>)` containing the asset bytes if found.
    /// `None` if the asset does not exist.
    fn get(&self, path: &str) -> Option<Vec<u8>> {
        let key: AssetKey = path.into();
        self.assets.get(&key).map(Cow::into_owned)
    }
}

/// Internal representation of Tauri web assets configuration.
#[derive(Clone)]
struct TauriWebAssets {
    /// The normalized mount path for these assets.
    mount_path: String,
    /// The source from which to load the assets.
    source: WebAssetSource,
}

impl TauriWebAssets {
    /// Creates Tauri web assets from a directory on disk.
    ///
    /// # Arguments
    /// * `directory_path` - Directory containing the frontend assets.
    /// * `path` - Relative path under the Tauri root where assets should be mounted.
    ///
    /// # Returns
    /// A new TauriWebAssets instance configured for directory loading.
    fn from_directory(directory_path: &str, path: &str) -> Self {
        let mount_path = normalize_mount_path(path);
        Self {
            mount_path: mount_path.clone(),
            source: scope_web_asset_source(
                WebAssetSource::directory(directory_path),
                &mount_path,
            ),
        }
    }

    /// Creates Tauri web assets from embedded resources.
    ///
    /// # Arguments
    /// * `assets` - Embedded frontend asset provider.
    /// * `path` - Relative path under the Tauri root where assets should be mounted.
    ///
    /// # Returns
    /// A new TauriWebAssets instance configured for embedded loading.
    fn from_embedded(assets: SharedWebAssets, path: &str) -> Self {
        let mount_path = normalize_mount_path(path);
        Self {
            mount_path: mount_path.clone(),
            source: scope_web_asset_source(WebAssetSource::embedded(assets), &mount_path),
        }
    }

    /// Returns whether this mount serves the given request path.
    ///
    /// The root mount (empty path) serves every request; a scoped mount only
    /// serves requests whose path is below its mount segment, so a bundle
    /// mounted at `/admin/` never shadows sibling routes.
    ///
    /// # Arguments
    /// * `request_path` - The normalized request path starting with `/`.
    ///
    /// # Returns
    /// `true` when this mount is a candidate for the request.
    fn matches(&self, request_path: &str) -> bool {
        if self.mount_path.is_empty() {
            return true;
        }
        request_path == format!("/{}", self.mount_path)
            || request_path.starts_with(&format!("/{}/", self.mount_path))
    }

    /// Generates the window URL for these web assets.
    ///
    /// # Arguments
    /// * `web_scheme` - The custom URI scheme to use.
    ///
    /// # Returns
    /// The URL string for the Tauri window configuration.
    fn window_url(&self, web_scheme: &str) -> String {
        if self.mount_path.is_empty() {
            format!("{web_scheme}://localhost/")
        } else {
            format!("{web_scheme}://localhost/{}/", self.mount_path)
        }
    }

    /// Loads a web asset from a request path.
    ///
    /// # Arguments
    /// * `request_path` - The requested asset path.
    ///
    /// # Returns
    /// `Ok(WebAsset)` if the asset is found.
    /// `Err(String)` if the asset cannot be found.
    fn load_request(&self, request_path: &str) -> Result<super::web_assets::WebAsset, String> {
        let relative_path = strip_mount_path(request_path, &self.mount_path).ok_or_else(|| {
            format!(
                "Web asset not found: {}",
                request_path.trim_start_matches('/')
            )
        })?;
        self.source.load(&relative_path)
    }
}

/// Main thread dispatcher implementation for Tauri applications.
struct TauriMainThreadDispatcher {
    /// The Tauri application handle (set during setup).
    app_handle: Mutex<Option<::tauri::AppHandle<::tauri::Wry>>>,
    /// Tasks queued before the app handle is available.
    pending_tasks: Mutex<Vec<MainThreadTask>>,
    /// Registered event handlers.
    handlers: Arc<MainThreadHandlerStore>,
    /// Shared main thread context.
    context: Arc<Mutex<MainThreadContext>>,
}

impl TauriMainThreadDispatcher {
    /// Creates a new Tauri main thread dispatcher.
    ///
    /// # Returns
    /// A new Arc-wrapped TauriMainThreadDispatcher instance.
    fn new() -> Arc<Self> {
        Arc::new(Self {
            app_handle: Mutex::new(None),
            pending_tasks: Mutex::new(Vec::new()),
            handlers: Arc::new(MainThreadHandlerStore::default()),
            context: Arc::new(Mutex::new(MainThreadContext::new())),
        })
    }

    /// Attaches the Tauri app handle to this dispatcher.
    ///
    /// This method must be called during Tauri setup to enable main thread
    /// dispatching. It also processes any tasks that were queued before
    /// the app handle was available.
    ///
    /// # Arguments
    /// * `app_handle` - The Tauri application handle.
    ///
    /// # Returns
    /// `Ok(())` if the app handle was successfully attached.
    /// `Err(MainThreadDispatchError)` if there was an error.
    fn attach_app_handle(
        &self,
        app_handle: ::tauri::AppHandle<::tauri::Wry>,
    ) -> Result<(), MainThreadDispatchError> {
        {
            let mut handle_slot = self.app_handle.lock().map_err(|_| {
                MainThreadDispatchError::DispatchFailed("Tauri app handle store poisoned".into())
            })?;
            *handle_slot = Some(app_handle.clone());
        }
        {
            let mut context = self.context.lock().map_err(|_| {
                MainThreadDispatchError::DispatchFailed("Tauri main-thread context poisoned".into())
            })?;
            context.insert_resource(app_handle.clone());
        }

        let pending_tasks = {
            let mut pending_tasks = self.pending_tasks.lock().map_err(|_| {
                MainThreadDispatchError::DispatchFailed("Tauri pending task queue poisoned".into())
            })?;
            pending_tasks.drain(..).collect::<Vec<_>>()
        };

        for task in pending_tasks {
            let context = Arc::clone(&self.context);
            app_handle
                .run_on_main_thread(move || {
                    if let Ok(mut context) = context.lock() {
                        task(&mut context);
                    }
                })
                .map_err(|err| MainThreadDispatchError::DispatchFailed(err.to_string()))?;
        }
        Ok(())
    }
}

impl MainThreadDispatcher for TauriMainThreadDispatcher {
    fn dispatch_main_thread_task(
        &self,
        task: MainThreadTask,
    ) -> Result<(), MainThreadDispatchError> {
        let app_handle = self
            .app_handle
            .lock()
            .map_err(|_| {
                MainThreadDispatchError::DispatchFailed("Tauri app handle store poisoned".into())
            })?
            .clone();

        if let Some(app_handle) = app_handle {
            let context = Arc::clone(&self.context);
            app_handle
                .run_on_main_thread(move || {
                    if let Ok(mut context) = context.lock() {
                        task(&mut context);
                    }
                })
                .map_err(|err| MainThreadDispatchError::DispatchFailed(err.to_string()))
        } else {
            self.pending_tasks
                .lock()
                .map_err(|_| {
                    MainThreadDispatchError::DispatchFailed(
                        "Tauri pending task queue poisoned".into(),
                    )
                })?
                .push(task);
            Ok(())
        }
    }

    fn register_main_thread_handler(
        &self,
        handler: MainThreadHandler,
    ) -> Result<MainThreadHandlerId, MainThreadDispatchError> {
        self.handlers.register(handler)
    }

    fn unregister_main_thread_handler(
        &self,
        id: MainThreadHandlerId,
    ) -> Result<(), MainThreadDispatchError> {
        self.handlers.unregister(id)
    }

    fn dispatch_main_thread_event(
        &self,
        id: MainThreadHandlerId,
        event: MainThreadEvent,
    ) -> Result<(), MainThreadDispatchError> {
        self.handlers.ensure_registered(id)?;
        let handlers = Arc::clone(&self.handlers);
        self.dispatch_main_thread_task(Box::new(move |context| {
            let _ = handlers.dispatch_event(context, id, event);
        }))
    }
}

/// Controller backend for a Tauri application.
///
/// This struct implements the ControlerService trait for Tauri applications,
/// providing a bridge between the Arachnea controller system and Tauri's
/// IPC and webview systems.
pub struct TauriControlerService {
    /// The Tauri context (consumed during launch).
    context: Option<::tauri::Context<::tauri::Wry>>,
    /// The custom URI scheme for web assets.
    web_scheme: String,
    /// The API path prefix for stream routes.
    api_prefix: String,
    /// Registered serialized function handlers.
    handlers: Vec<(String, SerializedControlerFunction)>,
    /// Registered header-aware JSON function handlers.
    json_handlers: Vec<(String, JsonControlerFunction)>,
    /// Registered stream function handlers with their entry points.
    stream_handlers: Vec<(String, StreamControlerFunction, String)>,
    /// Configured web assets mounts.
    ///
    /// The first registered mount is the root frontend; additional mounts serve
    /// dedicated bundles (e.g. the administration app) below their own path.
    web_assets: Vec<TauriWebAssets>,
    /// The main thread dispatcher for this service.
    main_thread_dispatcher: Arc<TauriMainThreadDispatcher>,
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
    /// demand by [`open_admin_window`].
    ///
    /// # Returns
    /// The configured Tauri context ready for application launch.
    fn build_context(&mut self) -> ::tauri::Context<::tauri::Wry> {
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
        let entry_point = format!(
            "{}://{}{}",
            self.web_scheme.trim_end_matches(':'),
            self.api_prefix.trim_matches('/'),
            command
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
        let handlers = std::mem::take(&mut self.handlers);
        let json_handlers = std::mem::take(&mut self.json_handlers);
        let stream_handlers = std::mem::take(&mut self.stream_handlers);
        let web_assets = self.web_assets.clone();
        let web_scheme = self.web_scheme.clone();
        let api_prefix = self.api_prefix.clone();
        let context = self.build_context();
        let setup_dispatcher = Arc::clone(&self.main_thread_dispatcher);
        let global_dispatcher: Arc<dyn MainThreadDispatcher> = self.main_thread_dispatcher.clone();
        install_global_main_thread_dispatcher(global_dispatcher)
            .expect("Failed to install the Tauri main-thread dispatcher.");
        let mut builder = ::tauri::Builder::default()
            .setup(move |app| {
                setup_dispatcher.attach_app_handle(app.handle().clone())?;
                Ok(())
            })
            .plugin(
                ::tauri_plugin_opener::Builder::new()
                    .open_js_links_on_click(true)
                    .build(),
            );

        // Kept for the invoke handler: the protocol closure moves `web_scheme`.
        let invoke_web_scheme = web_scheme.clone();
        if !web_assets.is_empty() {
            let uri_scheme = web_scheme.clone();
            builder = builder.register_uri_scheme_protocol(uri_scheme, move |_app, request| {
                let request_path = request_path_from_tauri_uri(request.uri(), &web_scheme);

                if let Some((command, path, query)) = split_stream_route(&request_path, &api_prefix)
                {
                    if let Some((_cmd, handler, entry_point)) = stream_handlers
                        .iter()
                        .find(|(registered, _, _)| registered == command)
                    {
                        let handler = std::sync::Arc::clone(handler);
                        let entry_point = entry_point.clone();
                        let method = request.method().to_string();
                        let headers_map = request
                            .headers()
                            .iter()
                            .filter_map(|(name, value)| {
                                value
                                    .to_str()
                                    .ok()
                                    .map(|v| (name.to_string(), v.to_string()))
                            })
                            .collect::<std::collections::HashMap<String, String>>();

                        let response = tokio::task::block_in_place(|| {
                            tauri::async_runtime::block_on(async {
                                let mut output = handler(ControlerStreamInput {
                                    path,
                                    query,
                                    method,
                                    headers: headers_map,
                                    body: request.body().to_vec(),
                                    entry_point,
                                    force_buffer_response: true,
                                })
                                .await?;

                                // Tauri does not support native streaming responses, so we
                                // collect the stream into memory here, inside the same async
                                // context where the TCP connection was established. This
                                // avoids waker-propagation issues that cause chunked body
                                // readers to fail when polled from a nested block_in_place.
                                if let ResponseBody::Streamed(stream) = &mut output.body {
                                    use futures::StreamExt;
                                    let mut body = Vec::new();
                                    let mut stream = std::pin::pin!(stream);
                                    while let Some(chunk) = stream.next().await {
                                        match chunk {
                                            Ok(bytes) => body.extend_from_slice(&bytes),
                                            Err(e) => {
                                                tracing::warn!(
                                                    error = %e,
                                                    "stream error while collecting body for Tauri response"
                                                );
                                                return Err(format!("Stream error: {}", e));
                                            }
                                        }
                                    }
                                    output.body = ResponseBody::Buffered(body);
                                }

                                Ok(output)
                            })
                        });

                        return match response {
                            Ok(ControlerStreamOutput {
                                status,
                                body,
                                content_type,
                                headers,
                            }) => {
                                let body = match body {
                                    ResponseBody::Buffered(mut bytes) => {
                                        if request.method() == ::tauri::http::Method::HEAD {
                                            bytes.clear();
                                        }
                                        bytes
                                    }
                                    ResponseBody::Streamed(_) => {
                                        unreachable!("streaming responses are collected inside the handler call")
                                    }
                                };
                                let mut builder = ::tauri::http::Response::builder()
                                    .status(
                                        ::tauri::http::StatusCode::from_u16(status)
                                            .unwrap_or(::tauri::http::StatusCode::OK),
                                    )
                                    .header("Content-Type", &content_type);
                                for (name, value) in &headers {
                                    if name.to_ascii_lowercase() != "content-type" {
                                        builder = builder.header(name.as_str(), value.as_str());
                                    }
                                }
                                builder
                                    .body(body)
                                    .expect("Failed to build the Tauri stream response.")
                            }
                            Err(error) => ::tauri::http::Response::builder()
                                .status(400)
                                .header("Content-Type", "text/plain; charset=utf-8")
                                .body(error.into_bytes())
                                .expect("Failed to build the Tauri stream error response."),
                        };
                    }
                }

                // Select the most specific mount serving this request: dedicated
                // bundles (e.g. `/admin/`) take precedence over the root mount.
                let selected = web_assets
                    .iter()
                    .filter(|assets| assets.matches(&request_path))
                    .max_by_key(|assets| assets.mount_path.len())
                    .map(|assets| assets.load_request(&request_path))
                    .unwrap_or_else(|| Err("Web asset not found".to_string()));
                match selected {
                    Ok(asset) => {
                        let bytes = if asset.mime_type.starts_with("text/html") {
                            replace_html_base(asset.bytes, TAURI_WEB_BASE)
                        } else {
                            asset.bytes
                        };
                        ::tauri::http::Response::builder()
                            .header("Content-Type", asset.mime_type)
                            .body(bytes)
                            .expect("Failed to build Tauri protocol response.")
                    }
                    Err(_) => ::tauri::http::Response::builder()
                        .status(404)
                        .body(Vec::new())
                        .expect("Failed to build Tauri 404 protocol response."),
                }
            });
        }

        builder
            .invoke_handler(move |invoke: Invoke| {
                let command = invoke.message.command().to_string();
                if command == ADMIN_WINDOW_OPEN_COMMAND {
                    let app = invoke.message.webview().app_handle().clone();
                    let admin_url = format!(
                        "{}://localhost/{}/",
                        invoke_web_scheme.trim_end_matches(':'),
                        ADMIN_WEB_MOUNT
                    );
                    open_admin_window(app, admin_url);
                    invoke.resolver.respond(Ok::<(), InvokeError>(()));
                    return true;
                }
                if let Some((_cmd, handler)) = handlers.iter().find(|(c, _)| *c == command) {
                    let handler = std::sync::Arc::clone(handler);
                    let payload = match invoke.message.payload() {
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
                } else if let Some((_cmd, handler)) =
                    json_handlers.iter().find(|(c, _)| *c == command)
                {
                    // Tauri IPC carries no HTTP headers and no conditional
                    // validation; the desktop frontend never sends ETags.
                    let handler = std::sync::Arc::clone(handler);
                    let payload = match invoke.message.payload() {
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
            })
            .run(context)
            .expect("Error launching the Tauri application.");
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
fn open_admin_window(app: AppHandle<Wry>, admin_url: String) {
    if let Some(window) = app.get_webview_window(ADMIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }

    let url: ::tauri::Url = admin_url.parse().expect("Invalid Tauri admin URL.");
    let window = WebviewWindowBuilder::new(
        &app,
        ADMIN_WINDOW_LABEL,
        WebviewUrl::CustomProtocol(url),
    )
    .title("Arachnéa - Administration")
    .inner_size(980.0, 680.0)
    .build();

    if let Err(error) = window {
        tracing::warn!(error = %error, "failed to open the administration window");
    }
}

/// Extracts the request path from a Tauri URI.
///
/// This function strips the custom scheme and host from Tauri URIs to extract
/// the actual request path.
///
/// # Arguments
/// * `uri` - The Tauri HTTP URI.
/// * `web_scheme` - The custom URI scheme to strip.
///
/// # Returns
/// The request path with the scheme and host removed.
fn request_path_from_tauri_uri(uri: &::tauri::http::Uri, web_scheme: &str) -> String {
    let uri = uri.to_string();
    let uri = uri.split(&['?', '#'][..]).next().unwrap_or_default();

    for prefix in [
        format!("{web_scheme}://localhost"),
        format!("https://{web_scheme}.localhost"),
        format!("http://{web_scheme}.localhost"),
    ] {
        if let Some(path) = uri.strip_prefix(&prefix) {
            return path.to_string();
        }
    }

    uri.to_string()
}

/// Splits a stream route path into its components.
///
/// This function parses stream route paths to extract the command name,
/// remaining path, and query parameters.
///
/// # Arguments
/// * `request_path` - The full request path.
/// * `api_prefix` - The API prefix to strip from the path.
///
/// # Returns
/// `Some((command, remaining_path, query))` if the path matches a stream route.
/// `None` if the path doesn't start with the API prefix.
fn split_stream_route<'a>(
    request_path: &'a str,
    api_prefix: &str,
) -> Option<(&'a str, String, String)> {
    let (path_only, query) = match request_path.split_once('?') {
        Some((path, query)) => (path, query.to_string()),
        None => (request_path, String::new()),
    };

    let stream_path = path_only.strip_prefix(api_prefix)?;
    let (command, remaining_path) = match stream_path.split_once('/') {
        Some((command, tail)) => (command, tail.to_string()),
        None => (stream_path, String::new()),
    };

    Some((command, remaining_path, query))
}

/// Normalizes a Tauri web scheme by removing protocol suffixes and slashes.
///
/// # Arguments
/// * `web_scheme` - The web scheme to normalize.
///
/// # Returns
/// The normalized scheme, or the default scheme if the input is empty.
fn normalize_web_scheme(web_scheme: &str) -> String {
    let normalized = web_scheme
        .trim()
        .trim_end_matches("://")
        .trim_end_matches('/')
        .to_string();

    if normalized.is_empty() {
        DEFAULT_TAURI_WEB_SCHEME.to_string()
    } else {
        normalized
    }
}

/// Normalizes a Tauri API prefix by trimming slashes and adding surrounding slashes.
///
/// # Arguments
/// * `api_prefix` - The API prefix to normalize.
///
/// # Returns
/// The normalized prefix with surrounding slashes, or the default prefix if empty.
fn normalize_api_prefix(api_prefix: &str) -> String {
    let normalized = api_prefix.trim().trim_matches('/');

    if normalized.is_empty() {
        DEFAULT_TAURI_API_PREFIX.to_string()
    } else {
        format!("/{normalized}/")
    }
}
