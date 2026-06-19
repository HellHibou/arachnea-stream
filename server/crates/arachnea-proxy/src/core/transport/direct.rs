use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time;

use crate::core::{ConnectMetadata, Destination, ProxyError, ProxyStream, Result, TransportKind};

/// Logs a direct TCP connection attempt at DEBUG level.
///
/// # Parameters
///
/// - `destination`: Target being connected to.
fn log_connect(destination: &Destination) {
    tracing::debug!(
        target = %destination.authority(),
        transport = "direct-tcp",
        "connecting to destination"
    );
}

/// Opens a direct TCP stream to a destination.
///
/// # Parameters
///
/// - `destination`: Target destination for the TCP connection.
/// - `timeout`: Maximum duration for the connect operation.
///
/// # Returns
///
/// Proxy stream wrapping the direct TCP connection.
///
/// # Errors
///
/// Returns an error when the TCP connection fails or times out.
pub async fn connect_tcp(destination: &Destination, timeout: Duration) -> Result<ProxyStream> {
    log_connect(destination);
    let authority = destination.authority();
    let stream = time::timeout(timeout, TcpStream::connect(authority))
        .await
        .map_err(|_| ProxyError::Timeout("direct tcp connect"))??;
    Ok(ProxyStream::new(
        stream,
        ConnectMetadata {
            route_name: Some("direct".to_string()),
            transport_kind: Some(TransportKind::Direct),
            hops: vec!["direct".to_string()],
        },
    ))
}
