//! DNS server adapter for Arachnea DNS.
//!
//! This crate owns TOML configuration loading, UDP/TCP listeners and DNS
//! packet adaptation. All resolution decisions are delegated to
//! the `core` module.

mod config;
mod dns;
mod server;

pub use config::{
    FileBlocklist, FileConfig, FileForwarder, FileLocalRecord, FileProfile, NetworkConfig,
    ServerConfig,
};
pub use dns::handle_packet;
pub use server::{run_tcp, run_udp, Server};

#[cfg(test)]
#[path = "tests/config_tests.rs"]
mod config_tests;
