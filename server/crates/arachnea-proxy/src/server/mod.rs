//! Network server adapters for Arachnea Proxy.
//!
//! This crate owns local listeners, server-side ACLs and inbound protocol
//! parsing. Route selection and outbound transports are delegated to
//! the `core` module.

mod acl;
mod auth;
mod config;
pub mod handlers;
pub mod listeners;
mod loopback;
mod server;

pub use acl::ClientAcl;
pub use auth::{validate_http_basic_header, ProxyAuthConfig, ProxyCredentials};
pub use config::{FileConfig, NetworkConfig, ServerConfig};
pub use loopback::{LoopbackProxyRuntime, LoopbackRoute};
pub use server::ProxyServer;

#[cfg(test)]
#[path = "../../tests/server/config.rs"]
mod config_tests;

#[cfg(test)]
#[path = "../../tests/server/mod.rs"]
mod proxy_integration_tests;
