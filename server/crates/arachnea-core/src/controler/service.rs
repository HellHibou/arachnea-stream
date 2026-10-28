//! Backend-independent controller registration and lifecycle contract.

use super::{
    JsonControlerFunction, MainThreadDispatcher, RestServerHandle, SerializedControlerFunction,
    SharedWebAssets, StreamControlerFunction,
};
use std::sync::Arc;

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

    /// Returns the hot-reload handle of the REST server, when supported.
    ///
    /// Only the REST backend exposes one; the default implementation returns
    /// `None` (desktop backend). The handle lets the application apply the
    /// port, network mode and entrypoint root settings to the running server
    /// without restarting the process.
    fn rest_server_handle(&self) -> Option<Arc<RestServerHandle>> {
        None
    }
}
