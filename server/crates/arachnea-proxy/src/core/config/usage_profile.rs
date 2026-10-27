//! High-level usage profiles and explicit chain generation.

use crate::core::{ProxyChain, ProxyNode, Result, RoutePolicy};
use serde::{Deserialize, Serialize};

use super::proxy_config::ProxyConfig;
use super::proxy_profile::ProxyProfile;
use super::system_relay::{system_relay_config_from_env, system_relay_direct_config};
use super::timeout::TimeoutConfig;

/// High-level usage profiles that generate explicit proxy configuration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageProfile {
    /// Use the current system networking setup.
    ///
    /// This profile reads standard proxy environment variables when available
    /// and falls back to a direct route otherwise.
    SystemRelay,

    /// Send all traffic through one configured proxy.
    ///
    /// # Parameters
    ///
    /// - `ProxyNode`: Proxy node that receives all outbound connections.
    SingleForwarder(ProxyNode),

    /// Send all traffic through a fixed multi-hop proxy chain.
    ///
    /// # Parameters
    ///
    /// - `Vec<ProxyNode>`: Ordered proxy nodes used by the chain.
    MultiForwarder(Vec<ProxyNode>),

    /// Prefer proxy-only behavior with minimized logs and no direct fallback.
    ///
    /// # Parameters
    ///
    /// - `Vec<ProxyNode>`: Ordered proxy nodes used by the privacy chain.
    Privacy(Vec<ProxyNode>),

    /// Require explicit customization on top of the generated defaults.
    Advanced,
}

impl UsageProfile {
    /// Builds a complete starting configuration from this usage profile.
    ///
    /// `SystemRelay` ignores invalid system proxy environment values and falls
    /// back to a direct configuration. Use `try_config` when those errors should
    /// be reported to the caller.
    ///
    /// # Returns
    ///
    /// Proxy configuration initialized for the profile defaults.
    pub fn config(self) -> ProxyConfig {
        match self.try_config() {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(%error, "ignoring invalid system proxy settings");
                system_relay_direct_config()
            }
        }
    }

    /// Builds a complete starting configuration and applies explicit timeouts.
    ///
    /// `SystemRelay` ignores invalid system proxy environment values and falls
    /// back to a direct configuration. Use `try_config_with_timeouts` when
    /// those errors should be reported to the caller.
    ///
    /// # Parameters
    ///
    /// - `timeouts`: Timeout values installed on the generated configuration.
    ///
    /// # Returns
    ///
    /// Proxy configuration initialized for the profile defaults and timeouts.
    pub fn config_with_timeouts(self, timeouts: TimeoutConfig) -> ProxyConfig {
        self.config().with_timeouts(timeouts)
    }

    /// Builds a complete starting configuration and reports system proxy errors.
    ///
    /// # Returns
    ///
    /// Proxy configuration initialized for the profile defaults.
    ///
    /// # Errors
    ///
    /// Returns an error when `SystemRelay` finds an unsupported or invalid
    /// proxy environment variable.
    pub fn try_config(self) -> Result<ProxyConfig> {
        match self {
            Self::SystemRelay => system_relay_config_from_env(),
            Self::SingleForwarder(node) => Ok(single_forwarder_config(
                node,
                ProxyProfile::SingleProxy,
                false,
            )),
            Self::MultiForwarder(nodes) => Ok(multi_forwarder_config(
                "multi_forwarder",
                nodes,
                ProxyProfile::MultiForwarder,
                false,
            )),
            Self::Privacy(nodes) => Ok(multi_forwarder_config(
                "privacy",
                nodes,
                ProxyProfile::Privacy,
                false,
            )),
            Self::Advanced => Ok(ProxyConfig {
                profile: ProxyProfile::Advanced,
                ..ProxyConfig::default()
            }),
        }
    }

    /// Builds a complete starting configuration with explicit timeouts and reports errors.
    ///
    /// # Parameters
    ///
    /// - `timeouts`: Timeout values installed on the generated configuration.
    ///
    /// # Returns
    ///
    /// Proxy configuration initialized for the profile defaults and timeouts.
    ///
    /// # Errors
    ///
    /// Returns an error when `SystemRelay` finds an unsupported or invalid
    /// proxy environment variable.
    pub fn try_config_with_timeouts(self, timeouts: TimeoutConfig) -> Result<ProxyConfig> {
        self.try_config()
            .map(|config| config.with_timeouts(timeouts))
    }
}

/// Builds a single-forwarder configuration.
///
/// # Parameters
///
/// - `node`: Proxy node receiving all outbound traffic.
/// - `profile`: Low-level profile marker stored in the configuration.
/// - `allow_direct_fallback`: Whether direct fallback is allowed after failures.
///
/// # Returns
///
/// Proxy configuration using `node` as the default chain.
pub(super) fn single_forwarder_config(
    node: ProxyNode,
    profile: ProxyProfile,
    allow_direct_fallback: bool,
) -> ProxyConfig {
    let chain_name = node.name.clone();
    ProxyConfig {
        profile,
        upstreams: vec![node.clone()],
        chains: vec![ProxyChain::single(chain_name.clone(), node)],
        egress_pools: Vec::new(),
        routing: RoutePolicy {
            default_chain: Some(chain_name),
            allow_direct_fallback,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    }
}

/// Builds a fixed multi-forwarder configuration.
///
/// # Parameters
///
/// - `chain_name`: Name assigned to the generated chain.
/// - `nodes`: Ordered proxy nodes used by the chain.
/// - `profile`: Low-level profile marker stored in the configuration.
/// - `allow_direct_fallback`: Whether direct fallback is allowed after failures.
///
/// # Returns
///
/// Proxy configuration using `nodes` as one fixed chain.
fn multi_forwarder_config(
    chain_name: &str,
    nodes: Vec<ProxyNode>,
    profile: ProxyProfile,
    allow_direct_fallback: bool,
) -> ProxyConfig {
    let chain_name = nodes
        .first()
        .filter(|_| nodes.len() == 1)
        .map_or_else(|| chain_name.to_string(), |node| node.name.clone());
    ProxyConfig {
        profile,
        upstreams: nodes.clone(),
        chains: vec![ProxyChain::fixed(chain_name.clone(), nodes)],
        egress_pools: Vec::new(),
        routing: RoutePolicy {
            default_chain: Some(chain_name),
            allow_direct_fallback,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    }
}
