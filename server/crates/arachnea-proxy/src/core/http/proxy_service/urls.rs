//! Parsing and construction of proxy paths and public proxy URLs.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use url::Url;

use crate::core::http::actions::{proxy_action_header, ProxyHttpActionConfig};
use crate::core::{PROXY_HEADER_PARAMETER_AFFINITY, PROXY_HEADER_PARAMETER_COUNTRIES};

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
pub(super) fn build_target_url(
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
pub(super) fn encode_proxy_path(
    api_prefix: &str,
    resolved_url: &Url,
    opts_encoded: &str,
) -> String {
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
pub(super) fn entry_point_location_prefix(entry_point: &str) -> String {
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

/// Parses the proxy path segments into the target URL components.
///
/// The path format is: `[opts_<BASE64>/]<PROTOCOL>[:<PORT>]/<HOST>/<PATH>`
/// where `<PATH>` may include further `/` segments.
pub(super) struct ParsedProxyPath {
    pub(super) opts_encoded: String,
    pub(super) protocol: String,
    pub(super) port: Option<u16>,
    pub(super) host: String,
    pub(super) path_and_query: String,
}

pub(super) fn parse_proxy_path(path: &str) -> Result<ParsedProxyPath, (u16, String)> {
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

    // Next segment must be protocol[:port]
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

/// Builds a proxied media URL carrying a canonical JSON set of allowed countries.
pub fn proxied_url_with_countries(
    media_locator: &str,
    http_proxy_public_path: Option<&str>,
    countries: &[String],
    actions: &[ProxyHttpActionConfig],
    headers: &[(&str, &str)],
) -> String {
    proxied_url_with_countries_and_internal_options(
        media_locator,
        http_proxy_public_path,
        countries,
        None,
        &[],
        actions,
        headers,
        false,
        None,
    )
}

/// Builds a proxied media URL carrying countries, affinity and rejection statuses.
pub fn proxied_url_with_countries_and_policy(
    media_locator: &str,
    http_proxy_public_path: Option<&str>,
    countries: &[String],
    proxy_affinity: Option<&str>,
    proxy_rejection_statuses: &[u16],
    actions: &[ProxyHttpActionConfig],
    headers: &[(&str, &str)],
) -> String {
    proxied_url_with_countries_and_internal_options(
        media_locator,
        http_proxy_public_path,
        countries,
        proxy_affinity,
        proxy_rejection_statuses,
        actions,
        headers,
        false,
        None,
    )
}

/// Builds a proxied media URL carrying a canonical JSON set of allowed countries and an
/// optional TLS bypass flag.
pub fn proxied_url_with_countries_and_insecure_tls(
    media_locator: &str,
    http_proxy_public_path: Option<&str>,
    countries: &[String],
    actions: &[ProxyHttpActionConfig],
    headers: &[(&str, &str)],
    insecure_tls: bool,
) -> String {
    proxied_url_with_countries_and_internal_options(
        media_locator,
        http_proxy_public_path,
        countries,
        None,
        &[],
        actions,
        headers,
        insecure_tls,
        None,
    )
}

/// Builds a proxied media URL carrying countries and redirect-following settings.
pub fn proxied_url_with_countries_and_options(
    media_locator: &str,
    http_proxy_public_path: Option<&str>,
    countries: &[String],
    actions: &[ProxyHttpActionConfig],
    headers: &[(&str, &str)],
    follow_redirects: Option<serde_json::Value>,
) -> String {
    proxied_url_with_countries_and_internal_options(
        media_locator,
        http_proxy_public_path,
        countries,
        None,
        &[],
        actions,
        headers,
        false,
        follow_redirects,
    )
}

fn proxied_url_with_countries_and_internal_options(
    media_locator: &str,
    http_proxy_public_path: Option<&str>,
    countries: &[String],
    proxy_affinity: Option<&str>,
    proxy_rejection_statuses: &[u16],
    actions: &[ProxyHttpActionConfig],
    headers: &[(&str, &str)],
    insecure_tls: bool,
    follow_redirects: Option<serde_json::Value>,
) -> String {
    let countries = crate::core::canonical_proxy_countries(countries);
    let mut all_headers = headers.to_vec();
    let encoded_countries = (!countries.is_empty())
        .then(|| serde_json::to_string(&countries).expect("country list serializes"));
    if let Some(encoded_countries) = &encoded_countries {
        all_headers.push((PROXY_HEADER_PARAMETER_COUNTRIES, encoded_countries.as_str()));
    }
    if let Some(affinity) = proxy_affinity
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        all_headers.push((PROXY_HEADER_PARAMETER_AFFINITY, affinity));
    }
    proxied_url_with_internal_options(
        media_locator,
        http_proxy_public_path,
        actions,
        &all_headers,
        insecure_tls,
        follow_redirects,
        proxy_rejection_statuses,
    )
}

fn proxied_url_with_internal_options(
    media_locator: &str,
    http_proxy_public_path: Option<&str>,
    actions: &[ProxyHttpActionConfig],
    headers: &[(&str, &str)],
    insecure_tls: bool,
    follow_redirects: Option<serde_json::Value>,
    proxy_rejection_statuses: &[u16],
) -> String {
    let normalized_media_locator = media_locator.trim();
    if normalized_media_locator.starts_with("http://")
        || normalized_media_locator.starts_with("https://")
    {
        if let Some(proxy_path) = http_proxy_public_path
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let has_actions = !actions.is_empty();
            let has_extra_headers = !headers.is_empty();
            let has_follow_redirects = follow_redirects.is_some();
            let has_proxy_rejection_statuses = !proxy_rejection_statuses.is_empty();

            if has_actions
                || has_extra_headers
                || insecure_tls
                || has_follow_redirects
                || has_proxy_rejection_statuses
            {
                // Build proxy headers
                let mut proxy_headers: Vec<Vec<String>> = Vec::new();

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
                if has_proxy_rejection_statuses {
                    opts.insert(
                        "proxy_rejection_statuses".to_string(),
                        serde_json::json!(proxy_rejection_statuses),
                    );
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
