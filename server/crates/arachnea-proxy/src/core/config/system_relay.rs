//! System proxy environment parsing and direct fallback.

use crate::core::{ApplicationProtocol, ProxyChain, ProxyNode, Result, RouteRule};
use std::env;

use super::proxy_config::ProxyConfig;
use super::proxy_profile::ProxyProfile;
use super::usage_profile::single_forwarder_config;

/// Builds the direct system-relay fallback configuration.
///
/// # Returns
///
/// Direct proxy configuration using the current system network route.
pub(super) fn system_relay_direct_config() -> ProxyConfig {
    ProxyConfig::default()
}

/// Builds a system-relay configuration from standard proxy environment variables.
///
/// # Returns
///
/// Proxy configuration derived from `ALL_PROXY`, `HTTP_PROXY`, or `HTTPS_PROXY`,
/// falling back to a direct route when none is configured.
///
/// # Errors
///
/// Returns an error when a configured proxy URL cannot be converted to a proxy node.
pub(super) fn system_relay_config_from_env() -> Result<ProxyConfig> {
    if let Some(proxy_url) = first_proxy_env(&["ALL_PROXY", "all_proxy"]) {
        return Ok(single_forwarder_config(
            ProxyNode::from_url("system-all-proxy", proxy_url)?,
            ProxyProfile::Direct,
            false,
        ));
    }

    let mut config = system_relay_direct_config();
    if let Some(proxy_url) = first_proxy_env(&["HTTP_PROXY", "http_proxy"]) {
        append_system_protocol_proxy(
            &mut config,
            "system-http-proxy",
            proxy_url,
            ApplicationProtocol::Http,
        )?;
    }
    if let Some(proxy_url) = first_proxy_env(&["HTTPS_PROXY", "https_proxy"]) {
        append_system_protocol_proxy(
            &mut config,
            "system-https-proxy",
            proxy_url,
            ApplicationProtocol::Https,
        )?;
    }
    Ok(config)
}

/// Finds the first non-empty proxy environment variable.
///
/// # Arguments
///
/// - `names`: Environment variable names checked in order.
///
/// # Returns
///
/// First non-empty value found in the process environment.
fn first_proxy_env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

/// Adds one system proxy route for a specific protocol.
///
/// # Arguments
///
/// - `config`: Configuration receiving the proxy route.
/// - `name`: Stable name used for the node and chain.
/// - `proxy_url`: Proxy URL read from the environment.
/// - `protocol`: Application protocol matched by the route.
///
/// # Errors
///
/// Returns an error when `proxy_url` cannot be converted to a proxy node.
fn append_system_protocol_proxy(
    config: &mut ProxyConfig,
    name: &str,
    proxy_url: String,
    protocol: ApplicationProtocol,
) -> Result<()> {
    let node = ProxyNode::from_url(name, proxy_url)?;
    let chain_name = node.name.clone();
    config.upstreams.push(node.clone());
    config
        .chains
        .push(ProxyChain::single(chain_name.clone(), node));
    config
        .routing
        .rules
        .push(RouteRule::for_protocol(protocol, chain_name));
    Ok(())
}
