use std::collections::HashMap;
use std::pin::Pin;
use std::time::Duration;

use crate::core::http::body_readers::{
    decode_chunked_body, ChunkedBodyReader, ContentLengthBodyReader, UntilEofBodyReader,
};
use crate::core::{
    ArachneaProxyCore, ClientContext, ConnectRequest, Destination, HttpRequestTargetForm,
    ProxyError, ProxyStream, Result,
};
use crate::http::actions::{apply_post_actions, PostActionContext, ProxyHttpPostActionConfig};
use bytes::Bytes;
use futures::StreamExt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::io::ReaderStream;

/// Minimal HTTP response returned by the core convenience client.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimpleHttpResponse {
    /// Numeric HTTP status code.
    pub status: u16,
    /// Raw response bytes including headers and body.
    pub bytes: Vec<u8>,
}

/// Response body returned by the proxied HTTP client.
pub enum ProxiedResponseBody {
    /// Entire body loaded in memory, required for body-transforming post-actions.
    Buffered(Vec<u8>),
    /// Body decoded incrementally from the upstream connection.
    Streamed(Pin<Box<dyn futures::Stream<Item = std::io::Result<Bytes>> + Send>>),
}

/// Full proxied HTTP response with parsed headers and body.
pub struct ProxiedHttpResponse {
    /// Numeric HTTP status code.
    pub status: u16,
    /// Response headers.
    pub headers: HashMap<String, String>,
    /// Response body, buffered only when transformations require it.
    pub body: ProxiedResponseBody,
}

/// Input for a proxied HTTP request through the Arachnea proxy core.
#[derive(Clone, Debug)]
pub struct ProxiedHttpRequest {
    /// Absolute target URL (http or https scheme only).
    pub url: String,
    /// HTTP method (GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS).
    pub method: String,
    /// Request headers.
    pub headers: HashMap<String, String>,
    /// Request cookies to send as Cookie header.
    pub cookies: HashMap<String, String>,
    /// Request body bytes.
    pub body: Vec<u8>,
    /// Request-local proxy context used by routing parameter handlers.
    pub client_context: ClientContext,
    /// Post-response actions to apply to the response.
    pub post_actions: Vec<ProxyHttpPostActionConfig>,
    /// Whether the caller requires a fully buffered response body.
    ///
    /// This must be enabled when `post_actions` is non-empty.
    pub buffer_response_body: bool,
    /// Whether only response headers should be read (e.g. HEAD requests).
    pub headers_only: bool,
    /// Request context for action variable substitution.
    pub context: PostActionContext,
}

/// Minimal HTTP client that uses `ArachneaProxyCore` for connections.
#[derive(Clone)]
pub struct SimpleHttpClient {
    core: ArachneaProxyCore,
    tolerate_dechunked_bodies: bool,
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
        Self {
            core,
            tolerate_dechunked_bodies: true,
        }
    }

    /// Configures tolerance for proxies that remove chunk framing while leaving
    /// the `Transfer-Encoding: chunked` response header in place.
    ///
    /// When enabled (the default), buffered responses fall back to their raw
    /// body when chunk decoding fails. Streaming responses remain strict.
    pub fn with_dechunked_body_tolerance(mut self, enabled: bool) -> Self {
        self.tolerate_dechunked_bodies = enabled;
        self
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

    /// Performs a full proxied HTTP request through the Arachnea proxy core.
    ///
    /// This function does NOT follow redirects automatically. The caller receives
    /// the raw response including 3xx status codes with a potentially rewritten
    /// `Location` header (see `rewrite_location`).
    ///
    /// # Parameters
    ///
    /// - `request`: Full proxied HTTP request specification.
    ///
    /// # Returns
    ///
    /// Proxied HTTP response with parsed status, headers, and body.
    ///
    /// # Errors
    ///
    /// Returns an error when URL parsing, connection, or HTTP I/O fails.
    pub async fn request_proxied(
        &self,
        request: ProxiedHttpRequest,
    ) -> Result<ProxiedHttpResponse> {
        if !request.buffer_response_body && !request.post_actions.is_empty() {
            return Err(ProxyError::Protocol(
                "post-response actions require a buffered response body".to_string(),
            ));
        }

        let url = url::Url::parse(&request.url).map_err(|e| {
            ProxyError::Protocol(format!("invalid target URL '{}': {}", request.url, e))
        })?;

        let host = url.host_str().ok_or_else(|| {
            ProxyError::Protocol(format!("target URL '{}' has no host", request.url))
        })?;
        let port = url.port_or_known_default().ok_or_else(|| {
            ProxyError::Protocol(format!("target URL '{}' has no known port", request.url))
        })?;

        let path = url.path();
        let query = url.query();
        let request_target = match query {
            Some(q) => format!("{}?{}", path, q),
            None => path.to_string(),
        };

        // Determine application protocol
        let protocol = match url.scheme() {
            "https" => crate::core::ApplicationProtocol::Https,
            "http" => crate::core::ApplicationProtocol::Http,
            scheme => {
                return Err(ProxyError::Protocol(format!(
                    "unsupported URL scheme '{}'",
                    scheme
                )));
            }
        };
        let is_https = protocol == crate::core::ApplicationProtocol::Https;

        let destination = Destination::host_port(host, port).with_protocol(protocol);
        let stream = self
            .core
            .connect_http_request(
                ConnectRequest::new(destination)
                    .with_client_context(request.client_context.clone()),
            )
            .await?;

        // Build request headers
        let mut header_lines = Vec::new();
        let method = request.method.to_ascii_uppercase();
        let send_body = !request.body.is_empty() && method != "GET" && method != "HEAD";

        // First line
        header_lines.push(format!("{method} {request_target} HTTP/1.1"));

        // Host header
        let host_header = if port == 80 || port == 443 {
            host.to_string()
        } else {
            format!("{}:{}", host, port)
        };
        header_lines.push(format!("Host: {}", host_header));

        // Connection: close to simplify response parsing
        header_lines.push("Connection: close".to_string());

        // Apply caller headers (excluding non-transferable ones)
        let non_transferable = [
            "connection",
            "content-length",
            "transfer-encoding",
            "host",
            "proxy-authorization",
            "proxy-connection",
        ];
        for (name, value) in &request.headers {
            if !non_transferable.contains(&name.to_ascii_lowercase().as_str()) {
                header_lines.push(format!("{}: {}", name, value));
            }
        }

        // Cookies
        if !request.cookies.is_empty() {
            let cookie_str = request
                .cookies
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("; ");
            header_lines.push(format!("Cookie: {}", cookie_str));
        }

        // Content-Length if body is present
        if send_body {
            header_lines.push(format!("Content-Length: {}", request.body.len()));
        }

        // Authorization from proxy if applicable
        if let Some(auth) = &stream.proxy_authorization {
            header_lines.push(format!("Proxy-Authorization: {}", auth));
        }

        let mut request_bytes = header_lines.join("\r\n").into_bytes();
        request_bytes.extend_from_slice(b"\r\n\r\n");

        // Determine the target form for the request
        let request_to_send = if stream.target_form == HttpRequestTargetForm::AbsoluteForm {
            // Replace the request-target with the absolute URL
            let first_line_end = request_bytes
                .windows(2)
                .position(|window| window == b"\r\n")
                .unwrap_or(request_bytes.len());
            let mut absolute_request: Vec<u8> =
                format!("{method} {} HTTP/1.1", request.url).into_bytes();
            if let Some(rest) = request_bytes.get(first_line_end..) {
                absolute_request.extend_from_slice(rest);
            }
            absolute_request
        } else {
            request_bytes
        };

        let metadata = stream.stream.metadata().clone();

        let mut unified_stream =
            if is_https && stream.target_form == HttpRequestTargetForm::OriginForm {
                let tls_stream = crate::core::transport::tls::client_tls(
                    stream.stream,
                    host,
                    Duration::from_secs(10),
                    true,
                )
                .await?;
                ProxyStream::new(tls_stream, metadata)
            } else {
                stream.stream
            };

        let (status_code, mut headers) = send_request_and_read_head(
            &mut unified_stream,
            &request_to_send,
            &request.body,
            send_body,
        )
        .await?;

        let body = if request.headers_only {
            ProxiedResponseBody::Buffered(Vec::new())
        } else {
            let is_chunked = transfer_encoding_is_chunked(&headers);
            if request.buffer_response_body {
                let body = read_buffered_response_body(
                    unified_stream,
                    &headers,
                    is_chunked,
                    self.tolerate_dechunked_bodies,
                )
                .await?;
                if is_chunked {
                    remove_header_case_insensitive(&mut headers, "transfer-encoding");
                    remove_header_case_insensitive(&mut headers, "content-length");
                }
                ProxiedResponseBody::Buffered(process_response_body(
                    body,
                    &mut headers,
                    status_code,
                    &request.post_actions,
                    &request.context,
                )?)
            } else {
                let reader = select_body_reader(unified_stream, &headers);
                if is_chunked {
                    remove_header_case_insensitive(&mut headers, "transfer-encoding");
                    remove_header_case_insensitive(&mut headers, "content-length");
                }
                let reader_stream = ReaderStream::new(reader);
                ProxiedResponseBody::Streamed(Box::pin(reader_stream))
            }
        };

        Ok(ProxiedHttpResponse {
            status: status_code,
            headers,
            body,
        })
    }
}

/// Reads a response body that must be buffered for post-response processing.
async fn read_buffered_response_body<S>(
    mut stream: S,
    headers: &HashMap<String, String>,
    is_chunked: bool,
    tolerate_dechunked_bodies: bool,
) -> Result<Vec<u8>>
where
    S: AsyncRead + Send + Unpin + 'static,
{
    if is_chunked && tolerate_dechunked_bodies {
        let mut raw_body = Vec::new();
        stream.read_to_end(&mut raw_body).await?;
        return match decode_chunked_body(&raw_body) {
            Ok(body) => Ok(body),
            Err(error) => {
                tracing::warn!(
                    %error,
                    body_len = raw_body.len(),
                    "upstream retained Transfer-Encoding: chunked after dechunking the response; using raw body"
                );
                Ok(raw_body)
            }
        };
    }

    let reader = select_body_reader(stream, headers);
    let reader_stream = ReaderStream::new(reader);
    let mut body = Vec::new();
    futures::pin_mut!(reader_stream);
    while let Some(chunk) = reader_stream.next().await {
        body.extend_from_slice(&chunk?);
    }
    Ok(body)
}

/// Writes the HTTP request and reads only the response headers.
async fn send_request_and_read_head<S>(
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

/// Applies post-response actions to a buffered body and updates invalidated headers.
fn process_response_body(
    body: Vec<u8>,
    headers: &mut HashMap<String, String>,
    status_code: u16,
    post_actions: &[ProxyHttpPostActionConfig],
    context: &PostActionContext,
) -> Result<Vec<u8>> {
    let original_len = body.len();
    let new_body = apply_post_actions(status_code, headers, body, post_actions, context)?;

    if new_body.len() != original_len {
        remove_header_case_insensitive(headers, "content-length");
        remove_header_case_insensitive(headers, "etag");
        remove_header_case_insensitive(headers, "content-md5");
        remove_header_case_insensitive(headers, "digest");
        headers.insert("Content-Length".to_string(), new_body.len().to_string());
    }

    Ok(new_body)
}

fn header_value_case_insensitive<'a>(
    headers: &'a HashMap<String, String>,
    wanted: &str,
) -> Option<&'a String> {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
        .map(|(_, value)| value)
}

fn remove_header_case_insensitive(headers: &mut HashMap<String, String>, wanted: &str) {
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

/// Selects an appropriate body reader based on response headers.
fn select_body_reader<S: AsyncRead + Send + Unpin + 'static>(
    stream: S,
    headers: &HashMap<String, String>,
) -> Box<dyn AsyncRead + Send + Unpin> {
    // Check for chunked transfer encoding
    if transfer_encoding_is_chunked(headers) {
        return Box::new(ChunkedBodyReader::new(stream));
    }

    // Check for content-length
    if let Some(cl) = header_value_case_insensitive(headers, "content-length") {
        if let Ok(length) = cl.parse::<usize>() {
            return Box::new(ContentLengthBodyReader::new(stream, length));
        }
    }

    // Default to reading until EOF
    Box::new(UntilEofBodyReader::new(stream))
}

/// Returns whether transfer encoding includes chunked coding.
fn transfer_encoding_is_chunked(headers: &HashMap<String, String>) -> bool {
    header_value_case_insensitive(headers, "transfer-encoding").is_some_and(|value| {
        value
            .split(',')
            .any(|part| part.trim().eq_ignore_ascii_case("chunked"))
    })
}

/// Reads only the response headers from the stream.
async fn read_headers_only<S>(stream: &mut S) -> Result<Vec<u8>>
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
        if headers.len() > 65_536 {
            return Err(ProxyError::Protocol(
                "response headers exceed 64KB limit".to_string(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, DuplexStream};

    async fn response_stream(response: &[u8]) -> DuplexStream {
        let (mut writer, reader) = tokio::io::duplex(response.len().max(1));
        let response = response.to_vec();
        tokio::spawn(async move {
            writer.write_all(&response).await.unwrap();
            writer.shutdown().await.unwrap();
        });
        reader
    }

    #[tokio::test]
    async fn reads_complete_headers_without_consuming_body() {
        let mut stream = response_stream(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\ntest").await;
        let headers = read_headers_only(&mut stream).await.unwrap();
        assert_eq!(headers, b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n");

        let mut body = Vec::new();
        stream.read_to_end(&mut body).await.unwrap();
        assert_eq!(body, b"test");
    }

    #[tokio::test]
    async fn rejects_truncated_headers() {
        let mut stream = response_stream(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n").await;
        let error = read_headers_only(&mut stream).await.unwrap_err();
        assert!(matches!(error, ProxyError::Protocol(_)));
    }
}
