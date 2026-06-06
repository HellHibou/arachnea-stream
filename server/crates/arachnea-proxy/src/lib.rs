//! Unified Arachnea proxy crate.
//!
//! The `core`, `server`, and `connectors` features expose the former proxy
//! crates as module groups within one package.

#[cfg(feature = "connectors")]
pub mod connectors;
#[cfg(feature = "core")]
pub mod core;
#[cfg(feature = "server")]
pub mod server;

#[cfg(all(feature = "connectors", feature = "rquest"))]
pub use connectors::ArachneaRquestLoopback;
#[cfg(feature = "connectors")]
pub use connectors::{
    ArachneaHyperConnector, ArachneaProxyLayer, ArachneaTowerService, ConnectorError,
    ConnectorStream,
};
#[cfg(feature = "core")]
pub use core::*;
#[cfg(feature = "server")]
pub use server::{
    validate_http_basic_header, ClientAcl, FileConfig, NetworkConfig, ProxyAuthConfig,
    ProxyCredentials, ProxyServer, ServerConfig,
};
