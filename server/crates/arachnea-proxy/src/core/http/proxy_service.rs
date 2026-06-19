//! Proxy HTTP handler for the controller stream route.
//!
//! This module implements the `/proxy_http/<target_url_base64url>` endpoint
//! that forwards HTTP requests through the Arachnea proxy core.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use serde::Deserialize;
use url::Url;

use arachnea_core::controler::{ControlerStreamInput, ControlerStreamOutput};
use crate::core::http::{ProxiedHttpRequest, SimpleHttpClient};
use crate::core::ArachneaProxyCore;

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

/// Determines the entrypoint prefix used for Location rewriting.
fn entrypoint_prefix(api_prefix: &str) -> String {
    format!("/{}/proxy_http/", api_prefix.trim_matches('/'))
}

/// Encodes a URL as a base64url segment for the proxy path.
fn encode_target_segment(target_url: &str) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    URL_SAFE_NO_PAD.encode(target_url.as_bytes())
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

    let resolved_url = resolve_location(&location, current_target_url)?;

    let new_target = encode_target_segment(&resolved_url);
    let prefix = entrypoint_prefix(api_prefix);
    let new_location = if current_opts_encoded.is_empty() {
        format!("{prefix}{new_target}")
    } else {
        format!("{prefix}{new_target}?opts={current_opts_encoded}")
    };

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

/// Handles a proxied HTTP request through the controller stream interface.
///
/// This is the main entry point registered as a stream function.
pub async fn handle_proxy_http(
    proxy_core: Arc<ArachneaProxyCore>,
    input: ControlerStreamInput,
    api_prefix: &str,
) -> Result<ControlerStreamOutput, String> {
    // 1. Validate URL length
    let full_url = format!(
        "/{}/proxy_http/{}{}{}",
        api_prefix.trim_matches('/'),
        &input.path,
        if input.query.is_empty() { "" } else { "?" },
        &input.query
    );
    if full_url.len() > MAX_LOCAL_URL_BYTES {
        return Ok(stream_error(414, "URI Too Long"));
    }

    // 2. Parse target_url_base64url from path
    let target_b64 = input.path.trim_matches('/');
    if target_b64.is_empty() {
        return Ok(stream_error(400, "missing target URL"));
    }

    let target_url_bytes = match decode_base64url_segment(target_b64) {
        Ok(bytes) => bytes,
        Err(error) => return Ok(stream_error(400, error)),
    };
    let target_url_str = match String::from_utf8(target_url_bytes) {
        Ok(value) => value,
        Err(_) => return Ok(stream_error(400, "target URL is not valid UTF-8")),
    };

    // 3. Validate target URL
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

    // 4. Parse opts from query string
    let opts_encoded = if input.query.is_empty() {
        String::new()
    } else if input.query.starts_with("opts=") {
        // Raw "opts=<base64>" - extract directly
        input.query[5..].to_string()
    } else {
        // Parse as query string parameters
        let params: HashMap<String, String> = match serde_urlencoded::from_str(&input.query) {
            Ok(params) => params,
            Err(error) => return Ok(stream_error(400, format!("invalid query string: {error}"))),
        };
        match params.get("opts") {
            Some(value) => value.clone(),
            None => return Ok(stream_error(400, "missing 'opts' parameter")),
        }
    };

    let opts = if opts_encoded.is_empty() {
        None
    } else {
        let opts_json_bytes = match decode_base64url_segment(&opts_encoded) {
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

    // 5. Merge headers and cookies
    let (headers, cookies) = match merge_opts(&input.headers, opts) {
        Ok(value) => value,
        Err(error) => return Ok(stream_error(400, error)),
    };

    // 6. Execute proxy request
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

    // 7. Rewrite Location header for redirects
    if (300..400).contains(&proxy_response.status) {
        if let Err(error) = rewrite_location_header(
            &mut response_headers,
            &target_url_str,
            &opts_encoded,
            api_prefix,
        ) {
            return Ok(stream_error(502, error));
        }
    }
    filter_non_transferable(&mut response_headers);

    // 8. Derive content-type
    let content_type = remove_header_case_insensitive(&mut response_headers, "content-type")
        .unwrap_or_else(|| "application/octet-stream".to_string());

    // 9. Handle HEAD: return no body
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
