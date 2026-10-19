//! Declarative proxy configuration and resolution entry point.

use crate::core::parameters::validate_parameter_handler_configs;
use crate::core::{
    EgressPool, ParameterHandlerConfig, PrivacyPolicy, ProxyChain, ProxyError, ProxyNode, Result,
    RoutePolicy, SecurityPolicy, TransportKind,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::builder::ProxyConfigBuilder;
use super::name_resolution::NameResolutionMode;
use super::proxy_profile::ProxyProfile;
use super::resolved::ResolvedProxyConfig;
use super::timeout::TimeoutConfig;
use super::validation::{
    validate_chain_nodes, validate_egress_pools, validate_unique_upstream_names,
};

/// Declarative proxy configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxyConfig {
    /// Profile used to derive defaults.
    pub profile: ProxyProfile,
    /// Name resolution mode.
    pub name_resolution: NameResolutionMode,
    /// Declared upstream nodes.
    pub upstreams: Vec<ProxyNode>,
    /// Declared chains.
    pub chains: Vec<ProxyChain>,
    /// Declared proxy pools used by chain pool markers and Smart DNS hints.
    #[serde(default)]
    pub egress_pools: Vec<EgressPool>,
    /// Route selection configuration.
    pub routing: RoutePolicy,
    /// Use a direct route when no dynamic country proxy is available.
    /// Request parameters may override this default, subject to privacy policy.
    pub fallback_direct: bool,
    /// Privacy policy.
    pub privacy: PrivacyPolicy,
    /// Security policy.
    pub security: SecurityPolicy,
    /// Core timeout values.
    pub timeouts: TimeoutConfig,
    /// Configured request-parameter handlers.
    #[serde(default)]
    pub parameter_handlers: Vec<ParameterHandlerConfig>,
}

impl Default for ProxyConfig {
    /// Returns a loopback-safe direct proxy configuration.
    fn default() -> Self {
        Self {
            profile: ProxyProfile::Direct,
            name_resolution: NameResolutionMode::PreserveHostname,
            upstreams: vec![ProxyNode::direct("direct")],
            chains: vec![ProxyChain::direct()],
            egress_pools: Vec::new(),
            routing: RoutePolicy::default(),
            fallback_direct: false,
            privacy: PrivacyPolicy::default(),
            security: SecurityPolicy::default(),
            timeouts: TimeoutConfig::default(),
            parameter_handlers: Vec::new(),
        }
    }
}

impl ProxyConfig {
    /// Starts a builder for proxy configuration.
    ///
    /// # Returns
    ///
    /// Proxy configuration builder initialized with direct defaults.
    pub fn builder() -> ProxyConfigBuilder {
        ProxyConfigBuilder::default()
    }

    /// Replaces core timeout values.
    ///
    /// # Parameters
    ///
    /// - `timeouts`: Timeout values to install.
    ///
    /// # Returns
    ///
    /// Updated proxy configuration.
    pub fn with_timeouts(mut self, timeouts: TimeoutConfig) -> Self {
        self.timeouts = timeouts;
        self
    }

    /// Appends one proxy hop to the current default chain.
    ///
    /// # Parameters
    ///
    /// - `node`: Proxy node appended after the existing hops.
    ///
    /// # Returns
    ///
    /// Updated proxy configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when no default chain is configured, when the chain
    /// does not exist, or when `node` cannot be appended to that chain.
    pub fn append_proxy(mut self, node: ProxyNode) -> Result<Self> {
        let chain_name = self
            .routing
            .default_chain
            .clone()
            .or_else(|| self.chains.first().map(|chain| chain.name.clone()))
            .ok_or_else(|| ProxyError::Config("no default chain configured".to_string()))?;
        self.routing.default_chain = Some(chain_name.clone());
        self.append_proxy_to_chain(&chain_name, node)
    }

    /// Appends one proxy hop to an existing chain.
    ///
    /// # Parameters
    ///
    /// - `chain_name`: Name of the chain to update.
    /// - `node`: Proxy node appended after the existing hops.
    ///
    /// # Returns
    ///
    /// Updated proxy configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the chain does not exist, when `node` is a direct
    /// node, or when the target chain already contains a direct node.
    pub fn append_proxy_to_chain(mut self, chain_name: &str, node: ProxyNode) -> Result<Self> {
        if node.kind == TransportKind::Direct {
            return Err(ProxyError::Config(
                "cannot append a direct node as an extra proxy".to_string(),
            ));
        }
        let chain = self
            .chains
            .iter_mut()
            .find(|chain| chain.name == chain_name)
            .ok_or_else(|| ProxyError::Config(format!("chain '{chain_name}' does not exist")))?;
        if chain
            .nodes
            .iter()
            .any(|node| node.kind == TransportKind::Direct)
        {
            return Err(ProxyError::Config(format!(
                "chain '{chain_name}' contains a direct node and cannot be extended"
            )));
        }
        self.upstreams.push(node.clone());
        chain.nodes.push(node);
        Ok(self)
    }

    /// Validates the configuration and returns an effective immutable view.
    ///
    /// # Returns
    ///
    /// Resolved proxy configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the configuration is unsafe or internally
    /// inconsistent.
    pub fn resolve(mut self) -> Result<ResolvedProxyConfig> {
        apply_profile_defaults(&mut self);
        if self.chains.is_empty() {
            self.chains.push(ProxyChain::direct());
        }
        if self.routing.default_chain.is_none() {
            self.routing.default_chain = Some(self.chains[0].name.clone());
        }

        validate_unique_upstream_names(&self.upstreams)?;
        validate_egress_pools(&self.egress_pools, &self.security)?;
        let pool_names: HashSet<_> = self
            .egress_pools
            .iter()
            .map(|pool| pool.name.as_str())
            .collect();

        let mut chain_names = HashSet::new();
        for chain in &self.chains {
            if chain.name.trim().is_empty() {
                return Err(ProxyError::Config(
                    "chain names cannot be empty".to_string(),
                ));
            }
            if !chain_names.insert(chain.name.as_str()) {
                return Err(ProxyError::Config(format!(
                    "chain '{}' is declared more than once",
                    chain.name
                )));
            }
            if chain.nodes.is_empty() {
                return Err(ProxyError::Config(format!(
                    "chain '{}' has no nodes",
                    chain.name
                )));
            }
            validate_chain_nodes(chain, &self.security, &pool_names)?;
        }

        if let Some(default_chain) = &self.routing.default_chain {
            if !chain_names.contains(default_chain.as_str()) {
                return Err(ProxyError::Config(format!(
                    "default chain '{default_chain}' is not declared"
                )));
            }
        }

        for rule in &self.routing.rules {
            if !chain_names.contains(rule.chain.as_str()) {
                return Err(ProxyError::Config(format!(
                    "route rule references undeclared chain '{}'",
                    rule.chain
                )));
            }
        }
        validate_parameter_handler_configs(&self.parameter_handlers)?;

        Ok(ResolvedProxyConfig { inner: self })
    }
}

/// Applies derived defaults for the selected proxy profile.
///
/// # Parameters
///
/// - `config`: Mutable configuration that receives profile-derived defaults.
fn apply_profile_defaults(config: &mut ProxyConfig) {
    match config.profile {
        ProxyProfile::Direct
        | ProxyProfile::SingleProxy
        | ProxyProfile::MultiForwarder
        | ProxyProfile::Advanced => {}
        ProxyProfile::Privacy => {
            config.privacy.minimize_logs = true;
            config.privacy.prevent_direct_fallback = true;
            config.routing.allow_direct_fallback = false;
        }
        ProxyProfile::CensorshipResistance => {
            config.routing.allow_direct_fallback = false;
        }
    }
}
