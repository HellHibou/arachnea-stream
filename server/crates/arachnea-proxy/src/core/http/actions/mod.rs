//! Post-response HTTP actions for the proxy core.
//!
//! This module defines the trait and types for actions that can transform
//! HTTP responses after they are received from the upstream server.

pub mod replace_all;

use std::collections::HashMap;

use serde::Deserialize;

use crate::core::Result;

/// Common configuration for a post-response action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProxyHttpPostActionConfig {
    /// Action type name, e.g. `"ReplaceAll"`.
    pub action: String,
    /// Optional execution order, lower runs first.
    pub order: Option<i32>,
    /// Action-specific parameters.
    pub params: HashMap<String, String>,
}

/// Trait implemented by post-response HTTP actions.
pub trait ProxyHttpPostAction: Send + Sync {
    /// Applies the action to a response.
    ///
    /// # Arguments
    ///
    /// * `status_code` - HTTP status code of the response.
    /// * `headers` - Mutable response headers.
    /// * `body` - Response body bytes, consumed and returned.
    ///
    /// # Returns
    ///
    /// Transformed body bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when the action cannot be applied.
    fn post_apply(
        &self,
        status_code: u16,
        headers: &mut HashMap<String, String>,
        body: Vec<u8>,
    ) -> Result<Vec<u8>>;
}

/// Creates a concrete action from its configuration.
///
/// # Arguments
///
/// * `config` - Action configuration.
///
/// # Returns
///
/// Boxed action instance.
///
/// # Errors
///
/// Returns an error when the action type is unknown or configuration is invalid.
pub fn build_action(config: &ProxyHttpPostActionConfig) -> Result<Box<dyn ProxyHttpPostAction>> {
    match config.action.as_str() {
        "ReplaceAll" => Ok(Box::new(replace_all::ReplaceAllAction::from_config(config)?)),
        _ => Err(crate::core::ProxyError::Protocol(format!(
            "unknown post-response action '{}'",
            config.action
        ))),
    }
}

/// Applies a list of post-response actions in order.
///
/// Actions are sorted by `order` (ascending), then by position. Actions without
/// `order` are executed after ordered ones, in their discovered order.
///
/// # Arguments
///
/// * `status_code` - HTTP status code.
/// * `headers` - Mutable response headers.
/// * `body` - Response body bytes.
/// * `actions` - Actions to apply.
///
/// # Returns
///
/// Final body bytes after all actions.
///
/// # Errors
///
/// Returns an error when any action fails.
pub fn apply_post_actions(
    status_code: u16,
    headers: &mut HashMap<String, String>,
    body: Vec<u8>,
    actions: &[ProxyHttpPostActionConfig],
) -> Result<Vec<u8>> {
    if actions.is_empty() {
        return Ok(body);
    }

    tracing::debug!(
        "apply_post_actions: {} actions, body_len={}",
        actions.len(),
        body.len()
    );

    // Sort actions: ordered first by value, then unordered in original order.
    let mut indexed: Vec<(usize, &ProxyHttpPostActionConfig)> = actions.iter().enumerate().collect();
    indexed.sort_by(|(ia, a), (ib, b)| match (a.order, b.order) {
        (Some(oa), Some(ob)) => oa.cmp(&ob).then_with(|| ia.cmp(ib)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => ia.cmp(ib),
    });

    let mut current_body = body;
    for (_, config) in &indexed {
        let action = build_action(config)?;
        current_body = action.post_apply(status_code, headers, current_body)?;
    }

    Ok(current_body)
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
        let mut params = HashMap::new();
        params.insert("pattern".to_string(), self.pattern);
        params.insert("replacement".to_string(), self.replacement);
        ProxyHttpPostActionConfig {
            action: "ReplaceAll".to_string(),
            order: self.order,
            params,
        }
    }
}
