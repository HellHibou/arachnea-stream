//! Builder for declarative proxy configuration.

use crate::core::{
    ChainStrategy, EgressPool, ProxyChain, ProxyNode, Result, RoutePolicy, SecurityPolicy,
};

use super::proxy_config::ProxyConfig;
use super::proxy_profile::ProxyProfile;
use super::resolved::ResolvedProxyConfig;
use super::timeout::TimeoutConfig;
use super::usage_profile::UsageProfile;

/// Builder for `ProxyConfig`.
#[derive(Clone, Debug)]
pub struct ProxyConfigBuilder {
    config: ProxyConfig,
}

impl Default for ProxyConfigBuilder {
    /// Creates a builder initialized with default proxy configuration.
    fn default() -> Self {
        Self {
            config: ProxyConfig::default(),
        }
    }
}

impl ProxyConfigBuilder {
    /// Sets the profile.
    ///
    /// # Parameters
    ///
    /// - `profile`: Proxy profile to apply.
    ///
    /// # Returns
    ///
    /// Updated builder.
    pub fn profile(mut self, profile: ProxyProfile) -> Self {
        self.config.profile = profile;
        self
    }

    /// Replaces the current configuration with one generated from a usage profile.
    ///
    /// # Parameters
    ///
    /// - `profile`: Usage profile used to generate the new configuration.
    ///
    /// # Returns
    ///
    /// Updated builder.
    pub fn usage_profile(mut self, profile: UsageProfile) -> Self {
        self.config = profile.config();
        self
    }

    /// Replaces all chains.
    ///
    /// # Parameters
    ///
    /// - `chains`: Chains to place in the configuration.
    ///
    /// # Returns
    ///
    /// Updated builder.
    pub fn chains(mut self, chains: Vec<ProxyChain>) -> Self {
        self.config.chains = chains;
        self
    }

    /// Adds a single upstream node and creates a chain for it.
    ///
    /// # Parameters
    ///
    /// - `node`: Upstream node to add and make the default chain.
    ///
    /// # Returns
    ///
    /// Updated builder.
    pub fn upstream(mut self, node: ProxyNode) -> Self {
        self.config.upstreams.push(node.clone());
        self.config.chains = vec![ProxyChain {
            name: node.name.clone(),
            nodes: vec![node],
            strategy: ChainStrategy::FixedOrder,
        }];
        self.config.routing.default_chain =
            self.config.chains.first().map(|chain| chain.name.clone());
        self
    }

    /// Replaces all egress pools.
    ///
    /// # Parameters
    ///
    /// - `egress_pools`: Egress pools to place in the configuration.
    ///
    /// # Returns
    ///
    /// Updated builder.
    pub fn egress_pools(mut self, egress_pools: Vec<EgressPool>) -> Self {
        self.config.egress_pools = egress_pools;
        self
    }

    /// Replaces the route policy.
    ///
    /// # Parameters
    ///
    /// - `routing`: Route policy to install.
    ///
    /// # Returns
    ///
    /// Updated builder.
    pub fn routing(mut self, routing: RoutePolicy) -> Self {
        self.config.routing = routing;
        self
    }

    /// Replaces the security policy.
    ///
    /// # Parameters
    ///
    /// - `security`: Security policy to install.
    ///
    /// # Returns
    ///
    /// Updated builder.
    pub fn security(mut self, security: SecurityPolicy) -> Self {
        self.config.security = security;
        self
    }

    /// Builds the resolved configuration.
    ///
    /// # Returns
    ///
    /// Validated proxy configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the configuration is unsafe or internally
    /// inconsistent.
    pub fn build(self) -> Result<ResolvedProxyConfig> {
        self.config.resolve()
    }

    /// Replaces timeout values.
    ///
    /// # Parameters
    ///
    /// - `timeouts`: Timeout values to install.
    ///
    /// # Returns
    ///
    /// Updated builder.
    pub fn timeouts(mut self, timeouts: TimeoutConfig) -> Self {
        self.config.timeouts = timeouts;
        self
    }
}
