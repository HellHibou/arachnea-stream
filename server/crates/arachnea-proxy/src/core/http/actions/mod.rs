//! Post-response HTTP actions for the proxy core.
//!
//! This module defines the trait and types for actions that can transform
//! HTTP responses after they are received from the upstream server.

pub mod remove_header;
pub mod replace_all;

use std::collections::HashMap;

use crate::core::Result;

pub use remove_header::{ProxyHttpRemoveHeaderConfig, RemoveHeader, REMOVE_HEADER_ACTION_HEADER};
pub use replace_all::ReplaceAll;

#[cfg(feature = "controller-service")]
pub(crate) struct ParsedProxyActionHeaders {
    pub post_actions: Vec<ProxyHttpPostActionConfig>,
    pub redirect_actions: Vec<ProxyHttpRedirectActionConfig>,
}

#[cfg(feature = "controller-service")]
pub(crate) enum ProxyHttpRedirectActionConfig {
    RemoveHeader(ProxyHttpRemoveHeaderConfig),
}

#[cfg(feature = "controller-service")]
impl ParsedProxyActionHeaders {
    fn new() -> Self {
        Self {
            post_actions: Vec::new(),
            redirect_actions: Vec::new(),
        }
    }

    fn push(&mut self, action: ParsedProxyActionHeader) {
        match action {
            ParsedProxyActionHeader::PostAction(action) => self.post_actions.push(action),
            ParsedProxyActionHeader::RedirectAction(action) => self.redirect_actions.push(action),
        }
    }
}

#[cfg(feature = "controller-service")]
pub(crate) enum ParsedProxyActionHeader {
    PostAction(ProxyHttpPostActionConfig),
    RedirectAction(ProxyHttpRedirectActionConfig),
}

/// Serializable proxy action carried through controller proxy URL options.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProxyHttpActionConfig {
    /// Post-response action applied to the upstream response body/headers.
    PostAction(ProxyHttpPostActionConfig),
    /// Redirect-time action that removes headers from encoded proxy options.
    RemoveHeader(ProxyHttpRemoveHeaderConfig),
}

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

/// Returns true when post actions require buffering the entire response body.
///
/// Post-actions are conservatively treated as body transformations until a
/// future action explicitly supports streaming. This also implies identity
/// response encoding so an action never receives a compressed body.
#[cfg(feature = "controller-service")]
pub(crate) fn post_actions_require_body_buffering(actions: &[ProxyHttpPostActionConfig]) -> bool {
    !actions.is_empty()
}

/// Returns true when redirect actions should remove an encoded opts header.
#[cfg(feature = "controller-service")]
pub(crate) fn should_remove_opts_header_on_redirect(
    actions: &[ProxyHttpRedirectActionConfig],
    status: u16,
    header: &str,
) -> bool {
    actions.iter().any(|action| match action {
        ProxyHttpRedirectActionConfig::RemoveHeader(action) => {
            remove_header::should_remove_opts_header_on_redirect(action, status, header)
        }
    })
}

/// Serializes a proxy action to the action header carried in `opts.headers`.
#[cfg(feature = "controller-service")]
pub(crate) fn proxy_action_header(
    action: &ProxyHttpActionConfig,
) -> Option<(&'static str, String)> {
    replace_all::proxy_action_header(action).or_else(|| remove_header::proxy_action_header(action))
}

/// Parses and removes proxy action headers from a mutable request header map.
#[cfg(feature = "controller-service")]
pub(crate) fn parse_proxy_action_headers(
    headers: &mut HashMap<String, String>,
) -> std::result::Result<ParsedProxyActionHeaders, (u16, String)> {
    let mut actions = ParsedProxyActionHeaders::new();

    let header_names: Vec<String> = headers.keys().cloned().collect();
    for name in &header_names {
        let Some(value) = headers.get(name).cloned() else {
            continue;
        };

        if parse_proxy_action_header_into(&mut actions, name, &value)? {
            headers.remove(name);
        }
    }

    Ok(actions)
}

/// Parses one proxy action header and appends it to the parsed action buckets.
#[cfg(feature = "controller-service")]
pub(crate) fn parse_proxy_action_header_into(
    actions: &mut ParsedProxyActionHeaders,
    name: &str,
    value: &str,
) -> std::result::Result<bool, (u16, String)> {
    let Some(action) = parse_proxy_action_header_value(name, value)? else {
        return Ok(false);
    };

    actions.push(action);
    Ok(true)
}

/// Parses one proxy action header value, if the header name is known.
#[cfg(feature = "controller-service")]
pub(crate) fn parse_proxy_action_header_value(
    name: &str,
    value: &str,
) -> std::result::Result<Option<ParsedProxyActionHeader>, (u16, String)> {
    if !name.eq_ignore_ascii_case(replace_all::REPLACE_ALL_ACTION_HEADER)
        && !name.eq_ignore_ascii_case(remove_header::REMOVE_HEADER_ACTION_HEADER)
    {
        return Ok(None);
    }

    let name_lower = name.to_ascii_lowercase();
    if let Some(action) = replace_all::parse_proxy_action_header_value(&name_lower, name, value)? {
        return Ok(Some(action));
    }
    if let Some(action) = remove_header::parse_proxy_action_header_value(&name_lower, name, value)?
    {
        return Ok(Some(action));
    }

    Ok(None)
}

/// Context passed to post-response actions for variable substitution.
#[derive(Clone, Debug, Default)]
pub struct PostActionContext {
    /// Entry point / proxy base path, e.g. "http://127.0.0.1:8080/api/proxy".
    pub entry_point: String,
    /// Full target URL string, e.g. "https://host.com:8080/mon/chemin.jpg".
    pub target_url: String,
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
    /// * `context` - Request context for variable substitution.
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
        context: &PostActionContext,
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
        "ReplaceAll" => Ok(Box::new(replace_all::ReplaceAllAction::from_config(
            config,
        )?)),
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
/// * `context` - Request context for variable substitution.
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
    context: &PostActionContext,
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
    let mut indexed: Vec<(usize, &ProxyHttpPostActionConfig)> =
        actions.iter().enumerate().collect();
    indexed.sort_by(|(ia, a), (ib, b)| match (a.order, b.order) {
        (Some(oa), Some(ob)) => oa.cmp(&ob).then_with(|| ia.cmp(ib)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => ia.cmp(ib),
    });

    let mut current_body = body;
    for (_, config) in &indexed {
        let action = build_action(config)?;
        current_body = action.post_apply(status_code, headers, current_body, context)?;
    }

    Ok(current_body)
}
