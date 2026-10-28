//! RMC/BFM Play payload traversal and bounded diagnostic descriptions.

use anyhow::{Context, Result};
use serde_json::Value;

use crate::PLAYER_ERROR_BODY_PREVIEW_LENGTH;

/// Summarizes the section types present in a BFF page.
///
/// # Arguments
/// * `page` - Decoded BFF page payload.
///
/// # Returns
/// A diagnostic string listing section types.
pub(super) fn describe_sections(page: &Value) -> String {
    let types = page
        .pointer("/sections")
        .and_then(Value::as_array)
        .map(|sections| {
            sections
                .iter()
                .filter_map(|section| section.get("type").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    format!("section types=[{types}]")
}

/// Describes an unexpected payload shape for diagnostics: object keys and
/// URLs with query strings redacted, never token values.
///
/// # Arguments
/// * `payload` - Payload to describe.
///
/// # Returns
/// A bounded diagnostic string with keys and redacted URLs.
pub(super) fn describe_value(payload: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(object) = payload.as_object() {
        let keys: Vec<_> = object.keys().map(String::as_str).collect();
        parts.push(format!("keys=[{}]", keys.join(",")));
    }
    if let Some(drm) = payload.pointer("/drm").and_then(Value::as_object) {
        let keys: Vec<_> = drm.keys().map(String::as_str).collect();
        parts.push(format!("drm.keys=[{}]", keys.join(",")));
        for system in ["widevine", "fairplay", "playready"] {
            if let Some(child) = drm.get(system).and_then(Value::as_object) {
                let keys: Vec<_> = child.keys().map(String::as_str).collect();
                parts.push(format!("drm.{system}.keys=[{}]", keys.join(",")));
            }
        }
    }
    if let Some(stream) = payload.pointer("/stream").and_then(Value::as_object) {
        let keys: Vec<_> = stream.keys().map(String::as_str).collect();
        parts.push(format!("stream.keys=[{}]", keys.join(",")));
    }
    let mut urls = Vec::new();
    collect_strings(payload, &mut urls, 20);
    let redacted: Vec<String> = urls
        .into_iter()
        .filter(|value| value.starts_with("http"))
        .map(|url| redact_url(&url))
        .collect();
    if !redacted.is_empty() {
        parts.push(format!("urls=[{}]", redacted.join(",")));
    }
    if parts.is_empty() {
        parts.push("non-object payload".to_string());
    }
    parts.join(" ")
}

/// Recursively collects string values up to a limit.
///
/// # Arguments
/// * `payload` - JSON value to traverse.
/// * `output` - Accumulator receiving strings.
/// * `limit` - Maximum number of strings to collect.
fn collect_strings(payload: &Value, output: &mut Vec<String>, limit: usize) {
    if output.len() >= limit {
        return;
    }
    match payload {
        Value::String(value) => output.push(value.clone()),
        Value::Array(items) => {
            for item in items {
                collect_strings(item, output, limit);
            }
        }
        Value::Object(entries) => {
            for value in entries.values() {
                collect_strings(value, output, limit);
            }
        }
        _ => {}
    }
}

/// Hides the query string of a URL for diagnostics.
///
/// # Arguments
/// * `url` - URL to redact.
///
/// # Returns
/// The base URL with `?<redacted>` when a query was present.
fn redact_url(url: &str) -> String {
    match url.split_once('?') {
        Some((base, _)) => format!("{base}?<redacted>"),
        None => url.to_string(),
    }
}

/// Reads a trimmed non-empty string at a JSON pointer.
///
/// # Arguments
/// * `payload` - JSON value to query.
/// * `pointer` - JSON pointer selecting the string.
///
/// # Returns
/// The trimmed string, or `None` when absent, blank or not a string.
pub(super) fn read_opt_string(payload: &Value, pointer: &str) -> Option<String> {
    payload
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Returns the first HTTP URL containing a marker.
///
/// # Arguments
/// * `payload` - JSON value to search.
/// * `marker` - Required URL substring.
///
/// # Returns
/// The first matching URL, or `None` when none exists.
pub(super) fn find_url_containing(payload: &Value, marker: &str) -> Option<String> {
    find_all_urls_containing(payload, marker).into_iter().next()
}

/// Collects HTTP URLs containing a marker.
///
/// # Arguments
/// * `payload` - JSON value to search.
/// * `marker` - Required URL substring.
///
/// # Returns
/// Matching URLs in traversal order, bounded by the collection limit.
pub(super) fn find_all_urls_containing(payload: &Value, marker: &str) -> Vec<String> {
    let mut all = Vec::new();
    collect_strings(payload, &mut all, 50);
    all.into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| value.starts_with("http") && value.contains(marker))
        .collect()
}

/// Returns the first HTTP string found in a payload.
///
/// # Arguments
/// * `payload` - JSON value to search.
///
/// # Returns
/// The first trimmed string starting with `http`, or `None`.
pub(super) fn find_http_string(payload: &Value) -> Option<String> {
    let mut found = None;
    find_string(payload, &mut found, &|value| value.starts_with("http"));
    found
}

/// Finds an HTTP URL stored under a key containing a marker.
///
/// # Arguments
/// * `payload` - JSON value to search.
/// * `key_marker` - Case-insensitive key substring.
///
/// # Returns
/// The first matching HTTP URL, or `None`.
pub(super) fn find_keyed_url(payload: &Value, key_marker: &str) -> Option<String> {
    match payload {
        Value::Object(object) => {
            for (key, value) in object {
                if key.to_lowercase().contains(key_marker) {
                    if let Some(url) = value
                        .as_str()
                        .map(str::trim)
                        .filter(|value| value.starts_with("http"))
                    {
                        return Some(url.to_string());
                    }
                }
                if let Some(url) = find_keyed_url(value, key_marker) {
                    return Some(url);
                }
            }
            None
        }
        Value::Array(items) => {
            for item in items {
                if let Some(url) = find_keyed_url(item, key_marker) {
                    return Some(url);
                }
            }
            None
        }
        _ => None,
    }
}

/// Finds the first string matching a predicate.
///
/// # Arguments
/// * `payload` - JSON value to search.
/// * `found` - Accumulator receiving the first match.
/// * `matches` - Predicate applied to trimmed string values.
fn find_string(payload: &Value, found: &mut Option<String>, matches: &dyn Fn(&str) -> bool) {
    if found.is_some() {
        return;
    }
    match payload {
        Value::String(value) if matches(value.trim()) => {
            *found = Some(value.trim().to_string());
        }
        Value::Array(items) => {
            for item in items {
                find_string(item, found, matches);
            }
        }
        Value::Object(entries) => {
            for value in entries.values() {
                find_string(value, found, matches);
            }
        }
        _ => {}
    }
}

/// Reads a non-empty string at a JSON pointer.
///
/// # Arguments
/// * `payload` - JSON value to query.
/// * `pointer` - JSON pointer selecting the string.
/// * `error` - Error message used when the value is absent or blank.
///
/// # Errors
/// Returns an error when the pointer does not resolve to a non-empty string.
///
/// # Returns
/// The trimmed string value.
pub(super) fn read_string(payload: &Value, pointer: &str, error: &str) -> Result<String> {
    payload
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .context(error.to_string())
}

/// Produces a bounded preview of an HTTP error body.
///
/// # Arguments
/// * `body` - Raw response body.
///
/// # Returns
/// A trimmed preview capped by `PLAYER_ERROR_BODY_PREVIEW_LENGTH`, with an
/// ellipsis when longer, or `<empty body>` when blank.
pub(super) fn preview_error_body(body: &str) -> String {
    let value = body.trim();
    if value.is_empty() {
        return "<empty body>".to_string();
    }

    let mut preview: String = value
        .chars()
        .take(PLAYER_ERROR_BODY_PREVIEW_LENGTH)
        .collect();
    if value.chars().count() > 300 {
        preview.push_str("...");
    }
    preview
}
