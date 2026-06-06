//! Unified Arachnea DNS crate.
//!
//! The `core` and `server` features expose the former DNS crates as module
//! groups within one package.

#[cfg(feature = "core")]
pub mod core;
#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "core")]
pub use core::*;
#[cfg(feature = "server")]
pub use server::{
    handle_packet, run_tcp, run_udp, FileBlocklist, FileConfig, FileForwarder, FileLocalRecord,
    FileProfile, NetworkConfig, Server, ServerConfig,
};
