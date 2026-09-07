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

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use url::Url;

use crate::core::http::actions::{
    parse_proxy_action_header_into, parse_proxy_action_headers,
    post_actions_require_body_buffering, proxy_action_header,
    should_remove_opts_header_on_redirect, ParsedProxyActionHeaders, PostActionContext,
    ProxyHttpActionConfig, ProxyHttpPostActionConfig, ProxyHttpRedirectActionConfig,
};
use crate::core::http::{ProxiedHttpRequest, ProxiedResponseBody, SimpleHttpClient};
use crate::core::{
    normalize_parameter_value, ArachneaProxyCore, ClientContext, ClientParameter,
    ParameterDefinition, PROXY_HEADER_PARAMETER_COUNTRY,
};
use arachnea_core::controler::{
    ControlerService, ControlerServiceExt, ControlerStreamInput, ControlerStreamOutput,
    ResponseBody,
};

const MAX_LOCAL_URL_BYTES: usize = 8192;

/// Known option fields allowed in the `opts` JSON.
const ALLOWED_OPTS_FIELDS: &[&str] = &[
    "headers",
    "cookies",
    "proxy",
    "insecure_tls",
    "follow_redirects",
];

/// Parsed options from the `opts` segment of a proxy URL.
#[derive(Clone)]
struct ProxyHttpOpts {
    /// Headers as ordered pairs (from object or array-of-pairs format).
    headers: Vec<(String, String)>,
    /// Cookies map.
    cookies: HashMap<String, String>,
    /// Proxy configuration (reserved).
    #[allow(dead_code)]
    proxy: serde_json::Value,
    /// Requests a TLS bypass for an allowlisted exact host.
    insecure_tls: bool,
    /// Optional maximum number of upstream redirects to follow.
    follow_redirects: Option<usize>,
}

struct MergedProxyHttpInput {
    headers: HashMap<String, String>,
    cookies: HashMap<String, String>,
    post_actions: Vec<ProxyHttpPostActionConfig>,
    redirect_actions: Vec<ProxyHttpRedirectActionConfig>,
}

/// Decodes a base64url (no-padding) value from a path segment.
fn decode_base64url_segment(encoded: &str) -> Result<Vec<u8>, String> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|e| format!("Invalid base64url: {}", e))
}

/// Validates and parses the `opts` JSON value into parsed options.
///
/// Supports both legacy object format for `headers` and the new array-of-pairs
/// format that preserves duplicate header names.
fn parse_opts_value(value: &serde_json::Value) -> Result<ProxyHttpOpts, (u16, String)> {
    let obj = match value {
        serde_json::Value::Object(map) => map,
        serde_json::Value::Null => {
            return Ok(ProxyHttpOpts {
                headers: Vec::new(),
                cookies: HashMap::new(),
                proxy: serde_json::Value::Null,
                insecure_tls: false,
                follow_redirects: None,
            });
        }
        _ => return Err((400, "'opts' must be a JSON object".to_string())),
    };

    // Validate allowed fields
    for key in obj.keys() {
        if !ALLOWED_OPTS_FIELDS.contains(&key.as_str()) {
            return Err((404, format!("Unknown option field '{}'", key)));
        }
    }

    // Parse headers
    let headers = if let Some(headers_val) = obj.get("headers") {
        match headers_val {
            serde_json::Value::Object(map) => {
                // Legacy object format: {"name": "value", ...}
                map.iter()
                    .map(|(k, v)| {
                        let val = v.as_str().unwrap_or("").to_string();
                        (k.clone(), val)
                    })
                    .collect()
            }
            serde_json::Value::Array(arr) => {
                // Array-of-pairs format: [["name", "value"], ...]
                let mut pairs = Vec::with_capacity(arr.len());
                for (i, item) in arr.iter().enumerate() {
                    match item {
                        serde_json::Value::Array(pair) if pair.len() == 2 => {
                            let name = pair[0].as_str().ok_or_else(|| {
                                (400, format!("headers[{}][0] must be a string", i))
                            })?;
                            let value = pair[1].as_str().ok_or_else(|| {
                                (400, format!("headers[{}][1] must be a string", i))
                            })?;
                            pairs.push((name.to_string(), value.to_string()));
                        }
                        _ => {
                            return Err((
                                400,
                                format!("headers[{}] must be a string pair [name, value]", i),
                            ))
                        }
                    }
                }
                pairs
            }
            _ => {
                return Err((
                    400,
                    "'headers' must be a JSON object or array of pairs".to_string(),
                ))
            }
        }
    } else {
        Vec::new()
    };

    // Parse cookies
    let cookies = if let Some(cookies_val) = obj.get("cookies") {
        match cookies_val {
            serde_json::Value::Object(map) => map
                .iter()
                .map(|(k, v)| {
                    let val = v.as_str().unwrap_or("").to_string();
                    (k.clone(), val)
                })
                .collect(),
            _ => return Err((400, "'cookies' must be a JSON object".to_string())),
        }
    } else {
        HashMap::new()
    };

    let proxy = obj.get("proxy").cloned().unwrap_or(serde_json::Value::Null);
    let insecure_tls = match obj.get("insecure_tls") {
        Some(serde_json::Value::Bool(value)) => *value,
        Some(_) => return Err((400, "'insecure_tls' must be a boolean".to_string())),
        None => false,
    };
    let follow_redirects = match obj.get("follow_redirects") {
        Some(serde_json::Value::Bool(true)) => Some(arachnea_core::DEFAULT_MAX_REDIRECTS),
        Some(serde_json::Value::Bool(false)) => None,
        Some(serde_json::Value::Number(value)) => {
            let limit = value
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    (
                        400,
                        "'follow_redirects' must be true or a positive integer".to_string(),
                    )
                })?;
            Some(limit)
        }
        Some(_) => {
            return Err((
                400,
                "'follow_redirects' must be true, false, or a positive integer".to_string(),
            ))
        }
        None => None,
    };

    Ok(ProxyHttpOpts {
        headers,
        cookies,
        proxy,
        insecure_tls,
        follow_redirects,
    })
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

/// Filters hop-by-hop headers from an upstream response while preserving its
/// valid content length for streamed responses.
fn filter_response_non_transferable(headers: &mut HashMap<String, String>) {
    const HOP_BY_HOP: &[&str] = &[
        "connection",
        "transfer-encoding",
        "host",
        "proxy-authorization",
        "proxy-connection",
    ];
    headers.retain(|key, _| {
        let lower = key.to_ascii_lowercase();
        !HOP_BY_HOP.contains(&lower.as_str())
    });
}

fn remove_header_variants(headers: &mut HashMap<String, String>, wanted: &str) {
    let keys: Vec<String> = headers
        .keys()
        .filter(|name| name.eq_ignore_ascii_case(wanted))
        .cloned()
        .collect();
    for key in keys {
        headers.remove(&key);
    }
}

fn stream_error(status: u16, message: impl Into<String>) -> ControlerStreamOutput {
    ControlerStreamOutput {
        status,
        body: ResponseBody::Buffered(message.into().into_bytes()),
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

fn context_from_header_map(
    headers: &HashMap<String, String>,
    definitions: &[ParameterDefinition],
) -> ClientContext {
    let mut context = ClientContext::new();
    for definition in definitions {
        if let Some((_, value)) = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&definition.http_header))
        {
            context.insert(
                definition.name.clone(),
                ClientParameter::String(normalize_parameter_value(&definition.name, value)),
            );
        }
    }
    context
}

fn strip_non_forwarded_parameter_headers(
    headers: &mut HashMap<String, String>,
    definitions: &[ParameterDefinition],
) {
    headers.retain(|name, _| {
        definitions.iter().all(|definition| {
            definition.forward_header || !name.eq_ignore_ascii_case(&definition.http_header)
        })
    });
}

/// Validates that header names and values are syntactically acceptable.
fn validate_headers(headers: &[(String, String)]) -> Result<(), String> {
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
///
/// When `api_prefix` starts with `/`, the result is a root-relative proxy path
/// so HTTP clients resolve it against the host they actually used. Otherwise
/// the prefix is kept verbatim (e.g. custom scheme entry points such as
/// `arachnea://api/proxy`).
fn encode_proxy_path(api_prefix: &str, resolved_url: &Url, opts_encoded: &str) -> String {
    let api = api_prefix.trim_matches('/');
    let protocol = resolved_url.scheme();
    let host = resolved_url.host_str().unwrap_or("");
    let port = resolved_url.port();
    let path = resolved_url.path().trim_start_matches('/');
    let query = resolved_url
        .query()
        .map(|q| format!("?{}", q))
        .unwrap_or_default();

    let port_segment = match port {
        Some(p) => format!(":{}", p),
        _none => format!(""),
    };

    let opts_segment = if opts_encoded.is_empty() {
        String::new()
    } else {
        format!("opts_{}/", opts_encoded)
    };

    // Root-relative prefixes keep their leading slash so clients resolve the
    // rewritten Location against the request host instead of the bind address.
    let prefix = if !api.is_empty() && api_prefix.starts_with('/') {
        format!("/{api}/")
    } else {
        format!("{api}/")
    };

    format!("{prefix}{opts_segment}{protocol}://{host}{port_segment}/{path}{query}")
}

/// Reduces an entry-point URL to its public path for Location rewriting.
///
/// http/https entry points are reduced to a root-relative path so redirect
/// Locations resolve against the host the client actually used rather than
/// the server bind address (which may be unspecified, e.g. `0.0.0.0`).
/// Custom scheme entry points (e.g. `arachnea://api/proxy`) are returned
/// unchanged so the rewritten Location stays an absolute custom-scheme URL.
fn entry_point_location_prefix(entry_point: &str) -> String {
    let normalized = entry_point.trim().trim_end_matches('/');
    let Ok(url) = Url::parse(normalized) else {
        return normalized.to_string();
    };
    if url.scheme() != "http" && url.scheme() != "https" {
        return normalized.to_string();
    }

    let path = url.path().trim_matches('/');
    if path.is_empty() {
        String::new()
    } else {
        format!("/{path}")
    }
}

fn encode_proxy_opts(opts: &ProxyHttpOpts) -> String {
    if opts.headers.is_empty()
        && opts.cookies.is_empty()
        && opts.proxy.is_null()
        && !opts.insecure_tls
        && opts.follow_redirects.is_none()
    {
        return String::new();
    }

    let mut object = serde_json::Map::new();
    if !opts.headers.is_empty() {
        let headers: Vec<Vec<String>> = opts
            .headers
            .iter()
            .map(|(name, value)| vec![name.clone(), value.clone()])
            .collect();
        object.insert("headers".to_string(), serde_json::json!(headers));
    }
    if !opts.cookies.is_empty() {
        object.insert("cookies".to_string(), serde_json::json!(opts.cookies));
    }
    if !opts.proxy.is_null() {
        object.insert("proxy".to_string(), opts.proxy.clone());
    }
    if let Some(limit) = opts.follow_redirects {
        object.insert("follow_redirects".to_string(), serde_json::json!(limit));
    }

    URL_SAFE_NO_PAD.encode(serde_json::Value::Object(object).to_string())
}

fn redirect_opts_encoded(
    status: u16,
    current_opts_encoded: &str,
    current_opts: Option<&ProxyHttpOpts>,
    redirect_actions: &[ProxyHttpRedirectActionConfig],
) -> String {
    if redirect_actions.is_empty() {
        return current_opts_encoded.to_string();
    }

    let Some(current_opts) = current_opts else {
        return current_opts_encoded.to_string();
    };

    let mut next_opts = current_opts.clone();
    next_opts
        .headers
        .retain(|(name, _)| !should_remove_opts_header_on_redirect(redirect_actions, status, name));
    encode_proxy_opts(&next_opts)
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

    let location_prefix = entry_point_location_prefix(api_prefix);
    let new_location = encode_proxy_path(&location_prefix, &resolved_url, current_opts_encoded);

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
) -> Result<MergedProxyHttpInput, (u16, String)> {
    let mut headers = incoming_headers.clone();
    let mut cookies = HashMap::new();
    let mut proxy_actions = parse_proxy_action_headers(&mut headers)?;

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
        validate_headers(&opts.headers).map_err(|error| (400, error))?;
        validate_cookies(&opts.cookies).map_err(|error| (400, error))?;

        // Merge headers: opts override incoming
        for (key, value) in &opts.headers {
            if !parse_proxy_action_header_into(&mut proxy_actions, key, value)? {
                // Remove existing variants with the same case-insensitive name
                // to prevent duplicates when the casing differs
                let variants: Vec<String> = headers
                    .keys()
                    .filter(|k| k.eq_ignore_ascii_case(key))
                    .cloned()
                    .collect();
                for variant in variants {
                    headers.remove(&variant);
                }
                headers.insert(key.clone(), value.clone());
            }
        }
        filter_non_transferable(&mut headers);

        // Merge cookies: opts override incoming
        for (key, value) in &opts.cookies {
            cookies.insert(key.clone(), value.clone());
        }
    }

    let ParsedProxyActionHeaders {
        post_actions,
        redirect_actions,
    } = proxy_actions;

    Ok(MergedProxyHttpInput {
        headers,
        cookies,
        post_actions,
        redirect_actions,
    })
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

        let opts_parsed = match parse_opts_value(&opts_value) {
            Ok(opts) => opts,
            Err((status, message)) => return Ok(stream_error(status, message)),
        };

        Some(opts_parsed)
    };

    let redirect_opts = opts.clone();

    let verify_tls = if opts.as_ref().is_some_and(|opts| opts.insecure_tls) {
        let Some(host) = parsed_target.host_str() else {
            return Ok(stream_error(400, "HTTPS target is missing a host"));
        };
        if parsed_target.scheme() != "https" || !proxy_core.allows_insecure_tls_for_host(host) {
            return Ok(stream_error(
                403,
                format!("TLS bypass is not allowed for host '{host}'"),
            ));
        }
        tracing::warn!(
            host,
            "TLS certificate validation bypassed for allowlisted proxy host"
        );
        false
    } else {
        true
    };

    // Strip incoming Referer that points back to the proxy itself —
    // the proxy's own URL is meaningless to the upstream server.
    // Explicit Referer via opts headers will still be applied below.
    let mut incoming_headers = input.headers.clone();
    if let Some(referer_val) = incoming_headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("referer"))
        .map(|(_, v)| v.clone())
    {
        if referer_val.starts_with(&input.entry_point) {
            remove_header_variants(&mut incoming_headers, "referer");
        }
    }

    // Merge headers and cookies
    let MergedProxyHttpInput {
        mut headers,
        cookies,
        post_actions,
        redirect_actions,
    } = match merge_opts(&incoming_headers, opts) {
        Ok(value) => value,
        Err((status, error)) => return Ok(stream_error(status, error)),
    };
    let parameter_definitions = proxy_core.parameter_definitions();
    let client_context = context_from_header_map(&headers, &parameter_definitions);
    strip_non_forwarded_parameter_headers(&mut headers, &parameter_definitions);

    // Set the Host header from the target URL (host is removed as non-transferable
    //    from the incoming request since it refers to the proxy, not the target).
    let host_header = match parsed.port {
        Some(port) if port != 80 && port != 443 => format!("{}:{}", parsed.host, port),
        _ => parsed.host.clone(),
    };
    headers.insert("Host".to_string(), host_header);

    // When post-actions require body buffering, force Accept-Encoding: identity
    // to avoid dealing with compressed bodies. In streaming mode (no buffering),
    // leave Accept-Encoding untouched so the upstream can send compressed data.
    // When the controller backend does not support streaming (force_buffer_response),
    // also force buffering to avoid streaming-specific issues (e.g. strict chunked
    // encoding parsing on the streaming path combined with a block_in_place context
    // in Tauri that can cause waker-propagation issues).
    let buffer_response_body =
        post_actions_require_body_buffering(&post_actions) || input.force_buffer_response;
    if buffer_response_body {
        remove_header_variants(&mut headers, "accept-encoding");
        headers.insert("Accept-Encoding".to_string(), "identity".to_string());
    }

    // Log the target URL being called
    tracing::debug!(
        "proxy_http {} {} ({} opts segments, {} headers, {} cookies, {} post_actions) - Headers:{:?}",
        method,
        target_url_str,
        parsed.opts_encoded.len(),
        headers.len(),
        cookies.len(),
        post_actions.len(),
        headers
    );

    // Execute proxy request
    let client = SimpleHttpClient::new((*proxy_core).clone());
    let headers_only = method == "HEAD";
    let proxy_request = ProxiedHttpRequest {
        url: target_url_str.clone(),
        method: method.clone(),
        headers,
        cookies,
        body: input.body,
        client_context,
        post_actions,
        buffer_response_body,
        headers_only,
        context: PostActionContext {
            entry_point: input.entry_point.clone(),
            target_url: target_url_str.clone(),
            opts_encoded: parsed.opts_encoded.clone(),
        },
        verify_tls,
        follow_redirects: redirect_opts
            .as_ref()
            .and_then(|opts| opts.follow_redirects),
    };

    let proxy_response = match client.request_proxied(proxy_request).await {
        Ok(response) => response,
        Err(error) => return Ok(stream_error(502, format!("Proxy error: {error}"))),
    };

    let status = proxy_response.status;
    let mut response_headers = proxy_response.headers;

    // Rewrite Location header for redirects
    if (300..400).contains(&status) {
        let redirect_opts_encoded = redirect_opts_encoded(
            status,
            &parsed.opts_encoded,
            redirect_opts.as_ref(),
            &redirect_actions,
        );
        if let Err(error) = rewrite_location_header(
            &mut response_headers,
            &target_url_str,
            &redirect_opts_encoded,
            &input.entry_point,
        ) {
            return Ok(stream_error(502, error));
        }
    }
    filter_response_non_transferable(&mut response_headers);

    // Derive content-type
    let content_type = remove_header_case_insensitive(&mut response_headers, "content-type")
        .unwrap_or_else(|| "application/octet-stream".to_string());

    // Handle HEAD: return no body
    let body = if method == "HEAD" {
        ResponseBody::Buffered(Vec::new())
    } else {
        match proxy_response.body {
            ProxiedResponseBody::Buffered(bytes) => ResponseBody::Buffered(bytes),
            ProxiedResponseBody::Streamed(stream) => ResponseBody::Streamed(stream),
        }
    };

    Ok(ControlerStreamOutput {
        status,
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

/// Constructs a URL for proxied media access.
///
/// If the provided `media_locator` starts with `http://` or `https://` and a
/// non-empty `http_proxy_public_path` is supplied, the function returns a
/// combination of the trimmed proxy path and the media locator. When `country`
/// is supplied, the generated proxy URL carries the proxy country header in an
/// `opts` segment. When actions are supplied, they are included as proxy action
/// headers in the `opts` segment. Otherwise it returns the `media_locator`
/// unchanged.
///
/// # Arguments
/// - `media_locator`: The location of the media resource. If it starts with
///   `http://` or `https://`, it may be combined with a proxy path.
/// - `http_proxy_public_path`: An optional path to be used as the public
///   proxy base. If provided and non-empty, it is combined with
///   `media_locator` when the latter is an HTTP URL.
/// - `country`: Optional country or region hint passed to the proxy country
///   routing parameter. When omitted or empty, no country routing is requested.
/// - `actions`: Post-response actions to include in the proxy URL. When
///   non-empty, the `opts` segment uses an array-of-pairs format for `headers`
///   to preserve duplicate action headers.
///
/// # Returns
/// A String representing either the combined proxied URL or the original
/// `media_locator` if no combination is needed.
///
/// # Examples
/// ```
/// let url = proxied_url("http://example.com/media", Some("/proxy"), None, &[], &[]);
/// assert_eq!(url, "/proxy/http://example.com/media");
/// ```
pub fn proxied_url(
    media_locator: &str,
    http_proxy_public_path: Option<&str>,
    country: Option<&str>,
    actions: &[ProxyHttpActionConfig],
    headers: &[(&str, &str)],
) -> String {
    proxied_url_with_internal_options(
        media_locator,
        http_proxy_public_path,
        country,
        actions,
        headers,
        false,
        None,
    )
}

/// Constructs a proxied media URL with optional redirect-following settings.
pub fn proxied_url_with_options(
    media_locator: &str,
    http_proxy_public_path: Option<&str>,
    country: Option<&str>,
    actions: &[ProxyHttpActionConfig],
    headers: &[(&str, &str)],
    follow_redirects: Option<serde_json::Value>,
) -> String {
    proxied_url_with_internal_options(
        media_locator,
        http_proxy_public_path,
        country,
        actions,
        headers,
        false,
        follow_redirects,
    )
}

/// Constructs a proxied media URL with an explicit TLS-bypass request.
///
/// The request is still rejected by the proxy unless the target host appears in
/// its trusted server-side exact-host allowlist.
pub fn proxied_url_with_insecure_tls(
    media_locator: &str,
    http_proxy_public_path: Option<&str>,
    country: Option<&str>,
    actions: &[ProxyHttpActionConfig],
    headers: &[(&str, &str)],
    insecure_tls: bool,
) -> String {
    proxied_url_with_internal_options(
        media_locator,
        http_proxy_public_path,
        country,
        actions,
        headers,
        insecure_tls,
        None,
    )
}

fn proxied_url_with_internal_options(
    media_locator: &str,
    http_proxy_public_path: Option<&str>,
    country: Option<&str>,
    actions: &[ProxyHttpActionConfig],
    headers: &[(&str, &str)],
    insecure_tls: bool,
    follow_redirects: Option<serde_json::Value>,
) -> String {
    let normalized_media_locator = media_locator.trim();
    if normalized_media_locator.starts_with("http://")
        || normalized_media_locator.starts_with("https://")
    {
        if let Some(proxy_path) = http_proxy_public_path
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let has_country = country
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some();
            let has_actions = !actions.is_empty();
            let has_extra_headers = !headers.is_empty();
            let has_follow_redirects = follow_redirects.is_some();

            if has_country
                || has_actions
                || has_extra_headers
                || insecure_tls
                || has_follow_redirects
            {
                // Build proxy headers
                let mut proxy_headers: Vec<Vec<String>> = Vec::new();

                // Country header
                if let Some(country) = country.map(str::trim).filter(|value| !value.is_empty()) {
                    proxy_headers.push(vec![
                        PROXY_HEADER_PARAMETER_COUNTRY.to_string(),
                        country.to_string(),
                    ]);
                }

                // Action headers (preserving duplicates)
                for action in actions {
                    if let Some((action_header, action_value)) = proxy_action_header(action) {
                        proxy_headers.push(vec![action_header.to_string(), action_value]);
                    }
                }

                // Extra headers embedded as literal HTTP headers
                for (name, value) in headers {
                    let trimmed_name = name.trim();
                    let trimmed_value = value.trim();
                    if !trimmed_name.is_empty() && !trimmed_value.is_empty() {
                        proxy_headers
                            .push(vec![trimmed_name.to_string(), trimmed_value.to_string()]);
                    }
                }

                let mut opts = serde_json::Map::new();
                if !proxy_headers.is_empty() {
                    opts.insert("headers".to_string(), serde_json::json!(proxy_headers));
                }
                if insecure_tls {
                    opts.insert("insecure_tls".to_string(), serde_json::Value::Bool(true));
                }
                if let Some(follow_redirects) = follow_redirects {
                    opts.insert("follow_redirects".to_string(), follow_redirects);
                }
                let opts_encoded =
                    URL_SAFE_NO_PAD.encode(serde_json::Value::Object(opts).to_string());

                return format!(
                    "{}/opts_{}/{}",
                    proxy_path.trim_end_matches('/'),
                    opts_encoded,
                    normalized_media_locator
                );
            }

            return format!(
                "{}/{}",
                proxy_path.trim_end_matches('/'),
                normalized_media_locator
            );
        }
    }

    normalized_media_locator.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::http::actions::{
        parse_proxy_action_header_value, ParsedProxyActionHeader, ProxyHttpRedirectActionConfig,
        ProxyHttpRemoveHeaderConfig, RemoveHeader, REMOVE_HEADER_ACTION_HEADER,
    };

    fn decode_opts(encoded: &str) -> serde_json::Value {
        let bytes = URL_SAFE_NO_PAD.decode(encoded).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn parses_remove_header_json_action() {
        let action = parse_proxy_action_header_value(
            REMOVE_HEADER_ACTION_HEADER,
            r#"{"type":"onHttp302","headers":["Arachnea-Proxy-Country"]}"#,
        )
        .unwrap();

        match action {
            Some(ParsedProxyActionHeader::RedirectAction(
                ProxyHttpRedirectActionConfig::RemoveHeader(action),
            )) => {
                assert_eq!(action.trigger, "onHttp302");
                assert_eq!(action.headers, vec!["Arachnea-Proxy-Country".to_string()]);
            }
            _ => panic!("expected RemoveHeader action"),
        }
    }

    #[test]
    fn proxied_url_encodes_remove_header_action() {
        let url = proxied_url(
            "https://example.test/manifest.mpd",
            Some("/proxy"),
            Some("fr"),
            &[RemoveHeader::on_http302([
                PROXY_HEADER_PARAMETER_COUNTRY,
                REMOVE_HEADER_ACTION_HEADER,
            ])],
            &[],
        );

        let opts_encoded = url
            .split("/opts_")
            .nth(1)
            .and_then(|value| value.split('/').next())
            .expect("proxied URL should contain an opts segment");
        let opts = decode_opts(opts_encoded);
        let headers = opts
            .get("headers")
            .and_then(serde_json::Value::as_array)
            .expect("opts should contain headers");

        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0][0], PROXY_HEADER_PARAMETER_COUNTRY);
        assert_eq!(headers[0][1], "fr");
        assert_eq!(headers[1][0], REMOVE_HEADER_ACTION_HEADER);

        let action_value: serde_json::Value = serde_json::from_str(headers[1][1].as_str().unwrap())
            .expect("RemoveHeader value should be JSON");
        assert_eq!(action_value.get("type").unwrap(), "onHttp302");
        assert_eq!(
            action_value.get("headers").unwrap(),
            &serde_json::json!([PROXY_HEADER_PARAMETER_COUNTRY, REMOVE_HEADER_ACTION_HEADER])
        );
    }

    #[test]
    fn remove_header_updates_redirect_opts_on_http_302() {
        let opts = ProxyHttpOpts {
            headers: vec![
                ("Arachnea-Proxy-Country".to_string(), "fr".to_string()),
                (
                    REMOVE_HEADER_ACTION_HEADER.to_string(),
                    r#"{"type":"onHttp302","headers":["Arachnea-Proxy-Country","Arachnea-Proxy-RemoveHeader"]}"#.to_string(),
                ),
                ("X-Keep".to_string(), "1".to_string()),
            ],
            cookies: HashMap::new(),
            proxy: serde_json::Value::Null,
            insecure_tls: false,
            follow_redirects: None,
        };
        let encoded = encode_proxy_opts(&opts);
        let redirect_actions = vec![ProxyHttpRedirectActionConfig::RemoveHeader(
            ProxyHttpRemoveHeaderConfig {
                trigger: "onHttp302".to_string(),
                headers: vec![
                    "Arachnea-Proxy-Country".to_string(),
                    REMOVE_HEADER_ACTION_HEADER.to_string(),
                ],
            },
        )];

        let next_encoded = redirect_opts_encoded(302, &encoded, Some(&opts), &redirect_actions);
        let next = decode_opts(&next_encoded);
        let headers = next
            .get("headers")
            .and_then(serde_json::Value::as_array)
            .unwrap();

        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0][0], "X-Keep");
        assert_eq!(headers[0][1], "1");
    }

    #[test]
    fn encode_proxy_path_http_entry_point_is_root_relative() {
        let url = Url::parse("https://th2-edge-02.cdn.test/m6web/a.mpd").unwrap();
        assert_eq!(
            encode_proxy_path("/api/proxy", &url, "ABCD"),
            "/api/proxy/opts_ABCD/https://th2-edge-02.cdn.test/m6web/a.mpd"
        );
        assert_eq!(
            encode_proxy_path("/api/proxy", &url, ""),
            "/api/proxy/https://th2-edge-02.cdn.test/m6web/a.mpd"
        );
    }

    #[test]
    fn entry_point_location_prefix_reduces_http_to_path() {
        assert_eq!(
            entry_point_location_prefix("http://0.0.0.0:8080/api/proxy"),
            "/api/proxy"
        );
        assert_eq!(
            entry_point_location_prefix("http://192.168.178.34:8080/api/proxy"),
            "/api/proxy"
        );
        assert_eq!(entry_point_location_prefix("http://host:8080"), "");
        // Custom scheme entry points stay absolute.
        assert_eq!(
            entry_point_location_prefix("arachnea://api/proxy"),
            "arachnea://api/proxy"
        );
    }

    #[test]
    fn remove_header_keeps_redirect_opts_for_non_302() {
        let opts = ProxyHttpOpts {
            headers: vec![("Arachnea-Proxy-Country".to_string(), "fr".to_string())],
            cookies: HashMap::new(),
            proxy: serde_json::Value::Null,
            insecure_tls: false,
            follow_redirects: None,
        };
        let encoded = encode_proxy_opts(&opts);
        let redirect_actions = vec![ProxyHttpRedirectActionConfig::RemoveHeader(
            ProxyHttpRemoveHeaderConfig {
                trigger: "onHttp302".to_string(),
                headers: vec!["Arachnea-Proxy-Country".to_string()],
            },
        )];

        let next_encoded = redirect_opts_encoded(301, &encoded, Some(&opts), &redirect_actions);

        assert_eq!(next_encoded, encoded);
    }
}
