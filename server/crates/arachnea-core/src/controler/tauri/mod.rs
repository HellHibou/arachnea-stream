//! Tauri controller backend.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use ::tauri::{
    ipc::{Invoke, InvokeBody, InvokeError},
    utils::assets::AssetKey,
    Assets,
};
use anyhow::anyhow;
use serde_json::Value;

use super::web_assets::{
    normalize_mount_path, strip_mount_path, EmbeddedWebAssets, SharedWebAssets, WebAssetSource,
};
use super::{
    install_global_main_thread_dispatcher, main_thread::MainThreadHandlerStore,
    ControlerFunctionInput, ControlerService, ControlerStreamInput, ControlerStreamOutput,
    MainThreadContext, MainThreadDispatchError, MainThreadDispatcher, MainThreadEvent,
    MainThreadHandler, MainThreadHandlerId, MainThreadTask, SerializedControlerFunction,
    StreamControlerFunction,
};

const DEFAULT_TAURI_WEB_SCHEME: &str = "arachnea";
const DEFAULT_TAURI_API_PREFIX: &str = "/api/";

/// Configuration required to build a Tauri controller service.
pub struct TauriControlerConfiguration {
    context: ::tauri::Context<::tauri::Wry>,
    web_scheme: String,
    api_prefix: String,
}

impl TauriControlerConfiguration {
    /// Creates a Tauri controller configuration from an application-owned Tauri context.
    ///
    /// # Arguments
    /// * `context` - Generated Tauri context owned by the application crate.
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
    pub fn web_scheme(mut self, web_scheme: impl Into<String>) -> Self {
        self.web_scheme = normalize_web_scheme(&web_scheme.into());
        self
    }

    /// Sets the API path prefix used by binary stream routes.
    ///
    /// # Arguments
    /// * `api_prefix` - Path prefix used before registered binary stream commands.
    pub fn api_prefix(mut self, api_prefix: impl Into<String>) -> Self {
        self.api_prefix = normalize_api_prefix(&api_prefix.into());
        self
    }
}

/// Embedded web assets backed by a Tauri asset bundle.
pub struct TauriEmbeddedWebAssets {
    assets: Arc<dyn Assets<::tauri::Wry>>,
}

impl TauriEmbeddedWebAssets {
    /// Creates an embedded web asset provider from Tauri generated assets.
    ///
    /// # Arguments
    /// * `assets` - Generated Tauri asset bundle owned by the application crate.
    pub fn new(assets: Arc<dyn Assets<::tauri::Wry>>) -> Self {
        Self { assets }
    }
}

impl EmbeddedWebAssets for TauriEmbeddedWebAssets {
    fn get(&self, path: &str) -> Option<Vec<u8>> {
        let key: AssetKey = path.into();
        self.assets.get(&key).map(Cow::into_owned)
    }
}

#[derive(Clone)]
struct TauriWebAssets {
    mount_path: String,
    source: WebAssetSource,
}

impl TauriWebAssets {
    fn from_directory(directory_path: &str, path: &str) -> Self {
        Self {
            mount_path: normalize_mount_path(path),
            source: WebAssetSource::directory(directory_path),
        }
    }

    fn from_embedded(assets: SharedWebAssets, path: &str) -> Self {
        Self {
            mount_path: normalize_mount_path(path),
            source: WebAssetSource::embedded(assets),
        }
    }

    fn window_url(&self, web_scheme: &str) -> String {
        if self.mount_path.is_empty() {
            format!("{web_scheme}://localhost/")
        } else {
            format!("{web_scheme}://localhost/{}/", self.mount_path)
        }
    }

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

struct TauriMainThreadDispatcher {
    app_handle: Mutex<Option<::tauri::AppHandle<::tauri::Wry>>>,
    pending_tasks: Mutex<Vec<MainThreadTask>>,
    handlers: Arc<MainThreadHandlerStore>,
    context: Arc<Mutex<MainThreadContext>>,
}

impl TauriMainThreadDispatcher {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            app_handle: Mutex::new(None),
            pending_tasks: Mutex::new(Vec::new()),
            handlers: Arc::new(MainThreadHandlerStore::default()),
            context: Arc::new(Mutex::new(MainThreadContext::new())),
        })
    }

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
pub struct TauriControlerService {
    context: Option<::tauri::Context<::tauri::Wry>>,
    web_scheme: String,
    api_prefix: String,
    handlers: Vec<(String, SerializedControlerFunction)>,
    stream_handlers: Vec<(String, StreamControlerFunction, String)>,
    web_assets: Option<TauriWebAssets>,
    main_thread_dispatcher: Arc<TauriMainThreadDispatcher>,
}

impl TauriControlerService {
    /// Creates an empty Tauri controller service from an application-owned context.
    ///
    /// # Arguments
    /// * `context` - Generated Tauri context owned by the application crate.
    pub fn new(context: ::tauri::Context<::tauri::Wry>) -> Self {
        Self::with_configuration(TauriControlerConfiguration::new(context))
    }

    /// Creates an empty Tauri controller service from explicit configuration.
    ///
    /// # Arguments
    /// * `configuration` - Tauri controller configuration supplied by the application crate.
    pub fn with_configuration(configuration: TauriControlerConfiguration) -> Self {
        Self {
            context: Some(configuration.context),
            web_scheme: configuration.web_scheme,
            api_prefix: configuration.api_prefix,
            handlers: Vec::new(),
            stream_handlers: Vec::new(),
            web_assets: None,
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
        self.web_assets = Some(TauriWebAssets::from_directory(directory_path, path));
    }

    /// Loads frontend assets from the executable embedded resources.
    ///
    /// # Arguments
    /// * `assets` - Embedded frontend asset provider supplied by the application crate.
    /// * `path` - Relative path under the Tauri root where the assets should be mounted.
    #[allow(dead_code)]
    pub fn register_embedded_web_assets(&mut self, assets: SharedWebAssets, path: &str) {
        self.web_assets = Some(TauriWebAssets::from_embedded(assets, path));
    }

    fn build_context(&mut self) -> ::tauri::Context<::tauri::Wry> {
        let mut context = self
            .context
            .take()
            .expect("Tauri controller launched more than once.");
        if let Some(web_assets) = &self.web_assets {
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

        if let Some(web_assets) = web_assets.clone() {
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
                            tauri::async_runtime::block_on(handler(ControlerStreamInput {
                                path,
                                query,
                                method,
                                headers: headers_map,
                                body: request.body().to_vec(),
                                entry_point,
                            }))
                        });

                        return match response {
                            Ok(ControlerStreamOutput {
                                status,
                                mut body,
                                content_type,
                                headers,
                            }) => {
                                if request.method() == ::tauri::http::Method::HEAD {
                                    body.clear();
                                }
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

                match web_assets.load_request(&request_path) {
                    Ok(asset) => ::tauri::http::Response::builder()
                        .header("Content-Type", asset.mime_type)
                        .body(asset.bytes)
                        .expect("Failed to build Tauri protocol response."),
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

fn normalize_api_prefix(api_prefix: &str) -> String {
    let normalized = api_prefix.trim().trim_matches('/');

    if normalized.is_empty() {
        DEFAULT_TAURI_API_PREFIX.to_string()
    } else {
        format!("/{normalized}/")
    }
}
