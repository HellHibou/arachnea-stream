//! Redirect option updates and proxied Location header rewriting.

use std::collections::HashMap;

use url::Url;

use crate::core::http::actions::{
    should_remove_opts_header_on_redirect, ProxyHttpRedirectActionConfig,
};

use super::options::{encode_proxy_opts, ProxyHttpOpts};
use super::urls::{encode_proxy_path, entry_point_location_prefix};

/// Re-encodes options after applying status-dependent header removal rules.
///
/// # Arguments
/// * `status` - HTTP response status used to evaluate the redirect rule.
/// * `current_opts_encoded` - Current encoded options, retained when no redirect rule changes them.
/// * `current_opts` - Decoded current options, or `None` when no options were supplied.
/// * `redirect_actions` - Redirect-specific rules that can remove option headers for this status.
pub(super) fn redirect_opts_encoded(
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
///
/// # Arguments
/// * `location` - Absolute or relative Location value returned by the upstream server.
/// * `current_target_url` - Upstream URL used as the base for relative redirect locations.
///
fn resolve_location(location: &str, current_target_url: &str) -> Result<String, String> {
    let base = Url::parse(current_target_url)
        .map_err(|e| format!("Failed to parse current target URL: {}", e))?;
    let resolved = base
        .join(location)
        .map_err(|e| format!("Failed to resolve Location '{}': {}", location, e))?;
    Ok(resolved.to_string())
}

/// Rewrites a `Location` header value to a proxied URL.
///
/// # Arguments
/// * `headers` - HTTP headers inspected or updated by this operation.
/// * `current_target_url` - Upstream URL used as the base for relative redirect locations.
/// * `current_opts_encoded` - Current encoded options, retained when no redirect rule changes them.
/// * `api_prefix` - Controller API prefix used to construct the local proxy route.
///
pub(super) fn rewrite_location_header(
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
