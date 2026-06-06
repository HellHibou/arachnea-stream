use std::time::Duration;

use base64::Engine;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time;

use crate::core::{Destination, ProxyError, ProxyNodeCredentials, Result};

/// Performs an HTTP CONNECT handshake over an existing stream.
///
/// # Parameters
///
/// - `stream`: Stream connected to the HTTP proxy.
/// - `destination`: Target destination requested through the proxy.
/// - `timeout`: Maximum duration for the handshake.
/// - `credentials`: Optional credentials sent as `Proxy-Authorization`.
///
/// # Errors
///
/// Returns an error when request writing, response reading, upstream rejection,
/// or timeout handling fails.
pub async fn connect_tunnel<S>(
    stream: &mut S,
    destination: &Destination,
    timeout: Duration,
    credentials: Option<&ProxyNodeCredentials>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut request = format!(
        "CONNECT {} HTTP/1.1\r\nHost: {}\r\nProxy-Connection: keep-alive\r\n",
        destination.authority(),
        destination.authority()
    );
    if let Some(credentials) = credentials {
        request.push_str("Proxy-Authorization: Basic ");
        request.push_str(&basic_authorization_value(credentials));
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    time::timeout(timeout, async {
        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;
        let response = read_headers(stream, 16 * 1024).await?;
        validate_connect_response(&response)
    })
    .await
    .map_err(|_| ProxyError::Timeout("http connect handshake"))?
}

/// Encodes HTTP Basic credentials for an upstream proxy.
///
/// # Parameters
///
/// - `credentials`: Upstream proxy credentials.
///
/// # Returns
///
/// Base64 encoded `username:password` value.
pub(crate) fn basic_authorization_value(credentials: &ProxyNodeCredentials) -> String {
    base64::engine::general_purpose::STANDARD
        .encode(format!("{}:{}", credentials.username, credentials.password))
}

/// Reads an HTTP header block from an upstream proxy.
///
/// # Parameters
///
/// - `stream`: Stream connected to the HTTP proxy.
/// - `max_bytes`: Maximum accepted header size.
///
/// # Returns
///
/// Raw header bytes including the terminating CRLF sequence.
///
/// # Errors
///
/// Returns an error when the proxy closes early, exceeds `max_bytes`, or the
/// stream fails.
async fn read_headers<S>(stream: &mut S, max_bytes: usize) -> Result<Vec<u8>>
where
    S: AsyncRead + Unpin,
{
    let mut data = Vec::new();
    let mut byte = [0u8; 1];
    while data.len() < max_bytes {
        let read = stream.read(&mut byte).await?;
        if read == 0 {
            return Err(ProxyError::Protocol(
                "upstream closed before http headers completed".to_string(),
            ));
        }
        data.push(byte[0]);
        if data.ends_with(b"\r\n\r\n") {
            return Ok(data);
        }
    }
    Err(ProxyError::Protocol(
        "http proxy response headers exceeded limit".to_string(),
    ))
}

/// Validates an HTTP `CONNECT` response status line.
///
/// # Parameters
///
/// - `response`: Raw response header bytes from the upstream proxy.
///
/// # Errors
///
/// Returns an error when the status line is malformed or the status code is not
/// successful.
fn validate_connect_response(response: &[u8]) -> Result<()> {
    let line_end = response
        .windows(2)
        .position(|window| window == b"\r\n")
        .ok_or_else(|| ProxyError::Protocol("missing http status line".to_string()))?;
    let line = std::str::from_utf8(&response[..line_end])
        .map_err(|_| ProxyError::Protocol("http status line is not utf-8".to_string()))?;
    let mut parts = line.split_whitespace();
    let _version = parts.next();
    let status = parts
        .next()
        .ok_or_else(|| ProxyError::Protocol("missing http status code".to_string()))?
        .parse::<u16>()
        .map_err(|_| ProxyError::Protocol("invalid http status code".to_string()))?;
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(ProxyError::UpstreamRejected(format!(
            "http connect returned status {status}"
        )))
    }
}
