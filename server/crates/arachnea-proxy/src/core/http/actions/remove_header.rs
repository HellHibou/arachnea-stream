//! Redirect-time action that removes headers from encoded proxy options.

use serde::Deserialize;

use super::ProxyHttpActionConfig;
#[cfg(feature = "controller-service")]
use super::{ParsedProxyActionHeader, ProxyHttpRedirectActionConfig};

/// Proxy action header that carries a JSON RemoveHeader rule.
pub const REMOVE_HEADER_ACTION_HEADER: &str = "Arachnea-Proxy-RemoveHeader";
#[cfg(feature = "controller-service")]
const REMOVE_HEADER_ACTION_HEADER_LOWER: &str = "arachnea-proxy-removeheader";
const REMOVE_HEADER_TRIGGER_ON_HTTP_302: &str = "onHttp302";

/// Redirect-time rule that removes a header from encoded proxy options.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProxyHttpRemoveHeaderConfig {
    /// Trigger name, e.g. `"onHttp302"`.
    pub trigger: String,
    /// Header names to remove from `opts.headers` when the trigger matches.
    pub headers: Vec<String>,
}

impl ProxyHttpRemoveHeaderConfig {
    /// Returns true when this rule should run for the given HTTP status.
    #[cfg(feature = "controller-service")]
    pub(crate) fn applies_to_status(&self, status: u16) -> bool {
        self.trigger.trim() == REMOVE_HEADER_TRIGGER_ON_HTTP_302 && status == 302
    }

    /// Returns true when this rule targets the provided header name.
    #[cfg(feature = "controller-service")]
    pub(crate) fn targets_header(&self, header: &str) -> bool {
        self.headers
            .iter()
            .any(|candidate| candidate.trim().eq_ignore_ascii_case(header))
    }
}

/// Returns true when this action should remove the provided opts header.
#[cfg(feature = "controller-service")]
pub(crate) fn should_remove_opts_header_on_redirect(
    action: &ProxyHttpRemoveHeaderConfig,
    status: u16,
    header: &str,
) -> bool {
    action.applies_to_status(status) && action.targets_header(header)
}

/// Constructor namespace for RemoveHeader proxy actions.
pub struct RemoveHeader;

impl RemoveHeader {
    /// Creates a RemoveHeader action that runs when a redirect response is HTTP 302.
    pub fn on_http302<I, S>(headers: I) -> ProxyHttpActionConfig
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        ProxyHttpActionConfig::RemoveHeader(ProxyHttpRemoveHeaderConfig {
            trigger: REMOVE_HEADER_TRIGGER_ON_HTTP_302.to_string(),
            headers: headers.into_iter().map(Into::into).collect(),
        })
    }
}

/// JSON configuration for a RemoveHeader action header value.
#[derive(Clone, Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct RemoveHeaderValue {
    /// Trigger name, currently `onHttp302`.
    #[serde(rename = "type")]
    pub trigger: String,
    /// Header names to remove from encoded proxy options.
    pub headers: Vec<String>,
}

impl RemoveHeaderValue {
    /// Converts this header value into a redirect-time remove-header rule.
    #[allow(dead_code)]
    pub fn into_config(self) -> ProxyHttpRemoveHeaderConfig {
        ProxyHttpRemoveHeaderConfig {
            trigger: self.trigger,
            headers: self.headers,
        }
    }
}

/// Serializes a RemoveHeader action to its proxy action header.
#[cfg(feature = "controller-service")]
pub(crate) fn proxy_action_header(
    action: &ProxyHttpActionConfig,
) -> Option<(&'static str, String)> {
    let ProxyHttpActionConfig::RemoveHeader(action) = action else {
        return None;
    };

    Some((
        REMOVE_HEADER_ACTION_HEADER,
        serde_json::json!({
            "type": action.trigger,
            "headers": action.headers,
        })
        .to_string(),
    ))
}

/// Parses the JSON value of a RemoveHeader proxy action header.
#[cfg(feature = "controller-service")]
pub(crate) fn parse_proxy_action_header_value(
    name_lower: &str,
    name: &str,
    value: &str,
) -> std::result::Result<Option<ParsedProxyActionHeader>, (u16, String)> {
    if name_lower != REMOVE_HEADER_ACTION_HEADER_LOWER {
        return Ok(None);
    }

    serde_json::from_str::<RemoveHeaderValue>(value)
        .map(|header_value| {
            Some(ParsedProxyActionHeader::RedirectAction(
                ProxyHttpRedirectActionConfig::RemoveHeader(header_value.into_config()),
            ))
        })
        .map_err(|error| (502, format!("invalid {name} header value: {error}")))
}
