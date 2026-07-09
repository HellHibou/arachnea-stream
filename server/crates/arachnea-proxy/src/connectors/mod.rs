//! Client-side connector adapters for Arachnea Proxy.
//!
//! This crate adapts `ArachneaProxyCore` into client connector shapes without
//! owning route selection or transport policy.

mod error;
/// Hyper connector for use as a `hyper` client connector.
pub mod hyper;
/// Tower layer for proxy-aware service middleware.
pub mod layer;
/// rquest loopback connector for proxy-transparent HTTP clients.
pub mod rquest;
/// Raw TCP/TLS connector stream types.
pub mod stream;
/// Tower service adapter wrapping `ArachneaProxyCore`.
pub mod tower;

pub use error::{ConnectorError, Result};
pub use hyper::ArachneaHyperConnector;
pub use layer::ArachneaProxyLayer;

#[cfg(feature = "rquest")]
pub use rquest::ArachneaRquestLoopback;
pub use stream::ConnectorStream;
pub use tower::ArachneaTowerService;
