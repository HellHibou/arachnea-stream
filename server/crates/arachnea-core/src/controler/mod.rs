//! Controller abstractions shared by the REST and Tauri backends.

/// Main-thread dispatch primitives shared by controller backends.
pub mod main_thread;
/// Options shared by controller backends.
pub mod options;
/// REST controller backend based on Warp.
pub mod rest;
/// Tauri controller backend.
pub mod tauri;
mod web_assets;

mod application;
mod contracts;
mod macros;
mod registration;
mod rest_settings;
mod service;

pub use application::{
    create_application_controler_from_config, desktop_embedded_web_assets, DesktopApplicationConfig,
};
pub use contracts::{
    ControlerFunctionInput, ControlerJsonInput, ControlerJsonOutput, ControlerStreamInput,
    ControlerStreamOutput, JsonControlerFunction, JsonControlerFuture, RequestControlerContext,
    ResponseBody, SerializedControlerFunction, SerializedControlerFuture, StreamControlerFunction,
    StreamControlerFuture,
};
use options::ServerNetworkMode;
pub(crate) use registration::normalize_etag;
pub use registration::{deserialize_input, ControlerServiceExt};
pub use rest_settings::rest_settings_source;
pub use service::ControlerService;

pub use main_thread::{
    global_main_thread_dispatcher, install_global_main_thread_dispatcher, MainThreadContext,
    MainThreadDispatchError, MainThreadDispatchLoop, MainThreadDispatcher, MainThreadEvent,
    MainThreadHandler, MainThreadHandlerId, MainThreadTask, QueuedMainThreadDispatcher,
};
pub use web_assets::{EmbeddedWebAssets, SharedWebAssets};

pub use rest::shutdown::ShutdownSignal;
pub use rest::supervisor::{
    RestServerApplyReport, RestServerHandle, RestServerSettings, RestSettingsSource,
};
pub use rest::tray::{
    gui_available, spawn_tauri_server_tray, ServerTrayConfiguration, ServerTrayFactory,
    ServerTrayHandle, ServerTrayUpdate,
};

/// Header carrying the client validator (`If-None-Match`) for conditional requests.
pub use crate::HEADER_IF_NONE_MATCH;

/// Header carrying the server validator echoed back to clients.
pub use crate::HEADER_ETAG;
