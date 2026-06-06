//! Client-side connector adapters for Arachnea Proxy.
//!
//! This crate adapts `ArachneaProxyCore` into client connector shapes without
//! owning route selection or transport policy.

mod error;
pub mod hyper;
pub mod layer;
pub mod rquest;
pub mod stream;
pub mod tower;

pub use error::{ConnectorError, Result};
pub use hyper::ArachneaHyperConnector;
pub use layer::ArachneaProxyLayer;

#[cfg(feature = "rquest")]
pub use rquest::ArachneaRquestLoopback;
pub use stream::ConnectorStream;
pub use tower::ArachneaTowerService;
