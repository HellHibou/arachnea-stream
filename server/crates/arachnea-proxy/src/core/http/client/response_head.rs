//! HTTP request writes, bounded response-head reads and header/status parsing.

use crate::core::{ProxyError, Result};
use std::collections::HashMap;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Writes the HTTP request and reads only the response headers.
///
/// # Arguments
/// * `stream` - Connected stream from which HTTP or proxy-protocol bytes are read or written.
/// * `request_head` - Serialized request line and headers, including the terminating empty line.
/// * `body` - HTTP body bytes to send or transform.
/// * `send_body` - Whether this request method permits sending the supplied body.
///
pub(super) async fn send_request_and_read_head<S>(
    stream: &mut S,
    request_head: &[u8],
    body: &[u8],
    send_body: bool,
) -> Result<(u16, HashMap<String, String>)>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    stream.write_all(request_head).await?;
    if send_body {
        stream.write_all(body).await?;
    }
    stream.flush().await?;

    let header_bytes = read_headers_only(stream).await?;
    parse_response_head(&header_bytes)
}

/// Parses raw HTTP response header bytes into status code and headers.
///
/// # Arguments
/// * `raw_header_bytes` - Response bytes containing the status line and header block.
///
fn parse_response_head(raw_header_bytes: &[u8]) -> Result<(u16, HashMap<String, String>)> {
    let header_end = raw_header_bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| {
            ProxyError::Protocol("http response has no header terminator".to_string())
        })?;

    let header_section = &raw_header_bytes[..header_end];

    let status_line_end = header_section
        .windows(2)
        .position(|window| window == b"\r\n")
        .ok_or_else(|| ProxyError::Protocol("http response has no status line".to_string()))?;

    let status_line = std::str::from_utf8(&header_section[..status_line_end])
        .map_err(|_| ProxyError::Protocol("http response status line is not utf-8".to_string()))?;

    let mut parts = status_line.splitn(3, ' ');
    let _version = parts.next();
    let status_code = parts
        .next()
        .ok_or_else(|| ProxyError::Protocol("http response has no status code".to_string()))?
        .parse::<u16>()
        .map_err(|_| ProxyError::Protocol("http status code is invalid".to_string()))?;

    let mut headers = HashMap::new();
    let header_text = &header_section[status_line_end + 2..];
    let header_text = std::str::from_utf8(header_text)
        .map_err(|_| ProxyError::Protocol("http response headers are not utf-8".to_string()))?;

    for line in header_text.lines() {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_string();
            let value = value.trim().to_string();
            if !name.is_empty() && !value.is_empty() {
                headers.entry(name).or_insert(value);
            }
        }
    }

    Ok((status_code, headers))
}

/// Returns one matching header value using ASCII-insensitive name comparison.
///
/// # Arguments
/// * `headers` - HTTP headers inspected or updated by this operation.
/// * `wanted` - Header name matched without regard to ASCII case.
pub(super) fn header_value_case_insensitive<'a>(
    headers: &'a HashMap<String, String>,
    wanted: &str,
) -> Option<&'a String> {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
        .map(|(_, value)| value)
}

/// Removes one matching header using ASCII-insensitive name comparison.
///
/// # Arguments
/// * `headers` - HTTP headers inspected or updated by this operation.
/// * `wanted` - Header name matched without regard to ASCII case.
pub(super) fn remove_header_case_insensitive(headers: &mut HashMap<String, String>, wanted: &str) {
    if let Some(name) = headers
        .keys()
        .find(|name| name.eq_ignore_ascii_case(wanted))
        .cloned()
    {
        headers.remove(&name);
    }
}

/// Parses the status code from an HTTP response.
///
/// # Arguments
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
pub(super) fn parse_status(bytes: &[u8]) -> Result<u16> {
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

/// Reads only the response headers from the stream.
///
/// # Arguments
/// * `stream` - Connected stream from which HTTP or proxy-protocol bytes are read or written.
///
pub(super) async fn read_headers_only<S>(stream: &mut S) -> Result<Vec<u8>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut headers = Vec::new();
    let mut byte = [0u8; 1];

    loop {
        let n = stream.read(&mut byte).await?;
        if n == 0 {
            return Err(ProxyError::Protocol(
                "http response ended before the header terminator".to_string(),
            ));
        }

        headers.push(byte[0]);
        if headers.ends_with(b"\r\n\r\n") {
            return Ok(headers);
        }
        if headers.len() > crate::PROXY_MEDIA_MAX_RESPONSE_HEADER_BYTES {
            return Err(ProxyError::Protocol(
                "response headers exceed 64KB limit".to_string(),
            ));
        }
    }
}
