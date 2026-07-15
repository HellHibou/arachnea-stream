//! Post-response action that replaces all occurrences of a regex pattern in
//! textual responses.

use std::collections::HashMap;

use encoding_rs::Encoding;
use regex::Regex;
use serde::Deserialize;

#[cfg(feature = "controller-service")]
use super::ParsedProxyActionHeader;
use super::{PostActionContext, ProxyHttpActionConfig, ProxyHttpPostActionConfig};
use crate::core::{ProxyError, Result};

/// Proxy action header that carries a JSON ReplaceAll rule.
pub const REPLACE_ALL_ACTION_HEADER: &str = "Arachnea-Proxy-ReplaceAll";
#[cfg(feature = "controller-service")]
const REPLACE_ALL_ACTION_HEADER_LOWER: &str = "arachnea-proxy-replaceall";

/// Constructor namespace for ReplaceAll proxy actions.
pub struct ReplaceAll;

impl ReplaceAll {
    /// Creates a ReplaceAll action without an explicit execution order.
    pub fn new(
        pattern: impl Into<String>,
        replacement: impl Into<String>,
        content_types: Option<Vec<String>>,
    ) -> ProxyHttpActionConfig {
        Self::with_order(None, pattern, replacement, content_types)
    }

    /// Creates a ReplaceAll action with an explicit execution order.
    pub fn ordered(
        order: i32,
        pattern: impl Into<String>,
        replacement: impl Into<String>,
        content_types: Option<Vec<String>>,
    ) -> ProxyHttpActionConfig {
        Self::with_order(Some(order), pattern, replacement, content_types)
    }

    fn with_order(
        order: Option<i32>,
        pattern: impl Into<String>,
        replacement: impl Into<String>,
        content_types: Option<Vec<String>>,
    ) -> ProxyHttpActionConfig {
        let mut params = HashMap::new();
        params.insert("pattern".to_string(), pattern.into());
        params.insert("replacement".to_string(), replacement.into());
        if let Some(content_types) = content_types {
            params.insert(
                "content_types".to_string(),
                serde_json::to_string(&content_types).unwrap_or_else(|_| "[]".to_string()),
            );
        }
        ProxyHttpActionConfig::PostAction(ProxyHttpPostActionConfig {
            action: "ReplaceAll".to_string(),
            order,
            params,
        })
    }
}

/// JSON configuration for a ReplaceAll action header value.
#[derive(Clone, Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct ReplaceAllHeaderValue {
    /// Optional execution order.
    #[serde(default)]
    pub order: Option<i32>,
    /// Regular expression pattern to match.
    pub pattern: String,
    /// Replacement text.
    pub replacement: String,
    /// Optional response content types on which the replacement may run.
    #[serde(default)]
    pub content_types: Option<Vec<String>>,
}

impl ReplaceAllHeaderValue {
    /// Converts this header value into an action config.
    #[allow(dead_code)]
    pub fn into_config(self) -> ProxyHttpPostActionConfig {
        match ReplaceAll::with_order(
            self.order,
            self.pattern,
            self.replacement,
            self.content_types,
        ) {
            ProxyHttpActionConfig::PostAction(action) => action,
            ProxyHttpActionConfig::RemoveHeader(_) => {
                unreachable!("ReplaceAll builds post actions")
            }
        }
    }
}

/// Serializes a ReplaceAll action to its proxy action header.
#[cfg(feature = "controller-service")]
pub(crate) fn proxy_action_header(
    action: &ProxyHttpActionConfig,
) -> Option<(&'static str, String)> {
    let ProxyHttpActionConfig::PostAction(action) = action else {
        return None;
    };
    if action.action != "ReplaceAll" {
        return None;
    }

    Some((
        REPLACE_ALL_ACTION_HEADER,
        serde_json::json!({
            "order": action.order,
            "pattern": action.params.get("pattern"),
            "replacement": action.params.get("replacement"),
            "content_types": action
                .params
                .get("content_types")
                .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok()),
        })
        .to_string(),
    ))
}

/// Parses the JSON value of a ReplaceAll proxy action header.
#[cfg(feature = "controller-service")]
pub(crate) fn parse_proxy_action_header_value(
    name_lower: &str,
    name: &str,
    value: &str,
) -> std::result::Result<Option<ParsedProxyActionHeader>, (u16, String)> {
    if name_lower != REPLACE_ALL_ACTION_HEADER_LOWER {
        return Ok(None);
    }

    serde_json::from_str::<ReplaceAllHeaderValue>(value)
        .map(|header_value| {
            Some(ParsedProxyActionHeader::PostAction(
                header_value.into_config(),
            ))
        })
        .map_err(|error| (502, format!("invalid {name} header value: {error}")))
}

/// Action that replaces all regex matches in the response body.
///
/// Only applies to textual content types. Binary responses are left unchanged.
pub struct ReplaceAllAction {
    pattern: Regex,
    replacement: String,
    content_types: Option<Vec<String>>,
}

impl ReplaceAllAction {
    /// Creates a `ReplaceAll` action from its configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Action configuration containing `pattern` and `replacement` parameters.
    ///
    /// # Returns
    ///
    /// Configured action.
    ///
    /// # Errors
    ///
    /// Returns an error when `pattern` is missing or invalid, or `replacement` is missing.
    pub fn from_config(config: &ProxyHttpPostActionConfig) -> Result<Self> {
        let pattern_str = config
            .params
            .get("pattern")
            .filter(|p| !p.is_empty())
            .ok_or_else(|| {
                ProxyError::Protocol(
                    "ReplaceAll action requires a non-empty 'pattern' parameter".to_string(),
                )
            })?;
        let replacement = config.params.get("replacement").ok_or_else(|| {
            ProxyError::Protocol("ReplaceAll action requires a 'replacement' parameter".to_string())
        })?;

        let pattern = Regex::new(pattern_str).map_err(|e| {
            ProxyError::Protocol(format!(
                "ReplaceAll invalid regex pattern '{}': {}",
                pattern_str, e
            ))
        })?;
        let content_types = config
            .params
            .get("content_types")
            .map(|value| parse_content_types(value))
            .transpose()?;

        Ok(Self {
            pattern,
            replacement: replacement.clone(),
            content_types,
        })
    }

    /// Detects whether the content type is textual.
    fn is_textual_content_type(&self, content_type: &str) -> bool {
        if let Some(content_types) = &self.content_types {
            return content_type_matches(content_type, content_types);
        }

        let ct = content_type.to_ascii_lowercase();
        if ct.starts_with("text/") {
            return true;
        }
        // Known textual mime types
        matches!(
            ct.split(';').next().unwrap_or("").trim(),
            "application/json"
                | "application/javascript"
                | "application/xml"
                | "application/xhtml+xml"
                | "image/svg+xml"
                | "application/vnd.apple.mpegurl"
                | "application/dash+xml"
        )
    }
}

impl super::ProxyHttpPostAction for ReplaceAllAction {
    fn post_apply(
        &self,
        _status_code: u16,
        headers: &mut HashMap<String, String>,
        body: Vec<u8>,
        context: &PostActionContext,
    ) -> Result<Vec<u8>> {
        if body.is_empty() {
            return Ok(body);
        }

        // Determine content type for textuality check
        let content_type = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value.as_str())
            .unwrap_or("");

        if !self.is_textual_content_type(content_type) {
            return Ok(body);
        }

        let content_encoding = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-encoding"))
            .map(|(_, value)| value.trim().to_ascii_lowercase());
        if content_encoding
            .as_deref()
            .is_some_and(|value| !value.is_empty() && value != "identity")
        {
            tracing::debug!(
                "ReplaceAll: skipping body with unsupported content-encoding {:?}",
                content_encoding
            );
            return Ok(body);
        }

        // Determine encoding from content-type charset
        let encoding = detect_encoding(content_type);
        let (decoded, encoding_used, had_errors) = encoding.decode(&body);

        if had_errors && decoded.len() == 0 {
            // Decoding failed completely, leave body unchanged
            tracing::debug!(
                "ReplaceAll: unable to decode body with {}, leaving unchanged",
                encoding_used.name()
            );
            return Ok(body);
        }

        // Resolve variables in replacement string
        let replacement = resolve_variables(&self.replacement, context);

        // Apply regex replacement
        let replaced = self.pattern.replace_all(&decoded, replacement.as_str());

        // Re-encode using the same encoding
        let (result_bytes, _encoding_used, _had_errors) = encoding_used.encode(&replaced);
        let result = result_bytes.into_owned();

        tracing::debug!(
            "ReplaceAll: replaced {} occurrences ({} -> {} bytes)",
            self.pattern.as_str(),
            body.len(),
            result.len()
        );

        Ok(result)
    }
}

fn parse_content_types(value: &str) -> Result<Vec<String>> {
    let content_types: Vec<String> = serde_json::from_str(value).map_err(|error| {
        ProxyError::Protocol(format!(
            "ReplaceAll invalid content_types parameter: {}",
            error
        ))
    })?;
    Ok(content_types
        .into_iter()
        .map(|content_type| content_type.trim().to_ascii_lowercase())
        .filter(|content_type| !content_type.is_empty())
        .collect())
}

fn content_type_matches(content_type: &str, allowed_content_types: &[String]) -> bool {
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    allowed_content_types
        .iter()
        .any(|allowed| allowed.trim().eq_ignore_ascii_case(&media_type))
}

/// Resolves predefined variables in a replacement string using request context.
fn resolve_variables(template: &str, context: &PostActionContext) -> String {
    let base_url = target_origin(&context.target_url);
    let proxy_path = entry_point_public_path(&context.entry_point);

    let path = url::Url::parse(&context.target_url)
        .ok()
        .map(|u| u.path().to_string())
        .unwrap_or_default();

    let path_base = match path.rfind('/') {
        Some(idx) => path[..=idx].to_string(),
        None => String::new(),
    };

    let proxy_inherited = if context.opts_encoded.is_empty() {
        proxy_path.clone()
    } else {
        format!("{}/opts_{}", proxy_path.trim_end_matches('/'), context.opts_encoded)
    };

    template
        .replace("{proxy}", &proxy_path)
        .replace("{proxy_inherited}", &proxy_inherited)
        .replace("{base_url}", &base_url)
        .replace("{path}", &path)
        .replace("{path_base}", &path_base)
}

fn target_origin(target_url: &str) -> String {
    let normalized = target_url.trim().trim_end_matches('/');
    let Ok(url) = url::Url::parse(normalized) else {
        return normalized.to_string();
    };

    let Some(host) = url.host_str() else {
        return normalized.to_string();
    };

    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{}]", host)
    } else {
        host.to_string()
    };
    let mut origin = format!("{}://{}", url.scheme(), host);
    if let Some(port) = url.port() {
        origin.push(':');
        origin.push_str(&port.to_string());
    }
    origin
}

fn entry_point_public_path(entry_point: &str) -> String {
    let normalized = entry_point.trim().trim_end_matches('/');
    if normalized.is_empty() {
        return String::new();
    }

    let Ok(url) = url::Url::parse(normalized) else {
        return normalized.to_string();
    };

    let mut path = match url.scheme() {
        "http" | "https" => url.path().to_string(),
        _ => {
            let mut value = String::new();
            if let Some(host) = url.host_str() {
                value.push('/');
                value.push_str(host.trim_matches('/'));
            }
            value.push_str(url.path());
            value
        }
    };

    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    path.trim_end_matches('/').to_string()
}

/// Detects the character encoding from a `content-type` header value.
///
/// Priority:
/// 1. `charset` parameter from the header.
/// 2. UTF-8 (fallback for textual types).
/// 3. Fallback to Windows-1252 then ISO-8859-1 if UTF-8 decoding fails.
fn detect_encoding(content_type: &str) -> &'static Encoding {
    // Try to extract charset from content-type
    for part in content_type.split(';') {
        let part = part.trim();
        if let Some(charset_value) = part
            .strip_prefix("charset=")
            .or_else(|| part.strip_prefix("charset ="))
        {
            let charset = charset_value.trim().trim_matches('"').trim_matches('\'');
            if let Some(encoding) = Encoding::for_label(charset.as_bytes()) {
                return encoding;
            }
        }
    }

    // Fallback: try UTF-8 first (most common for textual web content)
    encoding_rs::UTF_8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::http::actions::PostActionContext;
    use crate::core::http::actions::ProxyHttpPostAction;

    fn replace_all_action(params: HashMap<String, String>) -> ReplaceAllAction {
        ReplaceAllAction::from_config(&ProxyHttpPostActionConfig {
            action: "ReplaceAll".to_string(),
            order: None,
            params,
        })
        .unwrap()
    }

    #[test]
    fn test_is_textual_content_type() {
        let action = replace_all_action(HashMap::from([
            ("pattern".to_string(), "hello".to_string()),
            ("replacement".to_string(), "hi".to_string()),
        ]));

        assert!(action.is_textual_content_type("text/html"));
        assert!(action.is_textual_content_type("text/plain; charset=utf-8"));
        assert!(action.is_textual_content_type("application/json"));
        assert!(action.is_textual_content_type("application/javascript"));
        assert!(action.is_textual_content_type("image/svg+xml"));
        assert!(action.is_textual_content_type("application/vnd.apple.mpegurl"));
        assert!(action.is_textual_content_type("application/dash+xml"));
        assert!(!action.is_textual_content_type("application/x-mpegURL"));
        assert!(!action.is_textual_content_type("audio/mpegurl; charset=utf-8"));
        assert!(!action.is_textual_content_type("audio/x-mpegurl"));
        assert!(!action.is_textual_content_type("image/png"));
        assert!(!action.is_textual_content_type("video/mp4"));
        assert!(!action.is_textual_content_type("application/octet-stream"));
    }

    #[test]
    fn test_custom_content_types_override_default_filter() {
        let action = replace_all_action(HashMap::from([
            ("pattern".to_string(), "hello".to_string()),
            ("replacement".to_string(), "hi".to_string()),
            (
                "content_types".to_string(),
                serde_json::json!(["application/x-mpegurl", "audio/mpegurl"]).to_string(),
            ),
        ]));

        assert!(action.is_textual_content_type("application/x-mpegURL"));
        assert!(action.is_textual_content_type("audio/mpegurl; charset=utf-8"));
        assert!(!action.is_textual_content_type("text/plain"));
        assert!(!action.is_textual_content_type("application/vnd.apple.mpegurl"));
    }

    #[test]
    fn test_proxy_variable_uses_public_path() {
        let context = PostActionContext {
            entry_point: "http://127.0.0.1:8080/api/proxy".to_string(),
            target_url: "https://example.test/manifest.mpd".to_string(),
            opts_encoded: String::new(),
        };
        assert_eq!(
            resolve_variables("{proxy}/https://cdn.test/a", &context),
            "/api/proxy/https://cdn.test/a"
        );

        let context = PostActionContext {
            entry_point: "arachnea://api/proxy".to_string(),
            target_url: "https://example.test/manifest.mpd".to_string(),
            opts_encoded: String::new(),
        };
        assert_eq!(
            resolve_variables("{proxy}/https://cdn.test/a", &context),
            "/api/proxy/https://cdn.test/a"
        );
    }

    #[test]
    fn test_base_url_variable_uses_target_origin() {
        let context = PostActionContext {
            entry_point: "http://127.0.0.1:8080/api/proxy".to_string(),
            target_url: "https://myhost.be:8080/path/to/manifest.mpd?token=abc".to_string(),
            opts_encoded: String::new(),
        };

        assert_eq!(
            resolve_variables("{proxy}/{base_url}/m6web/", &context),
            "/api/proxy/https://myhost.be:8080/m6web/"
        );
    }

    #[test]
    fn test_proxy_inherited_with_opts() {
        let context = PostActionContext {
            entry_point: "http://127.0.0.1:8080/api/proxy".to_string(),
            target_url: "https://example.test/manifest.mpd".to_string(),
            opts_encoded: "ABCD".to_string(),
        };
        assert_eq!(
            resolve_variables("{proxy_inherited}", &context),
            "/api/proxy/opts_ABCD"
        );
    }

    #[test]
    fn test_proxy_inherited_without_opts() {
        let context = PostActionContext {
            entry_point: "http://127.0.0.1:8080/api/proxy".to_string(),
            target_url: "https://example.test/manifest.mpd".to_string(),
            opts_encoded: String::new(),
        };
        assert_eq!(
            resolve_variables("{proxy_inherited}", &context),
            "/api/proxy"
        );
    }

    #[test]
    fn test_replace_all_text() {
        let config = ProxyHttpPostActionConfig {
            action: "ReplaceAll".to_string(),
            order: None,
            params: HashMap::from([
                ("pattern".to_string(), "hello".to_string()),
                ("replacement".to_string(), "hi".to_string()),
            ]),
        };
        let action = ReplaceAllAction::from_config(&config).unwrap();
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "text/plain".to_string());
        let body = b"hello world, hello!".to_vec();
        let result = action
            .post_apply(200, &mut headers, body, &PostActionContext::default())
            .unwrap();
        assert_eq!(result, b"hi world, hi!".to_vec());
    }

    #[test]
    fn test_replace_all_no_match() {
        let config = ProxyHttpPostActionConfig {
            action: "ReplaceAll".to_string(),
            order: None,
            params: HashMap::from([
                ("pattern".to_string(), "xyz".to_string()),
                ("replacement".to_string(), "abc".to_string()),
            ]),
        };
        let action = ReplaceAllAction::from_config(&config).unwrap();
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "text/plain".to_string());
        let body = b"hello world".to_vec();
        let result = action
            .post_apply(
                200,
                &mut headers,
                body.clone(),
                &PostActionContext::default(),
            )
            .unwrap();
        assert_eq!(result, body);
    }

    #[test]
    fn test_replace_all_binary_unaffected() {
        let config = ProxyHttpPostActionConfig {
            action: "ReplaceAll".to_string(),
            order: None,
            params: HashMap::from([
                ("pattern".to_string(), "hello".to_string()),
                ("replacement".to_string(), "hi".to_string()),
            ]),
        };
        let action = ReplaceAllAction::from_config(&config).unwrap();
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "image/png".to_string());
        let body = b"hello world".to_vec();
        let result = action
            .post_apply(
                200,
                &mut headers,
                body.clone(),
                &PostActionContext::default(),
            )
            .unwrap();
        assert_eq!(result, body);
    }

    #[test]
    fn test_from_config_missing_pattern() {
        let config = ProxyHttpPostActionConfig {
            action: "ReplaceAll".to_string(),
            order: None,
            params: HashMap::from([("replacement".to_string(), "hi".to_string())]),
        };
        assert!(ReplaceAllAction::from_config(&config).is_err());
    }

    #[test]
    fn test_from_config_invalid_regex() {
        let config = ProxyHttpPostActionConfig {
            action: "ReplaceAll".to_string(),
            order: None,
            params: HashMap::from([
                ("pattern".to_string(), "[invalid".to_string()),
                ("replacement".to_string(), "hi".to_string()),
            ]),
        };
        assert!(ReplaceAllAction::from_config(&config).is_err());
    }
}
