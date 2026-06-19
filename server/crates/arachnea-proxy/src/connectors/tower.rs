use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::core::{ApplicationProtocol, ArachneaProxyCore, Destination};
use http::Uri;
use tower_service::Service;

use crate::connectors::{ConnectorError, ConnectorStream, Result};

/// Tower `Service<Uri>` that opens streams through `ArachneaProxyCore`.
#[derive(Clone)]
pub struct ArachneaTowerService {
    core: ArachneaProxyCore,
}

impl ArachneaTowerService {
    /// Creates a new tower connector service.
    ///
    /// # Parameters
    ///
    /// - `core`: Proxy core used to open outbound streams.
    ///
    /// # Returns
    ///
    /// Tower service backed by `core`.
    pub fn new(core: ArachneaProxyCore) -> Self {
        Self { core }
    }
}

impl Service<Uri> for ArachneaTowerService {
    /// Stream type returned after a successful connection.
    type Response = ConnectorStream;
    /// Error type returned by connector operations.
    type Error = ConnectorError;
    /// Boxed future used for asynchronous connector calls.
    type Future = Pin<Box<dyn Future<Output = Result<ConnectorStream>> + Send>>;

    /// Reports connector readiness.
    ///
    /// # Parameters
    ///
    /// - `_cx`: Task context supplied by the caller.
    ///
    /// # Returns
    ///
    /// Always returns ready because the connector has no internal backpressure.
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    /// Opens a proxied connection for a URI.
    ///
    /// # Parameters
    ///
    /// - `uri`: HTTP URI whose host, port, and scheme define the destination.
    ///
    /// # Returns
    ///
    /// A future that resolves to a connector stream.
    fn call(&mut self, uri: Uri) -> Self::Future {
        let core = self.core.clone();
        Box::pin(async move {
            let destination = destination_from_uri(&uri)?;
            tracing::debug!(
                uri = %uri,
                destination = %destination.authority(),
                "tower connector opening stream"
            );
            let stream = core.connect(destination).await?;
            Ok(ConnectorStream::new(stream))
        })
    }
}

/// Converts an HTTP URI to a proxy destination.
///
/// # Parameters
///
/// - `uri`: URI supplied by an HTTP client connector.
///
/// # Returns
///
/// Destination containing host, port, and protocol hint.
///
/// # Errors
///
/// Returns an error when the URI has no authority.
pub fn destination_from_uri(uri: &Uri) -> Result<Destination> {
    let scheme = uri.scheme_str().unwrap_or("http");
    let authority = uri
        .authority()
        .ok_or_else(|| ConnectorError::InvalidUri("uri has no authority".to_string()))?;
    let port = authority
        .port_u16()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    let protocol = if scheme == "https" {
        ApplicationProtocol::Https
    } else {
        ApplicationProtocol::Http
    };
    Ok(Destination::host_port(authority.host(), port).with_protocol(protocol))
}
