//! Proxy HTTP handler for the controller stream route.
//!
//! This module implements the endpoint that forwards HTTP requests through
//! the Arachnea proxy core. The path format is:
//!
//! `/<API_PREFIX>/[opts_<BASE64>]/<PROTOCOL>[_<PORT>]/<HOST>/<PATH>[?<QUERY>]`
//!
//! Examples:
//! - `/api/https_8080/www.google.lu/index.html`
//! - `/api/opts_ABCD/https_8080/www.google.lu/search?q=toto`
//! - `/api/https/www.google.lu/images/img.png`

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use serde::Deserialize;
use url::Url;

use crate::core::http::{ProxiedHttpRequest, SimpleHttpClient};
use crate::core::ArachneaProxyCore;
use arachnea_core::controler::{
    ControlerService, ControlerServiceExt, ControlerStreamInput, ControlerStreamOutput,
};

const MAX_LOCAL_URL_BYTES: usize = 8192;

/// Known option fields allowed in the `opts` JSON.
const ALLOWED_OPTS_FIELDS: &[&str] = &["headers", "cookies", "proxy"];

#[derive(Deserialize)]
struct ProxyHttpOpts {
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    cookies: HashMap<String, String>,
    #[allow(dead_code)]
    #[serde(default)]
    proxy: serde_json::Value,
}

/// Decodes a base64url (no-padding) value from a path segment.
fn decode_base64url_segment(encoded: &str) -> Result<Vec<u8>, String> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|e| format!("Invalid base64url: {}", e))
}

/// Validates the content of `opts` JSON against known allowed fields.
fn validate_opts_fields(value: &serde_json::Value) -> Result<(), (u16, String)> {
    match value {
        serde_json::Value::Object(map) => {
            for key in map.keys() {
                if !ALLOWED_OPTS_FIELDS.contains(&key.as_str()) {
                    return Err((404, format!("Unknown option field '{}'", key)));
                }
            }
            if let Some(headers) = map.get("headers") {
                if !headers.is_object() {
                    return Err((400, "'headers' must be a JSON object".to_string()));
                }
            }
            if let Some(cookies) = map.get("cookies") {
                if !cookies.is_object() {
                    return Err((400, "'cookies' must be a JSON object".to_string()));
                }
            }
            Ok(())
        }
        serde_json::Value::Null => Ok(()),
        _ => Err((400, "'opts' must be a JSON object".to_string())),
    }
}

/// Filters out non-transferable headers.
fn filter_non_transferable(headers: &mut HashMap<String, String>) {
    const NON_TRANSFERABLE: &[&str] = &[
        "connection",
        "content-length",
        "transfer-encoding",
        "host",
        "proxy-authorization",
        "proxy-connection",
        "referer",
    ];
    headers.retain(|key, _| {
        let lower = key.to_ascii_lowercase();
        !NON_TRANSFERABLE.contains(&lower.as_str())
    });
}

fn stream_error(status: u16, message: impl Into<String>) -> ControlerStreamOutput {
    ControlerStreamOutput {
        status,
        body: message.into().into_bytes(),
        content_type: "text/plain; charset=utf-8".to_string(),
        headers: HashMap::new(),
    }
}

fn remove_header_case_insensitive(
    headers: &mut HashMap<String, String>,
    wanted: &str,
) -> Option<String> {
    let key = headers
        .keys()
        .find(|name| name.eq_ignore_ascii_case(wanted))
        .cloned()?;
    headers.remove(&key)
}

/// Validates that header names and values are syntactically acceptable.
fn validate_headers(headers: &HashMap<String, String>) -> Result<(), String> {
    for (name, value) in headers {
        if name.is_empty()
            || name.len() > 256
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(format!("Invalid header name: '{}'", name));
        }
        if value.len() > 65536 {
            return Err(format!("Header value too long for '{}'", name));
        }
        if value.contains(&['\r', '\n'][..]) {
            return Err(format!("Header value contains CR/LF for '{}'", name));
        }
    }
    Ok(())
}

/// Validates that cookie names and values are acceptable.
fn validate_cookies(cookies: &HashMap<String, String>) -> Result<(), String> {
    for (name, value) in cookies {
        if name.is_empty()
            || name.len() > 256
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        {
            return Err(format!("Invalid cookie name: '{}'", name));
        }
        if value.contains(&[';', ',', ' ', '\r', '\n'][..]) {
            return Err(format!(
                "Cookie value contains prohibited chars for '{}'",
                name
            ));
        }
        if value.len() > 4096 {
            return Err(format!("Cookie value too long for '{}'", name));
        }
    }
    Ok(())
}

/// Result of parsing a proxy path segment into protocol and optional port.
struct ParsedProtocol {
    protocol: String,
    port: Option<u16>,
}

/// Parses a protocol segment like `https:8080`, `http:3000`, `https:`, `http:`.
fn parse_protocol_segment(segment: &str) -> Result<ParsedProtocol, String> {
    if segment == "https:" || segment == "https" {
        Ok(ParsedProtocol {
            protocol: "https".to_string(),
            port: None,
        })
    } else if segment == "http:" || segment == "http" {
        Ok(ParsedProtocol {
            protocol: "http".to_string(),
            port: None,
        })
    } else if let Some(rest) = segment.strip_prefix("https:") {
        let port: u16 = rest
            .parse()
            .map_err(|_| format!("invalid port in '{}'", segment))?;
        Ok(ParsedProtocol {
            protocol: "https".to_string(),
            port: Some(port),
        })
    } else if let Some(rest) = segment.strip_prefix("http:") {
        let port: u16 = rest
            .parse()
            .map_err(|_| format!("invalid port in '{}'", segment))?;
        Ok(ParsedProtocol {
            protocol: "http".to_string(),
            port: Some(port),
        })
    } else {
        Err(format!(
            "invalid protocol segment '{}': expected http, https, http_<PORT>, or https_<PORT>",
            segment
        ))
    }
}

/// Builds the target URL from the parsed components and local query string.
fn build_target_url(
    protocol: &str,
    port: Option<u16>,
    host: &str,
    path_and_query: &str,
    local_query: &str,
) -> String {
    let host_part = if host.contains(':') {
        // IPv6
        format!("[{}]", host)
    } else {
        host.to_string()
    };

    let authority = match port {
        Some(p) => format!("{}:{}", host_part, p),
        None => host_part,
    };

    let combined_path_query = if local_query.is_empty() {
        path_and_query.to_string()
    } else if path_and_query.contains('?') {
        format!("{}&{}", path_and_query, local_query)
    } else {
        format!("{}?{}", path_and_query, local_query)
    };

    format!("{}://{}{}", protocol, authority, combined_path_query)
}

/// Encodes a proxied URL path back into the proxy path format.
fn encode_proxy_path(api_prefix: &str, resolved_url: &Url, opts_encoded: &str) -> String {
    let api = api_prefix.trim_matches('/');
    let protocol = resolved_url.scheme();
    let host = resolved_url.host_str().unwrap_or("");
    let port = resolved_url.port();
    let path = resolved_url.path();
    let query = resolved_url
        .query()
        .map(|q| format!("?{}", q))
        .unwrap_or_default();

    let protocol_segment = match port {
        Some(p) => format!("{}_{}", protocol, p),
        None => protocol.to_string(),
    };

    let opts_segment = if opts_encoded.is_empty() {
        String::new()
    } else {
        format!("opts_{}/", opts_encoded)
    };

    format!("/{api}/{opts_segment}{protocol_segment}/{host}{path}{query}")
}

/// Recovers the absolute target URL from a potentially relative Location
/// header and the current target URL.
fn resolve_location(location: &str, current_target_url: &str) -> Result<String, String> {
    let base = Url::parse(current_target_url)
        .map_err(|e| format!("Failed to parse current target URL: {}", e))?;
    let resolved = base
        .join(location)
        .map_err(|e| format!("Failed to resolve Location '{}': {}", location, e))?;
    Ok(resolved.to_string())
}

/// Rewrites a `Location` header value to a proxied URL.
fn rewrite_location_header(
    headers: &mut HashMap<String, String>,
    current_target_url: &str,
    current_opts_encoded: &str,
    api_prefix: &str,
) -> Result<(), String> {
    let location = match headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("location"))
        .map(|(_, value)| value.clone())
    {
        Some(location) => location,
        None => return Ok(()),
    };

    let resolved_url_str = resolve_location(&location, current_target_url)?;
    let resolved_url = Url::parse(&resolved_url_str)
        .map_err(|e| format!("Failed to parse resolved URL: {}", e))?;

    let new_location = encode_proxy_path(api_prefix, &resolved_url, current_opts_encoded);

    if let Some(key) = headers
        .keys()
        .find(|name| name.eq_ignore_ascii_case("location"))
        .cloned()
    {
        headers.insert(key, new_location);
    }
    Ok(())
}

/// Merges frontend opts headers/cookies over the incoming headers/cookies.
///
/// Priority: incoming < opts (opts wins on conflict).
fn merge_opts(
    incoming_headers: &HashMap<String, String>,
    opts: Option<ProxyHttpOpts>,
) -> Result<(HashMap<String, String>, HashMap<String, String>), String> {
    let mut headers = incoming_headers.clone();
    let mut cookies = HashMap::new();

    // Extract cookies from incoming Cookie header
    if let Some(cookie_header) = headers.get("cookie").or_else(|| headers.get("Cookie")) {
        for pair in cookie_header.split(';') {
            let pair = pair.trim();
            if let Some((name, value)) = pair.split_once('=') {
                let name = name.trim().to_string();
                let value = value.trim().to_string();
                if !name.is_empty() {
                    cookies.insert(name, value);
                }
            }
        }
    }

    // Remove the raw Cookie header from headers (we manage it ourselves)
    headers.remove("cookie");
    headers.remove("Cookie");

    // Filter non-transferable
    filter_non_transferable(&mut headers);

    // Apply opts overrides
    if let Some(opts) = opts {
        // Validate all header/cookie data in opts first
        validate_headers(&opts.headers)?;
        validate_cookies(&opts.cookies)?;

        // Merge headers: opts override incoming
        for (key, value) in &opts.headers {
            headers.insert(key.clone(), value.clone());
        }
        filter_non_transferable(&mut headers);

        // Merge cookies: opts override incoming
        for (key, value) in &opts.cookies {
            cookies.insert(key.clone(), value.clone());
        }
    }

    Ok((headers, cookies))
}

/// Parses the proxy path segments into the target URL components.
///
/// The path format is: `[opts_<BASE64>/]<PROTOCOL>[_<PORT>]/<HOST>/<PATH>`
/// where `<PATH>` may include further `/` segments.
struct ParsedProxyPath {
    opts_encoded: String,
    protocol: String,
    port: Option<u16>,
    host: String,
    path_and_query: String,
}

fn parse_proxy_path(path: &str) -> Result<ParsedProxyPath, (u16, String)> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Err((400, "missing target in proxy path".to_string()));
    }

    // Split and filter out empty segments that come from double slashes
    // (e.g. /api/https://host becomes ["https:", "", "host"])
    let raw_segments: Vec<&str> = trimmed.split('/').collect();
    let segments: Vec<&str> = raw_segments.into_iter().filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return Err((400, "missing target in proxy path".to_string()));
    }

    let mut idx = 0;
    let opts_encoded;

    // Check if first segment is opts_<base64>
    if let Some(first) = segments.first() {
        if let Some(rest) = first.strip_prefix("opts_") {
            opts_encoded = rest.to_string();
            idx = 1;
        } else {
            opts_encoded = String::new();
        }
    } else {
        opts_encoded = String::new();
    }

    // Next segment must be protocol[_port]
    if idx >= segments.len() {
        return Err((400, "missing protocol segment".to_string()));
    }
    let parsed_protocol = parse_protocol_segment(segments[idx]).map_err(|e| (400, e))?;
    idx += 1;

    // Next segment must be host
    if idx >= segments.len() {
        return Err((400, "missing host segment".to_string()));
    }
    let host = segments[idx].to_string();
    idx += 1;

    // Everything after host is the path (join remaining segments)
    let path_and_query = if idx < segments.len() {
        let remaining = &segments[idx..];
        // Check if the last segment contains a query string
        let mut path = remaining.join("/");
        // Ensure path starts with /
        if !path.starts_with('/') {
            path.insert(0, '/');
        }
        path
    } else {
        String::from("/")
    };

    Ok(ParsedProxyPath {
        opts_encoded,
        protocol: parsed_protocol.protocol,
        port: parsed_protocol.port,
        host,
        path_and_query,
    })
}

/// Handles a proxied HTTP request through the controller stream interface.
///
/// This is the main entry point registered as a stream function. The
/// entry-point URL used for rewriting `Location` headers in redirect responses
/// is read from `input.entry_point`.
pub async fn handle_proxy_http(
    proxy_core: Arc<ArachneaProxyCore>,
    input: ControlerStreamInput,
) -> Result<ControlerStreamOutput, String> {
    // Validate URL length
    let full_url = format!(
        "{}/{}{}{}",
        input.entry_point,
        &input.path,
        if input.query.is_empty() { "" } else { "?" },
        &input.query
    );
    if full_url.len() > MAX_LOCAL_URL_BYTES {
        return Ok(stream_error(414, "URI Too Long"));
    }

    // Parse the new path format
    let parsed = match parse_proxy_path(&input.path) {
        Ok(p) => p,
        Err((status, msg)) => return Ok(stream_error(status, msg)),
    };

    // Build the target URL
    let target_url_str = build_target_url(
        &parsed.protocol,
        parsed.port,
        &parsed.host,
        &parsed.path_and_query,
        &input.query,
    );

    // Validate target URL
    let parsed_target = match Url::parse(&target_url_str) {
        Ok(url) => url,
        Err(_) => {
            return Ok(stream_error(
                400,
                format!("invalid target URL '{}'", target_url_str),
            ));
        }
    };

    let scheme = parsed_target.scheme();
    if scheme != "http" && scheme != "https" {
        return Ok(stream_error(
            400,
            format!(
                "unsupported URL scheme '{}' (only http/https allowed)",
                scheme
            ),
        ));
    }
    let method = input.method.to_ascii_uppercase();
    const SUPPORTED_METHODS: &[&str] =
        &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
    if !SUPPORTED_METHODS.contains(&method.as_str()) {
        return Ok(stream_error(
            400,
            format!("unsupported HTTP method '{}'", input.method),
        ));
    }

    // Parse opts from the path segment
    let opts = if parsed.opts_encoded.is_empty() {
        None
    } else {
        let opts_json_bytes = match decode_base64url_segment(&parsed.opts_encoded) {
            Ok(bytes) => bytes,
            Err(error) => return Ok(stream_error(400, error)),
        };
        let opts_json_str = match String::from_utf8(opts_json_bytes) {
            Ok(value) => value,
            Err(_) => return Ok(stream_error(400, "opts is not valid UTF-8")),
        };

        let opts_value: serde_json::Value = match serde_json::from_str(&opts_json_str) {
            Ok(value) => value,
            Err(error) => return Ok(stream_error(400, format!("invalid opts JSON: {error}"))),
        };

        if let Err((status, message)) = validate_opts_fields(&opts_value) {
            return Ok(stream_error(status, message));
        }

        let opts_parsed: ProxyHttpOpts = match serde_json::from_value(opts_value) {
            Ok(opts) => opts,
            Err(error) => return Ok(stream_error(400, format!("opts parsing error: {error}"))),
        };

        Some(opts_parsed)
    };

    // Merge headers and cookies
    let (mut headers, cookies) = match merge_opts(&input.headers, opts) {
        Ok(value) => value,
        Err(error) => return Ok(stream_error(400, error)),
    };

    // Set the Host header from the target URL (host is removed as non-transferable
    //    from the incoming request since it refers to the proxy, not the target).
    let host_header = match parsed.port {
        Some(port) if port != 80 && port != 443 => format!("{}:{}", parsed.host, port),
        _ => parsed.host.clone(),
    };
    headers.insert("Host".to_string(), host_header);

    // Log the target URL being called
    tracing::debug!(
        "proxy_http {} {} ({} opts segments, {} headers, {} cookies) - Headers:{:?}",
        method,
        target_url_str,
        parsed.opts_encoded.len(),
        headers.len(),
        cookies.len(),
        headers
    );

    // Execute proxy request
    let client = SimpleHttpClient::new((*proxy_core).clone());
    let proxy_request = ProxiedHttpRequest {
        url: target_url_str.clone(),
        method: method.clone(),
        headers,
        cookies,
        body: input.body,
    };

    let proxy_response = match client.request_proxied(proxy_request).await {
        Ok(response) => response,
        Err(error) => return Ok(stream_error(502, format!("Proxy error: {error}"))),
    };

    let mut response_headers = proxy_response.headers;

    // Rewrite Location header for redirects
    if (300..400).contains(&proxy_response.status) {
        if let Err(error) = rewrite_location_header(
            &mut response_headers,
            &target_url_str,
            &parsed.opts_encoded,
            &input.entry_point,
        ) {
            return Ok(stream_error(502, error));
        }
    }
    filter_non_transferable(&mut response_headers);

    // Derive content-type
    let content_type = remove_header_case_insensitive(&mut response_headers, "content-type")
        .unwrap_or_else(|| "application/octet-stream".to_string());

    // Handle HEAD: return no body
    let body = if method == "HEAD" {
        Vec::new()
    } else {
        proxy_response.body
    };

    Ok(ControlerStreamOutput {
        status: proxy_response.status,
        body,
        content_type,
        headers: response_headers,
    })
}

/// Registers the proxy HTTP stream handler on a controller service.
///
/// The handler is registered under the name `"proxy"` and routes through the
/// given proxy core. The entry-point URL for `Location` header rewriting is
/// read directly from `ControlerStreamInput::entry_point`, which is populated
/// by the controller backend at request time.
///
/// # Parameters
///
/// - `controler`: Mutable reference to a controller service that will host the handler.
/// - `proxy_core`: Reference to the proxy core used for outbound HTTP requests.
/// - ``
pub fn register_service(
    controler: &mut dyn ControlerService,
    proxy_core: &ArachneaProxyCore,
    name: &str,
) {
    let core = Arc::new(proxy_core.clone());
    controler.register_stream_function_with_state(name, core, |core, input| async move {
        handle_proxy_http(core, input).await
    });
}
