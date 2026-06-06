use std::collections::HashSet;
use std::env;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::parameters::validate_parameter_handler_configs;
use crate::core::ParameterHandlerConfig;
use crate::core::{
    ApplicationProtocol, ChainStrategy, EgressPool, PrivacyPolicy, ProxyChain, ProxyError,
    ProxyNode, Result, RoutePolicy, RouteRule, SecurityPolicy, TransportKind,
};

/// Operational profile used to derive inspectable defaults.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyProfile {
    /// Direct networking with basic safety defaults.
    #[serde(rename = "system_relay", alias = "direct")]
    Direct,
    /// All traffic is expected to use a single configured upstream.
    #[serde(rename = "single_forwarder", alias = "single_proxy")]
    SingleProxy,
    /// All traffic is expected to use one fixed multi-hop chain.
    MultiForwarder,
    /// Privacy-oriented defaults such as minimized logs and no direct fallback.
    Privacy,
    /// Fallback-friendly profile for censorship-resistance experiments.
    CensorshipResistance,
    /// Explicit configuration without strong implicit behavior.
    Advanced,
}

impl Default for ProxyProfile {
    /// Returns the direct profile as the default.
    fn default() -> Self {
        Self::Direct
    }
}

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

/// How the proxy treats hostnames before a transport requires an IP address.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameResolutionMode {
    /// Preserve hostnames and pass them to capable upstreams.
    PreserveHostname,
    /// Resolve locally as soon as a socket endpoint is needed.
    ResolveWhenRequired,
}

impl Default for NameResolutionMode {
    /// Returns hostname-preserving resolution mode.
    fn default() -> Self {
        Self::PreserveHostname
    }
}

/// Timeout values used by core transports.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeoutConfig {
    /// TCP or upstream connect timeout in milliseconds.
    pub connect_ms: u64,
    /// Proxy handshake timeout in milliseconds.
    pub handshake_ms: u64,
    /// Idle timeout in milliseconds for adapters that enforce one.
    pub idle_ms: u64,
}

impl Default for TimeoutConfig {
    /// Returns conservative core transport timeouts.
    fn default() -> Self {
        Self {
            connect_ms: 10_000,
            handshake_ms: 10_000,
            idle_ms: 60_000,
        }
    }
}

impl TimeoutConfig {
    /// Returns the connect timeout as a duration.
    ///
    /// # Returns
    ///
    /// TCP connect timeout.
    pub fn connect_duration(&self) -> Duration {
        Duration::from_millis(self.connect_ms)
    }

    /// Returns the handshake timeout as a duration.
    ///
    /// # Returns
    ///
    /// Proxy handshake timeout.
    pub fn handshake_duration(&self) -> Duration {
        Duration::from_millis(self.handshake_ms)
    }
}

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

/// Validates that declared upstream names are non-empty and unique.
///
/// # Parameters
///
/// - `upstreams`: Upstream node declarations from the configuration.
///
/// # Errors
///
/// Returns an error when a name is empty or appears more than once.
fn validate_unique_upstream_names(upstreams: &[ProxyNode]) -> Result<()> {
    let mut names = HashSet::new();
    for upstream in upstreams {
        if upstream.name.trim().is_empty() {
            return Err(ProxyError::Config(
                "upstream node names cannot be empty".to_string(),
            ));
        }
        if !names.insert(upstream.name.as_str()) {
            return Err(ProxyError::Config(format!(
                "upstream node '{}' is declared more than once",
                upstream.name
            )));
        }
    }
    Ok(())
}

/// Validates declared egress pools and their upstream references.
///
/// # Parameters
///
/// - `pools`: Egress pool declarations from the configuration.
/// - `security`: Security policy used for upstream TLS checks.
///
/// # Errors
///
/// Returns an error when a pool name is empty or duplicated, a pool has no
/// upstreams, or one pool member is missing or invalid.
fn validate_egress_pools(pools: &[EgressPool], security: &SecurityPolicy) -> Result<()> {
    let mut pool_names = HashSet::new();
    for pool in pools {
        if pool.name.trim().is_empty() {
            return Err(ProxyError::Config(
                "egress pool names cannot be empty".to_string(),
            ));
        }
        if !pool_names.insert(pool.name.as_str()) {
            return Err(ProxyError::Config(format!(
                "egress pool '{}' is declared more than once",
                pool.name
            )));
        }
        if pool.proxy_nodes.is_empty() {
            return Err(ProxyError::Config(format!(
                "egress pool '{}' has no upstreams",
                pool.name
            )));
        }
        if pool.test_batch_size == 0 {
            return Err(ProxyError::Config(format!(
                "egress pool '{}' test_batch_size must be greater than 0",
                pool.name
            )));
        }

        let mut member_names = HashSet::new();
        for upstream in &pool.proxy_nodes {
            if upstream.name.trim().is_empty() {
                return Err(ProxyError::Config(format!(
                    "egress pool '{}' contains an empty upstream reference",
                    pool.name
                )));
            }
            if !member_names.insert(upstream.name.as_str()) {
                return Err(ProxyError::Config(format!(
                    "egress pool '{}' references upstream '{}' more than once",
                    pool.name, upstream.name
                )));
            }
            if matches!(
                upstream.kind,
                TransportKind::Direct | TransportKind::ProxyPool
            ) {
                return Err(ProxyError::Config(format!(
                    "egress pool '{}' upstream '{}' must be a concrete proxy node",
                    pool.name, upstream.name
                )));
            }
            validate_proxy_node(upstream, security)?;
        }
    }
    Ok(())
}

/// Validates node-level constraints inside a proxy chain.
///
/// # Parameters
///
/// - `chain`: Chain to validate.
/// - `security`: Security policy used for upstream TLS checks.
/// - `pool_names`: Declared egress pool names that chain markers may reference.
///
/// # Errors
///
/// Returns an error when nodes are duplicated, incompatible, missing endpoints,
/// or unsafe according to `security`.
fn validate_chain_nodes(
    chain: &ProxyChain,
    security: &SecurityPolicy,
    pool_names: &HashSet<&str>,
) -> Result<()> {
    let has_direct = chain
        .nodes
        .iter()
        .any(|node| node.kind == crate::core::TransportKind::Direct);
    if has_direct && chain.nodes.len() > 1 {
        return Err(ProxyError::Config(format!(
            "chain '{}' mixes direct with proxy nodes",
            chain.name
        )));
    }

    let mut node_names = HashSet::new();
    for node in &chain.nodes {
        if node.name.trim().is_empty() {
            return Err(ProxyError::Config(format!(
                "chain '{}' contains a node with an empty name",
                chain.name
            )));
        }
        if !node_names.insert(node.name.as_str()) {
            return Err(ProxyError::Config(format!(
                "chain '{}' declares node '{}' more than once",
                chain.name, node.name
            )));
        }

        if node.kind == TransportKind::ProxyPool {
            if node.endpoint.is_some() {
                return Err(ProxyError::Config(format!(
                    "proxy pool marker '{}' must not declare an endpoint",
                    node.name
                )));
            }
            if !pool_names.contains(node.name.as_str()) {
                return Err(ProxyError::Config(format!(
                    "chain '{}' references undeclared egress pool '{}'",
                    chain.name, node.name
                )));
            }
        } else {
            validate_proxy_node(node, security)?;
        }
    }

    Ok(())
}

/// Validates one concrete proxy node declaration.
///
/// # Parameters
///
/// - `node`: Proxy node to validate.
/// - `security`: Security policy used for upstream TLS checks.
///
/// # Errors
///
/// Returns an error when the node endpoint is invalid or violates TLS policy.
fn validate_proxy_node(node: &ProxyNode, security: &SecurityPolicy) -> Result<()> {
    match node.kind {
        TransportKind::Direct => {
            if node.endpoint.is_some() {
                return Err(ProxyError::Config(format!(
                    "direct node '{}' must not declare an endpoint",
                    node.name
                )));
            }
        }
        TransportKind::ProxyPool => {
            return Err(ProxyError::Config(format!(
                "proxy pool marker '{}' is not a concrete proxy node",
                node.name
            )));
        }
        _ => {
            let endpoint = node.endpoint.as_deref().ok_or_else(|| {
                ProxyError::Config(format!("node '{}' requires an endpoint", node.name))
            })?;
            let endpoint_destination = crate::core::Destination::from_authority(
                endpoint,
                crate::core::ApplicationProtocol::Tcp,
            )
            .map_err(|_| {
                ProxyError::Config(format!(
                    "node '{}' has an invalid endpoint '{}'",
                    node.name, endpoint
                ))
            })?;
            if endpoint_destination.port == 0 {
                return Err(ProxyError::Config(format!(
                    "node '{}' endpoint port cannot be 0",
                    node.name
                )));
            }
        }
    }

    if !security.allow_insecure_upstreams
        && security.verify_upstream_tls
        && matches!(node.kind, TransportKind::HttpsProxy)
        && !node.verify_tls
    {
        return Err(ProxyError::Config(format!(
            "https upstream '{}' disables tls verification",
            node.name
        )));
    }
    Ok(())
}

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

/// Builds the direct system-relay fallback configuration.
///
/// # Returns
///
/// Direct proxy configuration using the current system network route.
fn system_relay_direct_config() -> ProxyConfig {
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
fn system_relay_config_from_env() -> Result<ProxyConfig> {
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
/// # Parameters
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
/// # Parameters
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
fn single_forwarder_config(
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

/// Effective validated proxy configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedProxyConfig {
    inner: ProxyConfig,
}

impl ResolvedProxyConfig {
    /// Returns the original resolved configuration.
    ///
    /// # Returns
    ///
    /// Underlying validated proxy configuration.
    pub fn as_config(&self) -> &ProxyConfig {
        &self.inner
    }

    /// Returns a chain by name.
    ///
    /// # Parameters
    ///
    /// - `name`: Chain name to look up.
    ///
    /// # Returns
    ///
    /// Matching chain when declared.
    pub fn chain(&self, name: &str) -> Option<&ProxyChain> {
        self.inner.chains.iter().find(|chain| chain.name == name)
    }

    /// Returns an egress pool by name.
    ///
    /// # Parameters
    ///
    /// - `name`: Egress pool name to look up.
    ///
    /// # Returns
    ///
    /// Matching egress pool when declared.
    pub fn egress_pool(&self, name: &str) -> Option<&EgressPool> {
        self.inner
            .egress_pools
            .iter()
            .find(|pool| pool.name == name)
    }

    /// Returns an upstream node by name.
    ///
    /// # Parameters
    ///
    /// - `name`: Upstream node name to look up.
    ///
    /// # Returns
    ///
    /// Matching upstream node when declared.
    pub fn upstream(&self, name: &str) -> Option<&ProxyNode> {
        self.inner
            .upstreams
            .iter()
            .find(|upstream| upstream.name == name)
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
