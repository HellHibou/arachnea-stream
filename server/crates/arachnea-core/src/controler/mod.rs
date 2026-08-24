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

pub use rest::shutdown::ShutdownSignal;
pub use rest::tray::{
    gui_available, spawn_tauri_server_tray, ServerTrayConfiguration, ServerTrayFactory,
    ServerTrayHandle,
};

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

/// Output of a header-aware JSON command.
///
/// Extends the plain JSON value with an HTTP status code and additional
/// response headers, used for conditional validation (`ETag` / `304`).
pub struct ControlerJsonOutput {
    /// Serialized JSON payload returned to the caller.
    pub value: Value,
    /// HTTP status code to return (defaults to 200).
    pub status: u16,
    /// Additional response headers (e.g. `ETag`).
    pub headers: HashMap<String, String>,
}

impl From<Value> for ControlerJsonOutput {
    /// Wraps a bare JSON value into a 200 OK output without extra headers.
    fn from(value: Value) -> Self {
        Self {
            value,
            status: 200,
            headers: HashMap::new(),
        }
    }
}

/// Header carrying the client validator (`If-None-Match`) for conditional requests.
pub const HEADER_IF_NONE_MATCH: &str = "if-none-match";

/// Header carrying the server validator echoed back to clients.
pub const HEADER_ETAG: &str = "etag";

/// Context of an incoming controller request.
///
/// Carries the HTTP headers received by the controller backend so registered
/// commands can access them without depending on a specific transport
/// (REST HTTP headers, empty Tauri IPC context, ...).
#[derive(Default, Clone)]
pub struct RequestControlerContext {
    /// Incoming request headers as key-value pairs.
    headers: HashMap<String, String>,
}

impl RequestControlerContext {
    /// Creates a context from the received header map.
    ///
    /// # Arguments
    /// * `headers` - Incoming HTTP request headers as key-value pairs.
    pub fn new(headers: HashMap<String, String>) -> Self {
        Self { headers }
    }

    /// Returns the optional value of the given header.
    ///
    /// The lookup is case-insensitive, matching HTTP header semantics.
    ///
    /// # Arguments
    /// * `name` - Header name to look up.
    pub fn get_header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Input of a header-aware JSON command.
///
/// Carries the deserialized payload alongside the incoming request context
/// so commands can implement conditional validation.
pub struct ControlerJsonInput {
    /// Deserialized command payload (JSON body or URL query string).
    pub payload: ControlerFunctionInput,
    /// Context of the incoming request (received HTTP headers).
    pub context: RequestControlerContext,
}

impl ControlerJsonInput {
    /// Returns the `If-None-Match` request header value, when present.
    pub fn if_none_match(&self) -> Option<&str> {
        self.context.get_header(HEADER_IF_NONE_MATCH)
    }
}

/// Type-erased future resolving to a header-aware JSON output or error string.
pub type JsonControlerFuture =
    Pin<Box<dyn Future<Output = Result<ControlerJsonOutput, String>> + Send + 'static>>;

/// Type-erased async callback stored by controller backends for header-aware
/// JSON commands.
pub type JsonControlerFunction =
    Arc<dyn Fn(ControlerJsonInput) -> JsonControlerFuture + Send + Sync + 'static>;

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

    /// Registers a type-erased header-aware JSON callback under a command name.
    ///
    /// Backends without HTTP semantics (e.g. Tauri IPC) receive empty request
    /// headers and ignore the status/headers of the output.
    ///
    /// # Arguments
    /// * `name` - The command name to register the function under.
    /// * `fct` - The header-aware JSON function to register.
    fn register_json_function(&mut self, name: &str, fct: JsonControlerFunction);

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
            let value = query_string_to_json_value(&query)?;
            serde_json::from_value(value).map_err(|e| format!("Deserialization error: {}", e))
        }
    }
}

/// Parses a raw URL query string into a JSON object value.
///
/// Each parameter value is parsed as JSON when it is a valid JSON value;
/// otherwise it remains a string. This lets GET clients send arrays, objects,
/// quoted strings, numbers, booleans, and null values through the query
/// string while preserving unquoted text values.
///
/// Repeated keys are collected into a JSON array of their raw string values.
///
/// # Arguments
///
/// * `query` - Raw URL query string (without the leading `?`).
///
/// # Returns
///
/// A JSON object mapping parameter names to their decoded values.
///
/// # Errors
///
/// Returns an error when the query string cannot be percent-decoded.
fn query_string_to_json_value(query: &str) -> Result<Value, String> {
    use serde_json::Map;

    let pairs = serde_urlencoded::from_str::<Vec<(String, String)>>(query)
        .map_err(|e| format!("Deserialization error: {}", e))?;
    let mut object = Map::new();

    for (key, raw_value) in pairs {
        let value = serde_json::from_str::<Value>(&raw_value)
            .unwrap_or_else(|_| Value::String(raw_value));

        match object.get_mut(&key) {
            Some(existing) => {
                if let Some(array) = existing.as_array_mut() {
                    array.push(value);
                } else {
                    let previous = existing.take();
                    *existing = Value::Array(vec![previous, value]);
                }
            }
            None => {
                object.insert(key, value);
            }
        }
    }

    Ok(Value::Object(object))
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
    /// depending on the backend and HTTP method used. The callback also receives
    /// the [`RequestControlerContext`] built by the controller backend. Errors are
    /// forwarded to the controller backend as user-visible command errors instead
    /// of panicking.
    ///
    /// When the callback produces a global ETag and the client's
    /// `If-None-Match` header matches it, the backend answers `304 Not Modified`
    /// with no body; otherwise the serialized payload is returned with an `ETag`
    /// response header when one was produced.
    ///
    /// # Type Parameters
    /// * `I` - Input type that implements `DeserializeOwned`.
    /// * `O` - Output type that implements `Serialize`.
    /// * `F` - Function type taking input and the request context.
    /// * `Fut` - Future type that resolves to `Result<(O, Option<String>), E>`.
    /// * `E` - Error type that implements `Display`.
    ///
    /// # Arguments
    /// * `name` - The command name to register under.
    /// * `fct` - The function to register.
    fn register_result_function<I, O, F, Fut, E>(&mut self, name: &str, fct: F)
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + Send + 'static,
        F: Fn(I, RequestControlerContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(O, Option<String>), E>> + Send + 'static,
        E: std::fmt::Display,
    {
        let fct = Arc::new(fct);
        let handler = move |input: ControlerJsonInput| -> JsonControlerFuture {
            let fct = Arc::clone(&fct);
            Box::pin(async move {
                let if_none_match = normalize_etag(input.if_none_match());
                let payload = deserialize_input(input.payload)?;
                let (response, etag) = fct(payload, input.context)
                    .await
                    .map_err(|e| e.to_string())?;

                if let Some(etag) = etag.as_ref() {
                    if if_none_match.as_deref() == Some(etag.as_str()) {
                        return Ok(ControlerJsonOutput {
                            value: Value::Null,
                            status: 304,
                            headers: HashMap::from([(HEADER_ETAG.to_string(), format!("\"{etag}\""))]),
                        });
                    }
                }

                let mut output = ControlerJsonOutput::from(serialize_output(response)?);
                if let Some(etag) = etag {
                    output.headers.insert(HEADER_ETAG.to_string(), format!("\"{etag}\""));
                }
                Ok(output)
            })
        };

        self.register_json_function(name, Arc::new(handler));
    }

    /// Registers an async callback bound to shared state without per-call boilerplate.
    ///
    /// See [`ControlerServiceExt::register_result_function`] for the
    /// conditional-validation contract.
    ///
    /// # Type Parameters
    /// * `S` - State type that implements `Send + Sync`.
    /// * `I` - Input type that implements `DeserializeOwned`.
    /// * `O` - Output type that implements `Serialize`.
    /// * `F` - Function type that takes state, the request context, and input.
    /// * `Fut` - Future type that resolves to `Result<(O, Option<String>), E>`.
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
        F: Fn(Arc<S>, RequestControlerContext, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(O, Option<String>), E>> + Send + 'static,
        E: std::fmt::Display,
    {
        self.register_result_function(
            name,
            move |input: I, context: RequestControlerContext| {
                let state = Arc::clone(&state);
                fct(state, context, input)
            },
        );
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

/// Normalizes an `If-None-Match` header value for comparison.
///
/// Strips surrounding quotes and the weak-validator `W/` prefix so both the
/// incoming header and the produced ETag compare as bare opaque strings.
fn normalize_etag(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    let value = value.strip_prefix("W/").unwrap_or(value);
    let value = value.trim_matches('"');
    (!value.is_empty()).then(|| value.to_string())
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

/// Which client connections the REST server accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ServerNetworkMode {
    /// Loopback only: the server binds to `127.0.0.1`.
    Local,
    /// Loopback plus the machine's local networks, detected from the interface
    /// netmasks. The server binds to all interfaces and rejects clients whose
    /// address does not belong to a local network range.
    #[default]
    Private,
    /// Any network: the server binds to all interfaces and accepts every client.
    Public,
}

impl std::str::FromStr for ServerNetworkMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "local" => Ok(Self::Local),
            "private" => Ok(Self::Private),
            "public" => Ok(Self::Public),
            _ => Err(format!(
                "expected one of `local`, `private` or `public`, got `{value}`"
            )),
        }
    }
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

    /// Which client connections the REST server accepts.
    ///
    /// Defaults to [`ServerNetworkMode::Private`], which binds to all
    /// interfaces and only accepts clients belonging to a local network range.
    pub network_mode: ServerNetworkMode,
    
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

    /// Optional factory creating the server tray icon in server mode.
    ///
    /// When set and a GUI is available, the REST controller shows a tray icon.
    pub server_tray_factory: Option<Arc<dyn ServerTrayFactory>>,

    /// Whether a server tray icon should be shown in server mode.
    ///
    /// When `true` a tray factory is installed and the REST controller shows a
    /// tray icon if a GUI is available. Set to `false` to force-disable the tray
    /// even on a graphical environment.
    pub tray_enabled: bool,
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
            network_mode: ServerNetworkMode::default(),
            entrypoint_root,
            entrypoint_api,
            web_scheme,
            api_prefix,
            server_tray_factory: None,
            tray_enabled: true,
        }
    }

    /// Sets the factory creating the server tray icon in server mode.
    ///
    /// # Arguments
    /// * `server_tray_factory` - Application-supplied tray factory.
    ///
    /// # Returns
    /// The modified options for chaining.
    pub fn with_server_tray_factory(
        mut self,
        server_tray_factory: Arc<dyn ServerTrayFactory>,
    ) -> Self {
        self.server_tray_factory = Some(server_tray_factory);
        self
    }

    /// Sets whether a server tray icon should be shown in server mode.
    ///
    /// # Arguments
    /// * `tray_enabled` - `true` to show a tray icon when a GUI is available,
    ///   `false` to force-disable it.
    ///
    /// # Returns
    /// The modified options for chaining.
    pub fn with_tray_enabled(mut self, tray_enabled: bool) -> Self {
        self.tray_enabled = tray_enabled;
        self
    }
}

impl Default for CoreApplicationOptions {
    fn default() -> Self {
        Self {
            application_mode: None,
            server_port: Some(DEFAULT_SERVER_PORT),
            network_mode: ServerNetworkMode::default(),
            entrypoint_root: None,
            entrypoint_api: Some(DEFAULT_TAURI_API_PREFIX.to_string()),
            web_scheme: Some(DEFAULT_TAURI_WEB_SCHEME.to_string()),
            api_prefix: None,
            server_tray_factory: None,
            tray_enabled: true,
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
    /// The generated Tauri context routed to the desktop backend in desktop
    /// mode. `None` in server mode, where the context is owned by the server
    /// tray instead and this backend never builds a desktop app.
    pub context: Option<::tauri::Context<::tauri::Wry>>,
    /// Embedded frontend assets backing the Tauri custom protocol.
    pub web_assets: SharedWebAssets,
}

/// Empty Tauri asset provider placed inside the generated context once its real
/// assets are moved into the shared web asset provider.
///
/// The Tauri context is generated exactly once per binary (macOS embeds the
/// `Info.plist` as a single `_EMBED_INFO_PLIST` symbol, so `generate_context!`
/// cannot be expanded more than once). The desktop app serves the frontend
/// through a custom protocol backed by the shared assets and the server tray is
/// headless, so neither reads assets through this placeholder.
struct EmptyTauriAssets;

impl ::tauri::Assets<::tauri::Wry> for EmptyTauriAssets {
    fn get(
        &self,
        _key: &::tauri::utils::assets::AssetKey,
    ) -> Option<std::borrow::Cow<'_, [u8]>> {
        None
    }

    fn iter(&self) -> Box<::tauri::utils::assets::AssetsIter<'_>> {
        Box::new(
            std::iter::empty::<(
                std::borrow::Cow<'static, str>,
                std::borrow::Cow<'static, [u8]>,
            )>(),
        )
    }

    fn csp_hashes(
        &self,
        _html_path: &::tauri::utils::assets::AssetKey,
    ) -> Box<dyn Iterator<Item = ::tauri::utils::assets::CspHash<'_>> + '_> {
        Box::new(std::iter::empty())
    }
}

/// Wraps Tauri generated assets into the shared embedded web asset provider.
///
/// The real embedded assets are extracted out of the supplied context (which is
/// generated only once, see [`EmptyTauriAssets`]) so they can be shared by
/// value, leaving an empty placeholder behind for the context's own copy.
///
/// # Arguments
/// * `context` - Generated Tauri context whose asset bundle is extracted.
///
/// # Returns
/// A `SharedWebAssets` provider backed by the Tauri asset bundle.
pub fn desktop_embedded_web_assets(
    context: &mut ::tauri::Context<::tauri::Wry>,
) -> SharedWebAssets {
    let original_assets = context.set_assets(Box::new(EmptyTauriAssets));
    let assets: Arc<dyn ::tauri::Assets<::tauri::Wry>> = Arc::from(original_assets);
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
            match options.network_mode {
                ServerNetworkMode::Local => {}
                ServerNetworkMode::Private => {
                    configuration =
                        configuration.server_ip(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
                    configuration = configuration.allowed_networks(
                        crate::controler::rest::local_networks(),
                    );
                }
                ServerNetworkMode::Public => {
                    configuration =
                        configuration.server_ip(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
                }
            }
            if let Some(entrypoint_root) = &options.entrypoint_root {
                configuration = configuration.entrypoint_root(entrypoint_root);
            }
            if let Some(entrypoint_api) = &options.entrypoint_api {
                configuration = configuration.entrypoint_api(entrypoint_api);
            }
            if let Some(tray_factory) = &options.server_tray_factory {
                configuration = configuration
                    .server_tray_factory(Arc::clone(tray_factory))
                    .tray_enabled(true);
            }

            Box::new(RestControlerService::new(configuration))
        } else {
            let context = desktop.context.expect(
                "desktop mode requires the Tauri context routed by the application crate",
            );
            Box::new(tauri_controler_service(
                context,
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
/// The Tauri context is generated **exactly once** per binary: on macOS the
/// embedded `Info.plist` is exposed as a single `_EMBED_INFO_PLIST` symbol, so
/// `tauri::generate_context!()` cannot be expanded more than once. The real
/// assets are extracted into the shared web asset provider and the context is
/// routed to the single runtime consumer of the effective application mode:
/// the server tray in [`ApplicationMode::Server`] or the Tauri controller in
/// [`ApplicationMode::Desktop`].
///
/// When [`CoreApplicationOptions::tray_enabled`] is true and no tray factory is
/// already present on the options, the macro installs a
/// [`ServerTrayIconService`] built from the caller's Tauri context. The tray
/// icon falls back to the context's embedded default window icon (derived from
/// `bundle.icon` in `tauri.conf.json`).
///
/// # Arguments
/// * `$options` - Parsed [`CoreApplicationOptions`].
///
/// # Returns
/// A boxed `ControlerService` ready for `register_service` and `launch`.
#[macro_export]
macro_rules! create_application_controler {
    ($options:expr) => {{
        let mut options = $options;

        // Resolve the effective application mode with the same defaults as
        // `create_application_controler_from_config`, so the single generated
        // Tauri context is handed to the one component that will consume it.
        let is_server = match &options.application_mode {
            ::std::option::Option::Some(mode) => {
                ::core::matches!(mode, $crate::controler::ApplicationMode::Server)
            }
            ::std::option::Option::None => {
                #[cfg(not(debug_assertions))]
                {
                    false // Release default: desktop.
                }
                #[cfg(debug_assertions)]
                {
                    true // Debug default: server.
                }
            }
        };

        let mut context = ::tauri::generate_context!();
        let web_assets = $crate::controler::desktop_embedded_web_assets(&mut context);

        let desktop_context = if is_server {
            if options.tray_enabled && options.server_tray_factory.is_none() {
                options.server_tray_factory = Some(::std::sync::Arc::new(
                    $crate::controler::rest::tray::ServerTrayIconService::new(context),
                ));
            }
            // In server mode the context is owned by the server tray; the
            // desktop backend is never built so it receives no context.
            ::std::option::Option::None
        } else {
            // In desktop mode the Tauri controller owns the whole context.
            ::std::option::Option::Some(context)
        };

        $crate::controler::create_application_controler_from_config(
            options,
            $crate::controler::DesktopApplicationConfig {
                context: desktop_context,
                web_assets,
            },
        )
    }};
}
