use crate::core::{ArachneaProxyCore, Destination, ProxyError, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Minimal HTTP response returned by the core convenience client.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimpleHttpResponse {
    /// Numeric HTTP status code.
    pub status: u16,
    /// Raw response bytes including headers and body.
    pub bytes: Vec<u8>,
}

/// Minimal HTTP client that uses `ArachneaProxyCore` for connections.
#[derive(Clone)]
pub struct SimpleHttpClient {
    core: ArachneaProxyCore,
}

impl SimpleHttpClient {
    /// Creates a client backed by the supplied core.
    ///
    /// # Parameters
    ///
    /// - `core`: Proxy core used for outbound HTTP connections.
    ///
    /// # Returns
    ///
    /// Simple HTTP client.
    pub fn new(core: ArachneaProxyCore) -> Self {
        Self { core }
    }

    /// Performs a simple HTTP/1.1 GET request.
    ///
    /// # Parameters
    ///
    /// - `host`: Target hostname.
    /// - `port`: Target port.
    /// - `path`: Request path, normalized to `/` when not absolute.
    ///
    /// # Returns
    ///
    /// Minimal HTTP response containing status and raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when connection, request writing, response reading, or
    /// status parsing fails.
    pub async fn get(&self, host: &str, port: u16, path: &str) -> Result<SimpleHttpResponse> {
        tracing::debug!(host = %host, port = %port, path = %path, "simple http client sending get request");
        let mut stream = self
            .core
            .connect(
                Destination::host_port(host, port)
                    .with_protocol(crate::core::ApplicationProtocol::Http),
            )
            .await?;
        let path = if path.starts_with('/') { path } else { "/" };
        let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await?;
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).await?;
        let status = parse_status(&bytes)?;
        Ok(SimpleHttpResponse { status, bytes })
    }
}

/// Parses the status code from an HTTP response.
///
/// # Parameters
///
/// - `bytes`: Raw HTTP response bytes starting with a status line.
///
/// # Returns
///
/// Numeric HTTP status code.
///
/// # Errors
///
/// Returns an error when the response has no valid status line or status code.
fn parse_status(bytes: &[u8]) -> Result<u16> {
    let line_end = bytes
        .windows(2)
        .position(|window| window == b"\r\n")
        .ok_or_else(|| ProxyError::Protocol("http response has no status line".to_string()))?;
    let line = std::str::from_utf8(&bytes[..line_end])
        .map_err(|_| ProxyError::Protocol("http response status is not utf-8".to_string()))?;
    let mut parts = line.split_whitespace();
    let _version = parts.next();
    let status = parts
        .next()
        .ok_or_else(|| ProxyError::Protocol("http response has no status code".to_string()))?
        .parse::<u16>()
        .map_err(|_| ProxyError::Protocol("http status code is invalid".to_string()))?;
    Ok(status)
}
