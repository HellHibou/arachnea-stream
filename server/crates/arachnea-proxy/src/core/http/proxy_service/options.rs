//! Parsing, validation, encoding and merging of proxy HTTP options.

use std::collections::HashMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

use crate::core::http::actions::{
    parse_proxy_action_header_into, parse_proxy_action_headers, ParsedProxyActionHeaders,
    ProxyHttpPostActionConfig, ProxyHttpRedirectActionConfig,
};

use super::headers::{filter_non_transferable, validate_cookies, validate_headers};

/// Known option fields allowed in the `opts` JSON.
const ALLOWED_OPTS_FIELDS: &[&str] = &[
    "headers",
    "cookies",
    "proxy",
    "insecure_tls",
    "follow_redirects",
    "proxy_rejection_statuses",
    "proxy_response_classifier",
    "proxy_retry_replayable",
];

/// Parsed options from the `opts` segment of a proxy URL.
#[derive(Clone)]
pub(super) struct ProxyHttpOpts {
    /// Headers as ordered pairs (from object or array-of-pairs format).
    pub(super) headers: Vec<(String, String)>,
    /// Cookies map.
    pub(super) cookies: HashMap<String, String>,
    /// Proxy configuration (reserved).
    #[allow(dead_code)]
    pub(super) proxy: serde_json::Value,
    /// Requests a TLS bypass for an allowlisted exact host.
    pub(super) insecure_tls: bool,
    /// Optional maximum number of upstream redirects to follow.
    pub(super) follow_redirects: Option<usize>,
    /// HTTP statuses that reject a destination association under the safe retry policy.
    pub(super) proxy_rejection_statuses: Vec<u16>,
    /// Known policy taking precedence over the legacy status list.
    pub(super) proxy_response_classifier:
        Option<crate::core::http::response_classifier::ProxyResponseClassifierConfig>,
    /// Explicit operation-level replay guarantee; request bodies are buffered and cloneable.
    pub(super) proxy_retry_replayable: bool,
}

pub(super) struct MergedProxyHttpInput {
    pub(super) headers: HashMap<String, String>,
    pub(super) cookies: HashMap<String, String>,
    pub(super) post_actions: Vec<ProxyHttpPostActionConfig>,
    pub(super) redirect_actions: Vec<ProxyHttpRedirectActionConfig>,
}

/// Decodes a base64url (no-padding) value from a path segment.
pub(super) fn decode_base64url_segment(encoded: &str) -> Result<Vec<u8>, String> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|e| format!("Invalid base64url: {}", e))
}

/// Validates and parses the `opts` JSON value into parsed options.
///
/// Supports both legacy object format for `headers` and the new array-of-pairs
/// format that preserves duplicate header names.
pub(super) fn parse_opts_value(value: &serde_json::Value) -> Result<ProxyHttpOpts, (u16, String)> {
    let obj = match value {
        serde_json::Value::Object(map) => map,
        serde_json::Value::Null => {
            return Ok(ProxyHttpOpts {
                headers: Vec::new(),
                cookies: HashMap::new(),
                proxy: serde_json::Value::Null,
                insecure_tls: false,
                follow_redirects: None,
                proxy_rejection_statuses: Vec::new(),
                proxy_retry_replayable: false,
                proxy_response_classifier: None,
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
    let proxy_rejection_statuses = match obj.get("proxy_rejection_statuses") {
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|value| u16::try_from(value).ok())
                    .filter(|value| (100..=599).contains(value))
                    .ok_or_else(|| {
                        (
                            400,
                            "'proxy_rejection_statuses' must contain HTTP status integers"
                                .to_string(),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => {
            return Err((
                400,
                "'proxy_rejection_statuses' must be an array".to_string(),
            ))
        }
        None => Vec::new(),
    };

    let proxy_response_classifier = obj
        .get("proxy_response_classifier")
        .map(|value| {
            let config: crate::core::http::response_classifier::ProxyResponseClassifierConfig =
                serde_json::from_value(value.clone()).map_err(|error| {
                    (400, format!("invalid proxy_response_classifier: {error}"))
                })?;
            config
                .validate()
                .map_err(|error| (400, error.to_string()))?;
            Ok::<_, (u16, String)>(config)
        })
        .transpose()?;
    Ok(ProxyHttpOpts {
        headers,
        cookies,
        proxy,
        insecure_tls,
        follow_redirects,
        proxy_rejection_statuses,
        proxy_response_classifier,
        proxy_retry_replayable: obj
            .get("proxy_retry_replayable")
            .map(|value| {
                value.as_bool().ok_or_else(|| {
                    (
                        400,
                        "'proxy_retry_replayable' must be a boolean".to_string(),
                    )
                })
            })
            .transpose()?
            .unwrap_or(false),
    })
}

pub(super) fn encode_proxy_opts(opts: &ProxyHttpOpts) -> String {
    if opts.headers.is_empty()
        && opts.cookies.is_empty()
        && opts.proxy.is_null()
        && !opts.insecure_tls
        && opts.follow_redirects.is_none()
        && opts.proxy_rejection_statuses.is_empty()
        && opts.proxy_response_classifier.is_none()
        && !opts.proxy_retry_replayable
    {
        return String::new();
    }

    let mut object = serde_json::Map::new();
    if opts.proxy_retry_replayable {
        object.insert("proxy_retry_replayable".into(), serde_json::json!(true));
    }
    if let Some(config) = &opts.proxy_response_classifier {
        object.insert(
            "proxy_response_classifier".into(),
            serde_json::json!(config),
        );
    }
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
    if !opts.proxy_rejection_statuses.is_empty() {
        object.insert(
            "proxy_rejection_statuses".to_string(),
            serde_json::json!(opts.proxy_rejection_statuses),
        );
    }

    URL_SAFE_NO_PAD.encode(serde_json::Value::Object(object).to_string())
}

/// Merges frontend opts headers/cookies over the incoming headers/cookies.
///
/// Priority: incoming < opts (opts wins on conflict).
pub(super) fn merge_opts(
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
