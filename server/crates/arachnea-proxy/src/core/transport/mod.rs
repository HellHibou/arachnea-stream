//! Outbound proxy transports.

/// Direct TCP transport.
pub mod direct;
/// External outbound transport integration.
pub mod external;
/// HTTP CONNECT / forward proxy transport.
pub mod http;
/// MASQUE (HTTP/3 CONNECT) transport.
pub mod masque;
/// SOCKS4/4a/5 transport.
pub mod socks;
/// TLS-over-TCP transport.
pub mod tls;

use async_trait::async_trait;

use crate::core::{ConnectRequest, ProxyStream, Result};

/// Trait implemented by outbound transports.
#[async_trait]
pub trait OutboundTransport: Send + Sync {
    /// Opens a stream for the supplied connect request.
    ///
    /// # Parameters
    ///
    /// - `request`: Connect request containing destination and context.
    ///
    /// # Returns
    ///
    /// Connected proxy stream.
    ///
    /// # Errors
    ///
    /// Returns an error when the transport cannot satisfy the request.
    async fn connect(&self, request: ConnectRequest) -> Result<ProxyStream>;
}

/// Metadata for a future UDP proxy association.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UdpAssociation {
    /// Relay endpoint returned by an upstream proxy.
    pub relay_endpoint: String,
}
