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

use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub use main_thread::{
    global_main_thread_dispatcher, install_global_main_thread_dispatcher, MainThreadContext,
    MainThreadDispatchError, MainThreadDispatchLoop, MainThreadDispatcher, MainThreadEvent,
    MainThreadHandler, MainThreadHandlerId, MainThreadTask, QueuedMainThreadDispatcher,
};
pub use web_assets::{EmbeddedWebAssets, SharedWebAssets};

/// Serialized payload received by controller backends.
pub enum ControlerFunctionInput {
    /// A JSON payload received from Tauri invocations or HTTP POST requests.
    Json(Value),
    /// A raw URL query string received from HTTP GET requests.
    Query(String),
}

/// Type-erased callback stored by controller backends.
pub type SerializedControlerFuture =
    Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'static>>;

/// Type-erased async callback stored by controller backends.
pub type SerializedControlerFunction =
    Arc<dyn Fn(ControlerFunctionInput) -> SerializedControlerFuture + Send + Sync + 'static>;

/// Raw stream payload received by controller backends.
pub struct ControlerStreamInput {
    /// Relative path mounted under the registered stream command.
    pub path: String,
    /// Raw URL query string when present.
    #[allow(dead_code)]
    pub query: String,
    /// HTTP method of the incoming request.
    #[allow(dead_code)]
    pub method: String,
    /// HTTP headers of the incoming request.
    #[allow(dead_code)]
    pub headers: HashMap<String, String>,
    /// Raw request body bytes.
    pub body: Vec<u8>,
    /// Entry-point URL prefix that includes the server base, root mount,
    /// API prefix, and command name (e.g. `http://127.0.0.1:8080/api/proxy`).
    pub entry_point: String,
}

/// Raw stream payload returned by controller backends.
pub struct ControlerStreamOutput {
    /// HTTP status code to return.
    pub status: u16,
    /// Stream response body.
    pub body: Vec<u8>,
    /// MIME type returned to the caller.
    pub content_type: String,
    /// Additional response headers.
    pub headers: HashMap<String, String>,
}

/// Type-erased async callback stored by controller backends for binary stream routes.
pub type StreamControlerFuture =
    Pin<Box<dyn Future<Output = Result<ControlerStreamOutput, String>> + Send + 'static>>;

/// Type-erased binary stream callback stored by controller backends.
pub type StreamControlerFunction =
    Arc<dyn Fn(ControlerStreamInput) -> StreamControlerFuture + Send + Sync + 'static>;

/// Common interface implemented by application controller backends.
pub trait ControlerService {
    /// Registers a type-erased callback under a command name.
    fn register_serialized_function(&mut self, name: &str, fct: SerializedControlerFunction);

    /// Registers a type-erased binary stream callback under a command name.
    fn register_stream_function(&mut self, name: &str, fct: StreamControlerFunction);

    /// Returns the browser-facing path for a binary stream command.
    ///
    /// The returned value is path-only so it remains valid when a REST
    /// deployment is published behind a reverse proxy on the current origin.
    ///
    /// # Arguments
    /// * `command` - Registered stream command name.
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
    fn main_thread_dispatcher(&self) -> Option<Arc<dyn MainThreadDispatcher>> {
        None
    }

    /// Starts the controller event loop.
    fn launch(&mut self);
}

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

fn serialize_output<O>(response: O) -> Result<Value, String>
where
    O: Serialize,
{
    serde_json::to_value(response).map_err(|e| format!("Serialization error: {}", e))
}

/// Convenience extension trait for registering typed controller callbacks.
pub trait ControlerServiceExt: ControlerService {
    /// Registers a strongly typed callback that may fail under a command name.
    ///
    /// The callback input is deserialized from either JSON or a raw query string,
    /// depending on the backend and HTTP method used. Errors are forwarded to the
    /// controller backend as user-visible command errors instead of panicking.
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
