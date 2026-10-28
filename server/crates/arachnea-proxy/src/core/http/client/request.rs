//! Serialization of proxied HTTP/1.1 request lines and headers.

use super::ProxiedHttpRequest;
use crate::core::HttpRequestTargetForm;

/// Wire headers and body-transfer decision prepared for a connected route.
pub(super) struct PreparedRequest {
    /// Request line and headers, including their terminating empty line.
    pub(super) bytes: Vec<u8>,
    /// Whether the request method permits the supplied non-empty body.
    pub(super) send_body: bool,
}

/// Serializes caller metadata using the connected route's target form and authorization.
///
/// # Arguments
/// * `request` - Caller request, cookies and non-transferable headers included.
/// * `host` - Destination hostname used by the Host header.
/// * `port` - Destination port, omitted from Host when 80 or 443.
/// * `request_target` - Origin-form path and optional raw query.
/// * `target_form` - Origin or absolute form required by the selected route.
/// * `proxy_authorization` - Optional upstream proxy authorization supplied by the connection.
pub(super) fn prepare(
    request: &ProxiedHttpRequest,
    host: &str,
    port: u16,
    request_target: &str,
    target_form: &HttpRequestTargetForm,
    proxy_authorization: Option<&str>,
) -> PreparedRequest {
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
    if let Some(auth) = proxy_authorization {
        header_lines.push(format!("Proxy-Authorization: {}", auth));
    }

    let mut request_bytes = header_lines.join("\r\n").into_bytes();
    request_bytes.extend_from_slice(b"\r\n\r\n");

    // Determine the target form for the request
    let request_to_send = if *target_form == HttpRequestTargetForm::AbsoluteForm {
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

    PreparedRequest {
        bytes: request_to_send,
        send_body,
    }
}
