#![warn(missing_docs)]
//! Unified Arachnea proxy crate.
//!
//! The `core`, `server`, and `connectors` features expose the former proxy
//! crates as module groups within one package.
//!
//! ## Features
//!
//! - **core**: In-process proxy routing core with multi-hop chains, proxy pools,
//!   route selection, and outbound transports (HTTP, HTTPS, SOCKS4/4a, SOCKS5, Tor).
//! - **server**: Proxy server listeners (HTTP, HTTPS, SOCKS4/4a, SOCKS5) with
//!   configuration, ACLs, authentication, and safety checks.
//! - **connectors**: Client-side adapters for `tower`, `hyper`, and `rquest` ecosystems.
//!
//! ## Usage
//!
//! Add to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! arachnea-proxy = { version = "0.1", features = ["core", "server"] }
//! ```
//!
//! Then use the core for in-process routing:
//!
//! ```no_run
//! use arachnea_proxy::core::{ArachneaProxyCore, Destination, ApplicationProtocol};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), arachnea_proxy::ProxyError> {
//!     let core = ArachneaProxyCore::new(Default::default())?;
//!     let _stream = core.connect(Destination::from_authority(
//!         "example.com:443",
//!         ApplicationProtocol::Https,
//!     )?).await?;
//!     Ok(())
//! }
//! ```

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
