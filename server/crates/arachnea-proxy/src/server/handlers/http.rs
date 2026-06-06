use std::net::SocketAddr;
use std::sync::Arc;

use crate::core::{
    normalize_parameter_value, ApplicationProtocol, ArachneaProxyCore, ClientContext,
    ConnectRequest, Destination, HttpRequestTargetForm, ParameterDefinition, ProxyError, Result,
};
use http::Uri;
use tokio::io::{self, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::server::handlers::connect::destination_from_connect_authority;
use crate::server::{validate_http_basic_header, ProxyCredentials, ServerConfig};

/// Handles one HTTP proxy connection.
pub async fn handle<S>(
    mut client: S,
    _peer: SocketAddr,
    config: Arc<ServerConfig>,
    core: ArachneaProxyCore,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let request = read_http_request(&mut client, config.max_http_header_bytes).await?;
    if let Some(credentials) = config
        .authentication
        .credentials()
        .map_err(ProxyError::Config)?
    {
        if !request_has_valid_proxy_auth(&request, &credentials) {
            write_proxy_auth_required(&mut client, &config.authentication.realm).await?;
            return Err(ProxyError::AccessDenied(
                "http proxy authentication failed".to_string(),
            ));
        }
    }
    if request.method.eq_ignore_ascii_case("CONNECT") {
        handle_connect(client, request, core).await
    } else {
        handle_absolute_request(client, request, core).await
    }
}

/// Returns whether a request contains valid proxy credentials.
///
/// # Parameters
///
/// - `request`: Parsed HTTP request head.
/// - `credentials`: Expected proxy credentials.
///
/// # Returns
///
/// `true` when the request includes a matching `Proxy-Authorization` header.
fn request_has_valid_proxy_auth(request: &HttpRequestHead, credentials: &ProxyCredentials) -> bool {
    request.headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case("Proxy-Authorization")
            && validate_http_basic_header(&header.value, credentials)
    })
}

/// Handles an HTTP `CONNECT` request by establishing a TCP tunnel.
///
/// # Parameters
///
/// - `client`: Client stream that issued the `CONNECT` request.
/// - `request`: Parsed HTTP request head.
/// - `core`: Proxy core used to open the outbound tunnel.
///
/// # Errors
///
/// Returns an error when destination parsing, outbound connection, or tunnel
/// relay fails.
async fn handle_connect<S>(
    mut client: S,
    request: HttpRequestHead,
    core: ArachneaProxyCore,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let destination = destination_from_connect_authority(&request.target)?;
    let parameter_definitions = core.parameter_definitions();
    let context = context_from_headers(&request.headers, &parameter_definitions);
    let mut upstream = match core
        .connect_request(ConnectRequest::new(destination).with_client_context(context))
        .await
    {
        Ok(stream) => stream,
        Err(error) => {
            let _ = write_error(&mut client, 502, "Bad Gateway").await;
            return Err(error);
        }
    };
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    io::copy_bidirectional(&mut client, &mut upstream).await?;
    Ok(())
}

/// Handles an absolute-form HTTP proxy request.
///
/// # Parameters
///
/// - `client`: Client stream that issued the HTTP proxy request.
/// - `request`: Parsed HTTP request head.
/// - `core`: Proxy core used to select the outbound route.
///
/// # Errors
///
/// Returns an error when URI parsing, route selection, outbound connection, or
/// stream relay fails.
async fn handle_absolute_request<S>(
    mut client: S,
    request: HttpRequestHead,
    core: ArachneaProxyCore,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let uri = request.target.parse::<Uri>().map_err(|_| {
        ProxyError::Protocol("http proxy request target is not a valid uri".to_string())
    })?;
    let scheme = uri.scheme_str().unwrap_or("http");
    let authority = uri
        .authority()
        .ok_or_else(|| ProxyError::Protocol("http proxy request has no authority".to_string()))?;
    let port = authority
        .port_u16()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    let host = authority.host();
    let protocol = if scheme == "https" {
        ApplicationProtocol::Https
    } else {
        ApplicationProtocol::Http
    };
    let destination = Destination::host_port(host, port).with_protocol(protocol);
    let parameter_definitions = core.parameter_definitions();
    let context = context_from_headers(&request.headers, &parameter_definitions);
    let mut outbound = match core
        .connect_http_request(ConnectRequest::new(destination).with_client_context(context))
        .await
    {
        Ok(outbound) => outbound,
        Err(error) => {
            let _ = write_error(&mut client, 502, "Bad Gateway").await;
            return Err(error);
        }
    };

    let request_target = match outbound.target_form {
        HttpRequestTargetForm::AbsoluteForm => request.target.as_str(),
        HttpRequestTargetForm::OriginForm => uri
            .path_and_query()
            .map(|value| value.as_str())
            .unwrap_or("/"),
    };
    let mut rewritten = format!(
        "{} {} {}\r\n",
        request.method, request_target, request.version
    );
    for header in request.headers {
        let name = header.name.to_ascii_lowercase();
        if should_strip_header(&name, &parameter_definitions) {
            continue;
        }
        rewritten.push_str(&header.name);
        rewritten.push_str(": ");
        rewritten.push_str(&header.value);
        rewritten.push_str("\r\n");
    }
    if let Some(proxy_authorization) = outbound.proxy_authorization {
        rewritten.push_str("Proxy-Authorization: ");
        rewritten.push_str(&proxy_authorization);
        rewritten.push_str("\r\n");
    }
    rewritten.push_str("Connection: close\r\n\r\n");
    outbound.stream.write_all(rewritten.as_bytes()).await?;
    io::copy_bidirectional(&mut client, &mut outbound.stream).await?;
    Ok(())
}

/// Writes a small HTTP error response to a client stream.
///
/// # Parameters
///
/// - `client`: Client stream that receives the response.
/// - `status`: Numeric HTTP status code.
/// - `reason`: HTTP reason phrase.
///
/// # Errors
///
/// Returns an I/O error when the response cannot be written.
async fn write_error<S>(client: &mut S, status: u16, reason: &str) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let body = format!("{status} {reason}\n");
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    client.write_all(response.as_bytes()).await
}

/// Writes an HTTP proxy authentication challenge.
///
/// # Parameters
///
/// - `client`: Client stream that receives the response.
/// - `realm`: HTTP Basic realm to advertise.
///
/// # Errors
///
/// Returns an I/O error when the response cannot be written.
async fn write_proxy_auth_required<S>(client: &mut S, realm: &str) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let body = "407 Proxy Authentication Required\n";
    let response = format!(
        "HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"{}\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        realm.replace('"', ""),
        body.len()
    );
    client.write_all(response.as_bytes()).await
}

/// Reads one HTTP request head from a stream.
///
/// # Parameters
///
/// - `client`: Client stream to read from.
/// - `max_bytes`: Maximum number of header bytes accepted before failing.
///
/// # Returns
///
/// A parsed HTTP request head.
///
/// # Errors
///
/// Returns an error when the client closes early, exceeds `max_bytes`, or sends
/// invalid headers.
async fn read_http_request<S>(client: &mut S, max_bytes: usize) -> Result<HttpRequestHead>
where
    S: AsyncRead + Unpin,
{
    let mut data = Vec::new();
    let mut byte = [0u8; 1];
    while data.len() < max_bytes {
        let read = client.read(&mut byte).await?;
        if read == 0 {
            return Err(ProxyError::Protocol(
                "client closed before http headers completed".to_string(),
            ));
        }
        data.push(byte[0]);
        if data.ends_with(b"\r\n\r\n") {
            return parse_http_request(&data);
        }
    }
    Err(ProxyError::Protocol(
        "http request headers exceeded configured limit".to_string(),
    ))
}

/// Parses raw HTTP/1.x request headers.
///
/// # Parameters
///
/// - `data`: Header bytes ending with the HTTP header terminator.
///
/// # Returns
///
/// Parsed request-line and header fields.
///
/// # Errors
///
/// Returns an error when headers are not UTF-8 or the request line is invalid.
fn parse_http_request(data: &[u8]) -> Result<HttpRequestHead> {
    let text = std::str::from_utf8(data)
        .map_err(|_| ProxyError::Protocol("http request headers are not utf-8".to_string()))?;
    let mut lines = text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| ProxyError::Protocol("missing http request line".to_string()))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| ProxyError::Protocol("missing http method".to_string()))?
        .to_string();
    let target = parts
        .next()
        .ok_or_else(|| ProxyError::Protocol("missing http request target".to_string()))?
        .to_string();
    let version = parts
        .next()
        .ok_or_else(|| ProxyError::Protocol("missing http version".to_string()))?
        .to_string();
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push(Header {
                name: name.trim().to_string(),
                value: value.trim().to_string(),
            });
        }
    }
    Ok(HttpRequestHead {
        method,
        target,
        version,
        headers,
    })
}

/// Builds a client context from registered proxy parameter headers.
///
/// # Parameters
///
/// - `headers`: Parsed HTTP request headers.
/// - `definitions`: Registered parameter definitions.
///
/// # Returns
///
/// Client context populated with recognized routing parameters.
fn context_from_headers(headers: &[Header], definitions: &[ParameterDefinition]) -> ClientContext {
    let mut context = ClientContext::new();
    for header in headers {
        if let Some(definition) = definitions
            .iter()
            .find(|definition| header.name.eq_ignore_ascii_case(&definition.http_header))
        {
            context.insert(
                definition.name.clone(),
                crate::core::ClientParameter::String(normalize_parameter_value(
                    &definition.name,
                    &header.value,
                )),
            );
        }
    }
    context
}

/// Returns whether an HTTP header must be removed before forwarding.
///
/// # Parameters
///
/// - `lowercase_name`: Lower-case HTTP header name.
/// - `definitions`: Registered parameter definitions.
///
/// # Returns
///
/// `true` when the header is hop-by-hop, privacy-sensitive, or a non-forwarded
/// parameter header.
fn should_strip_header(lowercase_name: &str, definitions: &[ParameterDefinition]) -> bool {
    if matches!(
        lowercase_name,
        "connection"
            | "proxy-connection"
            | "proxy-authorization"
            | "forwarded"
            | "x-forwarded-for"
            | "via"
    ) {
        return true;
    }
    definitions.iter().any(|definition| {
        !definition.forward_header && lowercase_name.eq_ignore_ascii_case(&definition.http_header)
    })
}

#[derive(Debug)]
/// Parsed HTTP request head without a body.
struct HttpRequestHead {
    method: String,
    target: String,
    version: String,
    headers: Vec<Header>,
}

#[derive(Debug)]
/// Parsed HTTP header name and value.
struct Header {
    name: String,
    value: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::PROXY_HEADER_PARAMETER_COUNTRY;
    use crate::core::PROXY_PARAMETER_COUNTRY;

    /// Verifies that registered headers are extracted into generic parameters.
    #[test]
    fn extracts_registered_parameter_headers() {
        let headers = vec![Header {
            name: PROXY_HEADER_PARAMETER_COUNTRY.to_string(),
            value: "be".to_string(),
        }];
        let definitions = vec![ParameterDefinition::new(
            PROXY_HEADER_PARAMETER_COUNTRY,
            PROXY_PARAMETER_COUNTRY,
            false,
        )];

        let context = context_from_headers(&headers, &definitions);

        assert_eq!(context.get_string(PROXY_PARAMETER_COUNTRY), Some("BE"));
    }

    /// Verifies that non-forwarded parameter headers are stripped from HTTP relay requests.
    #[test]
    fn strips_non_forwarded_parameter_headers() {
        let definitions = vec![ParameterDefinition::new(
            PROXY_HEADER_PARAMETER_COUNTRY,
            PROXY_PARAMETER_COUNTRY,
            false,
        )];

        assert!(should_strip_header("arachnea-proxy-country", &definitions));
    }

    /// Verifies that forwarded parameter headers can be preserved.
    #[test]
    fn preserves_forwarded_parameter_headers() {
        let definitions = vec![ParameterDefinition::new(
            "Arachnea-Proxy-Trace",
            "trace",
            true,
        )];

        assert!(!should_strip_header("arachnea-proxy-trace", &definitions));
    }
}
