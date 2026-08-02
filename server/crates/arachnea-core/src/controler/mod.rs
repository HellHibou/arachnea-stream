//! Controller abstractions shared by the REST and Tauri backends.

/// Main-thread dispatch primitives shared by controller backends.
pub mod main_thread;
/// REST controller backend based on Warp.
pub mod rest;
/// Tauri controller backend.
pub mod tauri;
mod web_assets;

use std::sync::Arc;
use std::{future::Future, pin::Pin};

use bytes::Bytes;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub use main_thread::{
    global_main_thread_dispatcher, install_global_main_thread_dispatcher, MainThreadContext,
    MainThreadDispatchError, MainThreadDispatchLoop, MainThreadDispatcher, MainThreadEvent,
    MainThreadHandler, MainThreadHandlerId, MainThreadTask, QueuedMainThreadDispatcher,
};
pub use web_assets::{EmbeddedWebAssets, SharedWebAssets};

use crate::controler::rest::{RestControlerConfiguration, RestControlerService};
use crate::controler::tauri::{
    TauriControlerConfiguration, TauriControlerService, TauriEmbeddedWebAssets,
};

/// Default port used by the REST controller when no CLI override is provided.
pub const DEFAULT_SERVER_PORT: u16 = 8080;

/// Custom URI scheme used by the desktop frontend.
pub const DEFAULT_TAURI_WEB_SCHEME: &str = "arachnea-core";

/// API prefix used by the desktop binary stream routes.
pub const DEFAULT_TAURI_API_PREFIX: &str = "api";

/// Serialized payload received by controller backends.
///
/// This enum represents the different types of input that controller functions
/// can receive from various sources.
pub enum ControlerFunctionInput {
    /// A JSON payload received from Tauri invocations or HTTP POST requests.
    ///
    /// This variant contains a parsed JSON value that can be deserialized
    /// into specific types.
    Json(Value),
    /// A raw URL query string received from HTTP GET requests.
    ///
    /// This variant contains the raw query string that can be parsed
    /// using URL decoding.
    Query(String),
}

/// Type-erased callback stored by controller backends.
///
/// This type represents a boxed future that resolves to a JSON value or error string.
pub type SerializedControlerFuture =
    Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'static>>;

/// Type-erased async callback stored by controller backends.
///
/// This type represents a function that takes controller input and returns
/// a serialized future.
pub type SerializedControlerFunction =
    Arc<dyn Fn(ControlerFunctionInput) -> SerializedControlerFuture + Send + Sync + 'static>;

/// Raw stream payload received by controller backends.
///
/// This struct represents the complete HTTP request information for stream
/// endpoints that handle binary data.
pub struct ControlerStreamInput {
    /// Relative path mounted under the registered stream command.
    ///
    /// This field contains the path portion of the URL after the command prefix.
    pub path: String,
    /// Raw URL query string when present.
    ///
    /// This field contains the query parameters from the URL.
    #[allow(dead_code)]
    pub query: String,
    /// HTTP method of the incoming request.
    ///
    /// This field contains the HTTP method (GET, POST, etc.).
    #[allow(dead_code)]
    pub method: String,
    /// HTTP headers of the incoming request.
    ///
    /// This field contains all HTTP headers as key-value pairs.
    #[allow(dead_code)]
    pub headers: HashMap<String, String>,
    /// Raw request body bytes.
    ///
    /// This field contains the complete request body as binary data.
    pub body: Vec<u8>,
    /// Entry-point URL prefix that includes the server base, root mount,
    /// API prefix, and command name (e.g. `http://127.0.0.1:8080/api/proxy`).
    ///
    /// This field can be used to construct absolute URLs for responses.
    pub entry_point: String,
    /// When `true`, stream handlers should buffer the full response body
    /// instead of returning a streaming response. This is set by controller
    /// backends that do not support native streaming (e.g. Tauri).
    ///
    /// Handlers that produce `ResponseBody::Streamed` may check this flag
    /// and force body buffering to avoid streaming-specific issues such as
    /// strict chunked encoding parsing on the streaming path.
    #[allow(dead_code)]
    pub force_buffer_response: bool,
}

/// Polymorphic response body that can be either fully buffered or streamed.
pub enum ResponseBody {
    /// Entire body loaded in memory (used when post-actions need body modification).
    Buffered(Vec<u8>),
    /// Streaming body transferred as continuous chunks.
    Streamed(Pin<Box<dyn futures::Stream<Item = Result<Bytes, std::io::Error>> + Send>>),
}

/// Raw stream payload returned by controller backends.
///
/// This struct represents the complete HTTP response for stream endpoints.
pub struct ControlerStreamOutput {
    /// HTTP status code to return.
    ///
    /// This field contains the numeric HTTP status code (e.g., 200, 404, 500).
    pub status: u16,
    /// Stream response body.
    ///
    /// This field contains the binary response data to send to the client.
    pub body: ResponseBody,
    /// MIME type returned to the caller.
    ///
    /// This field specifies the content type of the response.
    pub content_type: String,
    /// Additional response headers.
    ///
    /// This field contains any additional HTTP headers to include in the response.
    pub headers: HashMap<String, String>,
}

/// Type-erased async callback stored by controller backends for binary stream routes.
///
/// This type represents a boxed future that resolves to a stream output or error string.
pub type StreamControlerFuture =
    Pin<Box<dyn Future<Output = Result<ControlerStreamOutput, String>> + Send + 'static>>;

/// Type-erased binary stream callback stored by controller backends.
///
/// This type represents a function that takes stream input and returns
/// a stream future.
pub type StreamControlerFunction =
    Arc<dyn Fn(ControlerStreamInput) -> StreamControlerFuture + Send + Sync + 'static>;

/// Common interface implemented by application controller backends.
///
/// This trait defines the contract that all controller backends (REST, Tauri, etc.)
/// must implement to provide a consistent API for registering functions and
/// managing the application lifecycle.
pub trait ControlerService {
    /// Registers a type-erased callback under a command name.
    ///
    /// # Arguments
    /// * `name` - The command name to register the function under.
    /// * `fct` - The serialized function to register.
    fn register_serialized_function(&mut self, name: &str, fct: SerializedControlerFunction);

    /// Registers a type-erased binary stream callback under a command name.
    ///
    /// # Arguments
    /// * `name` - The command name to register the stream function under.
    /// * `fct` - The stream function to register.
    fn register_stream_function(&mut self, name: &str, fct: StreamControlerFunction);

    /// Returns the browser-facing path for a binary stream command.
    ///
    /// The returned value is path-only so it remains valid when a REST
    /// deployment is published behind a reverse proxy on the current origin.
    ///
    /// # Arguments
    /// * `command` - Registered stream command name.
    ///
    /// # Returns
    /// The public path for the stream command.
    fn stream_public_path(&self, command: &str) -> String;

    /// Mounts frontend assets loaded directly from a directory on disk.
    ///
    /// # Arguments
    /// * `directory_path` - Directory containing the frontend assets to expose.
    /// * `path` - Relative path under the controller root where the assets should be mounted.
    #[allow(dead_code)]
    fn register_web_directory(&mut self, directory_path: &str, path: &str);

    /// Mounts frontend assets embedded in the executable.
    ///
    /// # Arguments
    /// * `assets` - Embedded frontend asset provider supplied by the application crate.
    /// * `path` - Relative path under the controller root where the assets should be mounted.
    #[allow(dead_code)]
    fn register_embedded_web_assets(&mut self, assets: SharedWebAssets, path: &str);

    /// Returns the main-thread dispatcher supplied by this controller backend, when available.
    ///
    /// # Returns
    /// `Some(Arc<dyn MainThreadDispatcher>)` if the backend supports main-thread dispatching.
    /// `None` if the backend does not support main-thread dispatching.
    fn main_thread_dispatcher(&self) -> Option<Arc<dyn MainThreadDispatcher>> {
        None
    }

    /// Starts the controller event loop.
    ///
    /// This method begins processing requests and typically blocks until the
    /// application is shut down.
    fn launch(&mut self);
}

/// Deserializes controller input into a specific type.
///
/// # Type Parameters
/// * `I` - The type to deserialize into, must implement `DeserializeOwned`.
///
/// # Arguments
/// * `payload` - The controller function input to deserialize.
///
/// # Returns
/// `Ok(I)` if deserialization succeeds.
/// `Err(String)` if deserialization fails.
fn deserialize_input<I>(payload: ControlerFunctionInput) -> Result<I, String>
where
    I: DeserializeOwned,
{
    match payload {
        ControlerFunctionInput::Json(value) => {
            serde_json::from_value(value).map_err(|e| format!("Deserialization error: {}", e))
        }
        ControlerFunctionInput::Query(query) => {
            serde_urlencoded::from_str(&query).map_err(|e| format!("Deserialization error: {}", e))
        }
    }
}

/// Serializes output into a JSON value.
///
/// # Type Parameters
/// * `O` - The type to serialize, must implement `Serialize`.
///
/// # Arguments
/// * `response` - The response to serialize.
///
/// # Returns
/// `Ok(Value)` if serialization succeeds.
/// `Err(String)` if serialization fails.
fn serialize_output<O>(response: O) -> Result<Value, String>
where
    O: Serialize,
{
    serde_json::to_value(response).map_err(|e| format!("Serialization error: {}", e))
}

/// Convenience extension trait for registering typed controller callbacks.
///
/// This trait provides ergonomic methods for registering strongly-typed
/// controller functions without manual serialization/deserialization boilerplate.
pub trait ControlerServiceExt: ControlerService {
    /// Registers a strongly typed callback that may fail under a command name.
    ///
    /// The callback input is deserialized from either JSON or a raw query string,
    /// depending on the backend and HTTP method used. Errors are forwarded to the
    /// controller backend as user-visible command errors instead of panicking.
    ///
    /// # Type Parameters
    /// * `I` - Input type that implements `DeserializeOwned`.
    /// * `O` - Output type that implements `Serialize`.
    /// * `F` - Function type that takes input and returns a future.
    /// * `Fut` - Future type that resolves to `Result<O, E>`.
    /// * `E` - Error type that implements `Display`.
    ///
    /// # Arguments
    /// * `name` - The command name to register under.
    /// * `fct` - The function to register.
    fn register_result_function<I, O, F, Fut, E>(&mut self, name: &str, fct: F)
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + Send + 'static,
        F: Fn(I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, E>> + Send + 'static,
        E: std::fmt::Display,
    {
        let fct = Arc::new(fct);
        let handler = move |payload: ControlerFunctionInput| -> SerializedControlerFuture {
            let fct = Arc::clone(&fct);
            Box::pin(async move {
                let input = deserialize_input(payload)?;

                let response = fct(input).await.map_err(|e| e.to_string())?;
                serialize_output(response)
            })
        };

        self.register_serialized_function(name, Arc::new(handler));
    }

    /// Registers an async callback bound to shared state without per-call boilerplate.
    ///
    /// # Type Parameters
    /// * `S` - State type that implements `Send + Sync`.
    /// * `I` - Input type that implements `DeserializeOwned`.
    /// * `O` - Output type that implements `Serialize`.
    /// * `F` - Function type that takes state and input.
    /// * `Fut` - Future type that resolves to `Result<O, E>`.
    /// * `E` - Error type that implements `Display`.
    ///
    /// # Arguments
    /// * `name` - The command name to register under.
    /// * `state` - The shared state to bind to the function.
    /// * `fct` - The function to register.
    fn register_result_function_with_state<S, I, O, F, Fut, E>(
        &mut self,
        name: &str,
        state: Arc<S>,
        fct: F,
    ) where
        S: Send + Sync + 'static,
        I: DeserializeOwned + Send + 'static,
        O: Serialize + Send + 'static,
        F: Fn(Arc<S>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, E>> + Send + 'static,
        E: std::fmt::Display,
    {
        self.register_result_function(name, move |input: I| {
            let state = Arc::clone(&state);
            fct(state, input)
        });
    }

    /// Registers an async binary stream callback bound to shared state without per-call boilerplate.
    ///
    /// # Type Parameters
    /// * `S` - State type that implements `Send + Sync`.
    /// * `F` - Function type that takes state and stream input.
    /// * `Fut` - Future type that resolves to `Result<ControlerStreamOutput, E>`.
    /// * `E` - Error type that implements `Display`.
    ///
    /// # Arguments
    /// * `name` - The command name to register under.
    /// * `state` - The shared state to bind to the function.
    /// * `fct` - The function to register.
    fn register_stream_function_with_state<S, F, Fut, E>(
        &mut self,
        name: &str,
        state: Arc<S>,
        fct: F,
    ) where
        S: Send + Sync + 'static,
        F: Fn(Arc<S>, ControlerStreamInput) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ControlerStreamOutput, E>> + Send + 'static,
        E: std::fmt::Display,
    {
        let fct = Arc::new(fct);
        let handler = move |input: ControlerStreamInput| -> StreamControlerFuture {
            let state = Arc::clone(&state);
            let fct = Arc::clone(&fct);
            Box::pin(async move { fct(state, input).await.map_err(|e| e.to_string()) })
        };

        self.register_stream_function(name, Arc::new(handler));
    }
}

impl<T: ControlerService + ?Sized> ControlerServiceExt for T {}



/// Runtime mode the application controller runs under.
#[derive(PartialEq)]
pub enum ApplicationMode {
    /// Desktop application mode backed by the Tauri controller.
    Desktop,
    /// Headless HTTP server mode backed by the REST controller.
    Server,
}

/// Default runtime mode used when no CLI mode flag is provided.
///
/// In release builds the backend defaults to server mode; in debug builds it
/// defaults to desktop mode.
#[macro_export]
macro_rules! default_application_mode {
    () => {
        if cfg!(debug_assertions) {
            return ApplicationMode::Desktop;
        } else {
            return ApplicationMode::Server;
        }
    };
}


/// Runtime options parsed from command line arguments.
pub struct CoreApplicationOptions {

    /// Backend mode (desktop or server).
    pub application_mode: Option<ApplicationMode>,
   
    /// REST server port used in server mode.
    ///
    /// Falls back to [`DEFAULT_SERVER_PORT`] when `None`.
    pub server_port: Option<u16>,
    
    /// Optional public root path prefix for server mode.
    pub entrypoint_root: Option<String>,
    
    /// Optional public API path segment for server mode.
    pub entrypoint_api: Option<String>,
    
    /// Custom URI scheme used by the desktop frontend.
    ///
    /// Falls back to [`DEFAULT_TAURI_WEB_SCHEME`] when `None`.
    pub web_scheme: Option<String>,
    
    /// API path prefix used by the desktop binary stream routes.
    ///
    /// Falls back to [`DEFAULT_TAURI_API_PREFIX`] when `None`.
    pub api_prefix: Option<String>,
}

impl CoreApplicationOptions {
    
    /// Creates runtime options for the application controller.
    ///
    /// # Arguments
    /// * `application_mode` - Backend mode (desktop or server).
    /// * `server_port` - Optional REST server port used in server mode.
    /// * `entrypoint_root` - Optional public root path prefix for server mode.
    /// * `entrypoint_api` - Optional public API path segment for server mode.
    /// * `web_scheme` - Optional custom URI scheme for the desktop frontend.
    /// * `api_prefix` - Optional API path prefix for desktop binary stream routes.
    pub fn new(
        application_mode: Option<ApplicationMode>,
        server_port: Option<u16>,
        entrypoint_root: Option<String>,
        entrypoint_api: Option<String>,
        web_scheme: Option<String>,
        api_prefix: Option<String>,
    ) -> Self {
        Self {
            application_mode,
            server_port,
            entrypoint_root,
            entrypoint_api,
            web_scheme,
            api_prefix,
        }
    }
}

impl Default for CoreApplicationOptions {
    fn default() -> Self {
        Self {
            application_mode: None,
            server_port: Some(DEFAULT_SERVER_PORT),
            entrypoint_root: None,
            entrypoint_api: Some(DEFAULT_TAURI_API_PREFIX.to_string()),
            web_scheme: Some(DEFAULT_TAURI_WEB_SCHEME.to_string()),
            api_prefix: None,
        }
    }
}

/// Configuration required to build the desktop (Tauri) controller backend.
///
/// This groups the application-owned pieces that only the application crate can
/// produce: the generated Tauri context and its embedded web assets. It is kept
/// separate from [`CoreApplicationOptions`] because it is generated for each
/// application crate rather than parsed from runtime options.
pub struct DesktopApplicationConfig {
    /// The generated Tauri context owned by the application crate.
    pub context: ::tauri::Context<::tauri::Wry>,
    /// Embedded frontend assets backing the Tauri custom protocol.
    pub web_assets: SharedWebAssets,
}

/// Wraps Tauri generated assets into the shared embedded web asset provider.
///
/// # Arguments
/// * `context` - Generated Tauri context whose asset bundle is reused.
///
/// # Returns
/// A `SharedWebAssets` provider backed by the Tauri asset bundle.
pub fn desktop_embedded_web_assets(context: ::tauri::Context<::tauri::Wry>) -> SharedWebAssets {
    let assets: Arc<dyn ::tauri::Assets<::tauri::Wry>> = Arc::from(context.assets);
    Arc::new(TauriEmbeddedWebAssets::new(assets))
}

fn tauri_controler_service(
    context: ::tauri::Context<::tauri::Wry>,
    web_scheme: Option<String>,
    api_prefix: Option<String>,
) -> TauriControlerService {
    let web_scheme = web_scheme.unwrap_or_else(|| DEFAULT_TAURI_WEB_SCHEME.to_string());
    let api_prefix = api_prefix.unwrap_or_else(|| DEFAULT_TAURI_API_PREFIX.to_string());
    TauriControlerService::with_configuration(
        TauriControlerConfiguration::new(context)
            .web_scheme(web_scheme)
            .api_prefix(api_prefix),
    )
}

/// Creates the application controller from explicit runtime and desktop options.
///
/// Desktop mode builds a Tauri controller and embeds the frontend web assets;
/// server mode builds a REST controller and mounts the embedded assets under
/// the optional entry point prefixes.
///
/// # Arguments
/// * `options` - Runtime options selecting the backend mode, port and overrides.
/// * `desktop` - Desktop-specific generated context and embedded assets.
///
/// # Returns
/// A boxed `ControlerService` ready for `register_service` and `launch`.
pub fn create_application_controler_from_config(
    options: CoreApplicationOptions,
    desktop: DesktopApplicationConfig,
) -> Box<dyn ControlerService> {

    #[cfg(not(debug_assertions))] // Release mode defaults.
    let application_mode = options.application_mode.unwrap_or(ApplicationMode::Desktop);
 
    #[cfg(debug_assertions)] // Debug mode defaults.
    let application_mode = options.application_mode.unwrap_or(ApplicationMode::Server);

    let mut controler: Box<dyn ControlerService> =
        if application_mode == ApplicationMode::Server {
            let server_port = options.server_port.unwrap_or(DEFAULT_SERVER_PORT);
            let mut configuration =
                RestControlerConfiguration::default().server_port(server_port);
            if let Some(entrypoint_root) = &options.entrypoint_root {
                configuration = configuration.entrypoint_root(entrypoint_root);
            }
            if let Some(entrypoint_api) = &options.entrypoint_api {
                configuration = configuration.entrypoint_api(entrypoint_api);
            }

            Box::new(RestControlerService::new(configuration))
        } else {
            Box::new(tauri_controler_service(
                desktop.context,
                options.web_scheme,
                options.api_prefix,
            ))
        };

    controler.register_embedded_web_assets(desktop.web_assets, "");

    controler
}

/// Builds the application controller from parsed runtime options.
///
/// This macro expands [`create_application_controler_from_config`] with a
/// [`DesktopApplicationConfig`] generated from the caller crate's own Tauri
/// configuration. Because `macro_rules!` expands at the call site, the embedded
/// `::tauri::generate_context!()` is resolved inside the caller crate, where
/// `tauri.conf.json` and `tauri-build` live. The values of `web_scheme` and
/// `api_prefix` come from the supplied [`CoreApplicationOptions`].
///
/// # Arguments
/// * `$options` - Parsed [`CoreApplicationOptions`].
///
/// # Returns
/// A boxed `ControlerService` ready for `register_service` and `launch`.
#[macro_export]
macro_rules! create_application_controler {
    ($options:expr) => {{
        $crate::controler::create_application_controler_from_config(
            $options,
            $crate::controler::DesktopApplicationConfig {
                context: ::tauri::generate_context!(),
                web_assets: $crate::controler::desktop_embedded_web_assets(
                    ::tauri::generate_context!(),
                ),
            },
        )
    }};
}

