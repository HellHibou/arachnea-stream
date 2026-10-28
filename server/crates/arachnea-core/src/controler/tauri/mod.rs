//! Tauri controller backend and compatibility reexports.

mod assets;
mod configuration;
mod dispatcher;
mod ipc;
mod protocol;
mod runtime;
mod service;
mod stream_response;
mod tabs;
mod windows;

pub use assets::TauriEmbeddedWebAssets;
pub use configuration::{
    TauriBrowsingMode, TauriControlerConfiguration, DEFAULT_TAURI_BROWSING_MODE,
};
pub use service::TauriControlerService;
