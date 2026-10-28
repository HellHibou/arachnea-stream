//! HTTP header filtering, validation and request routing context extraction.

use std::collections::HashMap;

use crate::core::{normalize_parameter_value, ClientContext, ClientParameter, ParameterDefinition};

/// Filters out non-transferable headers.
///
/// # Arguments
/// * `headers` - HTTP headers inspected or updated by this operation.
///
pub(super) fn filter_non_transferable(headers: &mut HashMap<String, String>) {
    headers.retain(|key, _| {
        let lower = key.to_ascii_lowercase();
        !crate::PROXY_HTTP_REQUEST_NON_TRANSFERABLE_HEADERS.contains(&lower.as_str())
    });
}

/// Filters hop-by-hop headers from an upstream response while preserving its
/// valid content length for streamed responses.
///
/// # Arguments
/// * `headers` - HTTP headers inspected or updated by this operation.
///
pub(super) fn filter_response_non_transferable(headers: &mut HashMap<String, String>) {
    headers.retain(|key, _| {
        let lower = key.to_ascii_lowercase();
        !crate::PROXY_HTTP_RESPONSE_NON_TRANSFERABLE_HEADERS.contains(&lower.as_str())
    });
}

/// Removes every spelling of a header name using ASCII-insensitive comparison.
///
/// # Arguments
/// * `headers` - HTTP headers inspected or updated by this operation.
/// * `wanted` - Header name matched without regard to ASCII case.
pub(super) fn remove_header_variants(headers: &mut HashMap<String, String>, wanted: &str) {
    let keys: Vec<String> = headers
        .keys()
        .filter(|name| name.eq_ignore_ascii_case(wanted))
        .cloned()
        .collect();
    for key in keys {
        headers.remove(&key);
    }
}

/// Removes and returns one matching header; duplicate case variants are left intact.
///
/// # Arguments
/// * `headers` - Mutable header map; one matching entry is removed and its value returned.
/// * `wanted` - Header name matched without regard to ASCII case.
pub(super) fn remove_header_case_insensitive(
    headers: &mut HashMap<String, String>,
    wanted: &str,
) -> Option<String> {
    let key = headers
        .keys()
        .find(|name| name.eq_ignore_ascii_case(wanted))
        .cloned()?;
    headers.remove(&key)
}

/// Extracts and normalizes declared client parameters from HTTP headers.
///
/// # Arguments
/// * `headers` - HTTP headers inspected or updated by this operation.
/// * `definitions` - Parameter definitions specifying header names and forwarding policy.
pub(super) fn context_from_header_map(
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

/// Removes parameter headers whose definitions forbid upstream forwarding.
///
/// # Arguments
/// * `headers` - HTTP headers inspected or updated by this operation.
/// * `definitions` - Parameter definitions specifying header names and forwarding policy.
pub(super) fn strip_non_forwarded_parameter_headers(
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
///
/// # Arguments
/// * `headers` - HTTP headers inspected or updated by this operation.
///
/// # Errors
///
/// Rejects empty, oversized or invalid header names, oversized values, and values containing CR or LF.
pub(super) fn validate_headers(headers: &[(String, String)]) -> Result<(), String> {
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
///
/// # Arguments
/// * `cookies` - Cookie name/value pairs to validate before building the Cookie header.
///
/// # Errors
///
/// Rejects empty, oversized or invalid names and values containing prohibited characters or exceeding 4096 bytes.
pub(super) fn validate_cookies(cookies: &HashMap<String, String>) -> Result<(), String> {
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
