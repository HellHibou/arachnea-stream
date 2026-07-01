//! Post-response action that replaces all occurrences of a regex pattern in
//! textual responses.

use std::collections::HashMap;

use encoding_rs::Encoding;
use regex::Regex;
use serde::Deserialize;

#[cfg(feature = "controller-service")]
use super::ParsedProxyActionHeader;
use super::{ProxyHttpActionConfig, ProxyHttpPostActionConfig};
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
    ) -> ProxyHttpActionConfig {
        Self::with_order(None, pattern, replacement)
    }

    /// Creates a ReplaceAll action with an explicit execution order.
    pub fn ordered(
        order: i32,
        pattern: impl Into<String>,
        replacement: impl Into<String>,
    ) -> ProxyHttpActionConfig {
        Self::with_order(Some(order), pattern, replacement)
    }

    fn with_order(
        order: Option<i32>,
        pattern: impl Into<String>,
        replacement: impl Into<String>,
    ) -> ProxyHttpActionConfig {
        let mut params = HashMap::new();
        params.insert("pattern".to_string(), pattern.into());
        params.insert("replacement".to_string(), replacement.into());
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
}

impl ReplaceAllHeaderValue {
    /// Converts this header value into an action config.
    #[allow(dead_code)]
    pub fn into_config(self) -> ProxyHttpPostActionConfig {
        match ReplaceAll::with_order(self.order, self.pattern, self.replacement) {
            ProxyHttpActionConfig::PostAction(action) => action,
            ProxyHttpActionConfig::RemoveHeader(_) => {
                unreachable!("ReplaceAll builds post actions")
            }
        }
    }
}

/// Returns true when the post action needs identity response encoding.
#[cfg(feature = "controller-service")]
pub(crate) fn requires_identity_response_encoding(action: &ProxyHttpPostActionConfig) -> bool {
    action.action == "ReplaceAll"
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

        Ok(Self {
            pattern,
            replacement: replacement.clone(),
        })
    }

    /// Detects whether the content type is textual.
    fn is_textual_content_type(content_type: &str) -> bool {
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

        if !Self::is_textual_content_type(content_type) {
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

        // Apply regex replacement
        let replaced = self
            .pattern
            .replace_all(&decoded, self.replacement.as_str());

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
    use crate::core::http::actions::ProxyHttpPostAction;

    #[test]
    fn test_is_textual_content_type() {
        assert!(ReplaceAllAction::is_textual_content_type("text/html"));
        assert!(ReplaceAllAction::is_textual_content_type(
            "text/plain; charset=utf-8"
        ));
        assert!(ReplaceAllAction::is_textual_content_type(
            "application/json"
        ));
        assert!(ReplaceAllAction::is_textual_content_type(
            "application/javascript"
        ));
        assert!(ReplaceAllAction::is_textual_content_type("image/svg+xml"));
        assert!(ReplaceAllAction::is_textual_content_type(
            "application/vnd.apple.mpegurl"
        ));
        assert!(ReplaceAllAction::is_textual_content_type(
            "application/dash+xml"
        ));
        assert!(!ReplaceAllAction::is_textual_content_type("image/png"));
        assert!(!ReplaceAllAction::is_textual_content_type("video/mp4"));
        assert!(!ReplaceAllAction::is_textual_content_type(
            "application/octet-stream"
        ));
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
        let result = action.post_apply(200, &mut headers, body).unwrap();
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
        let result = action.post_apply(200, &mut headers, body.clone()).unwrap();
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
        let result = action.post_apply(200, &mut headers, body.clone()).unwrap();
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
