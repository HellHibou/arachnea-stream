//! In-process proxy core facade and shared runtime state.

use crate::core::dns_resolver::ProxyDnsResolver;
use crate::core::{
    build_parameter_handler, Destination, ParameterDefinition, ParameterRegistry, ProxyChain,
    ProxyConfig, ProxyConfigBuilder, ProxyInventory, ProxyNode, ProxyParameterHandler,
    ProxyPoolMemberStatus, ProxyStats, ResolvedProxyConfig, Result,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, RwLock};

mod connections;
mod dynamic_pools;
mod hops;
mod pool_state;
mod pools;
mod request_routing;
mod resolution;
mod udp;

/// Runtime state remembered for one proxy pool.
#[derive(Clone, Debug, Default)]
struct ProxyPoolRuntimeState {
    /// Cached selected upstream name.
    selected: Option<String>,
    /// Last known member statuses keyed by upstream name.
    statuses: BTreeMap<String, ProxyPoolMemberStatus>,
}

/// Compatibility check used when probing one pool member.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProxyPoolCheckMode {
    /// Candidate must open a tunnel to the supplied target.
    Tunnel,
    /// Final HTTP proxy candidates only need to accept cleartext HTTP forwarding.
    HttpForward,
}

/// Concrete chain produced after resolving proxy pool markers.
#[derive(Clone, Debug)]
struct ResolvedProxyChain {
    /// Chain containing only concrete nodes.
    chain: ProxyChain,
    /// Pool members selected while resolving the chain.
    pool_selections: Vec<ProxyPoolSelection>,
}

/// One pool member selected for a connection attempt.
#[derive(Clone, Debug)]
struct ProxyPoolSelection {
    /// Pool name that supplied the member.
    pool_name: String,
    /// Upstream node name selected from the pool.
    upstream: String,
    /// Endpoint authority (host:port) of the selected proxy, used to
    /// propagate failures to the proxy inventory.
    endpoint: Option<String>,
    /// Opaque affinity key that requested this dynamic selection.
    affinity: Option<String>,
}

/// In-process proxy core used by libraries, servers and client connectors.
#[derive(Clone)]
pub struct ArachneaProxyCore {
    config: Arc<ResolvedProxyConfig>,
    stats: Arc<ProxyStats>,
    parameter_handlers: Arc<Vec<Arc<dyn ProxyParameterHandler>>>,
    insecure_tls_hosts: Arc<RwLock<HashSet<String>>>,
    socks5_local_dns_nodes: Arc<RwLock<HashSet<String>>>,
    proxy_pool_states: Arc<RwLock<BTreeMap<String, ProxyPoolRuntimeState>>>,
    proxy_inventory: Option<Arc<ProxyInventory>>,
    dynamic_proxy_selections: Option<Arc<RwLock<HashMap<Destination, ProxyPoolSelection>>>>,
    pinned_route: Option<
        Arc<(
            Destination,
            ResolvedProxyChain,
            Option<crate::core::ProxySelectionToken>,
        )>,
    >,
    search_context: Option<crate::core::ProxySearchContext>,
    dns_resolver: ProxyDnsResolver,
}

impl fmt::Debug for ArachneaProxyCore {
    /// Formats the proxy core without exposing resolver internals.
    ///
    /// # Parameters
    ///
    /// - `f`: Formatter receiving the debug fields.
    ///
    /// # Returns
    ///
    /// Formatter result for the debug representation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("ArachneaProxyCore");
        debug
            .field("config", &self.config)
            .field("stats", &self.stats)
            .field("parameter_handlers", &self.parameter_handlers.len())
            .field(
                "insecure_tls_hosts",
                &self
                    .insecure_tls_hosts
                    .read()
                    .map(|hosts| hosts.len())
                    .unwrap_or_default(),
            )
            .field(
                "socks5_local_dns_nodes",
                &self
                    .socks5_local_dns_nodes
                    .read()
                    .map(|nodes| nodes.len())
                    .unwrap_or_default(),
            );
        debug.field(
            "proxy_pool_states",
            &self
                .proxy_pool_states
                .read()
                .map(|states| states.len())
                .unwrap_or_default(),
        );
        #[cfg(feature = "arachnea-dns")]
        debug.field(
            "arachnea_dns_enabled",
            &self.dns_resolver.uses_arachnea_dns(),
        );
        debug.finish()
    }
}

/// Builds configured parameter handler objects.
///
/// # Parameters
///
/// - `config`: Resolved proxy configuration containing handler declarations.
///
/// # Returns
///
/// Runtime parameter handlers in configured order.
///
/// # Errors
///
/// Returns an error when one handler configuration is invalid.
fn build_parameter_handlers(
    config: &ResolvedProxyConfig,
) -> Result<Vec<Arc<dyn ProxyParameterHandler>>> {
    config
        .as_config()
        .parameter_handlers
        .iter()
        .map(|handler| build_parameter_handler(handler).map(Arc::from))
        .collect()
}

impl ArachneaProxyCore {
    /// Creates a proxy core from a raw configuration.
    ///
    /// # Parameters
    ///
    /// - `config`: Proxy configuration to validate and resolve.
    ///
    /// # Returns
    ///
    /// Proxy core with shared stats.
    ///
    /// # Errors
    ///
    /// Returns an error when configuration validation fails, or when the
    /// profile-matched DNS core cannot be created while the `arachnea-dns`
    /// feature is enabled.
    pub fn new(config: ProxyConfig) -> Result<Self> {
        Self::from_resolved(config.resolve()?)
    }

    /// Creates a proxy core with an explicit `arachnea-dns` resolver.
    ///
    /// This constructor is available only when the `arachnea-dns` feature is
    /// enabled. Use it when application code wants to provide local records,
    /// custom upstreams, Smart DNS rules, or cache settings instead of the
    /// profile-derived default resolver created by `new`.
    ///
    /// # Parameters
    ///
    /// - `config`: Proxy configuration to validate and resolve.
    /// - `dns_core`: DNS core used when the proxy must resolve hostnames.
    ///
    /// # Returns
    ///
    /// Proxy core with the supplied DNS resolver installed.
    ///
    /// # Errors
    ///
    /// Returns an error when proxy configuration validation fails.
    #[cfg(feature = "arachnea-dns")]
    pub fn new_with_arachnea_dns(
        config: ProxyConfig,
        dns_core: arachnea_dns::core::ArachneaDnsCore,
    ) -> Result<Self> {
        Self::from_resolved_with_arachnea_dns(config.resolve()?, dns_core)
    }

    /// Creates a proxy core from a validated configuration.
    ///
    /// # Parameters
    ///
    /// - `config`: Already resolved proxy configuration.
    ///
    /// # Returns
    ///
    /// Proxy core with shared stats.
    ///
    /// # Errors
    ///
    /// Returns an error when parameter handler configuration is invalid, or
    /// when the profile-matched DNS core cannot be created while the
    /// `arachnea-dns` feature is enabled.
    pub fn from_resolved(config: ResolvedProxyConfig) -> Result<Self> {
        #[cfg(feature = "arachnea-dns")]
        {
            let dns_core =
                Self::default_arachnea_dns_core_for_proxy_profile(&config.as_config().profile)?;
            return Self::from_resolved_with_arachnea_dns(config, dns_core);
        }

        #[cfg(not(feature = "arachnea-dns"))]
        {
            let parameter_handlers = build_parameter_handlers(&config)?;
            Ok(Self {
                config: Arc::new(config),
                stats: Arc::new(ProxyStats::default()),
                parameter_handlers: Arc::new(parameter_handlers),
                insecure_tls_hosts: Arc::new(RwLock::new(HashSet::new())),
                socks5_local_dns_nodes: Arc::new(RwLock::new(HashSet::new())),
                proxy_pool_states: Arc::new(RwLock::new(BTreeMap::new())),
                proxy_inventory: None,
                dynamic_proxy_selections: None,
                pinned_route: None,
                search_context: None,
                dns_resolver: ProxyDnsResolver::system(),
            })
        }
    }

    /// Creates a proxy core from a resolved config with a dynamic proxy inventory.
    ///
    /// # Parameters
    ///
    /// - `config`: Already resolved proxy configuration.
    /// - `inventory`: Dynamic proxy inventory for country-based routing.
    ///
    /// # Returns
    ///
    /// Proxy core with the supplied inventory installed.
    ///
    /// # Errors
    ///
    /// Returns an error when configured proxy parameter handlers are invalid.
    pub fn from_resolved_with_proxy_inventory(
        config: ResolvedProxyConfig,
        inventory: ProxyInventory,
    ) -> Result<Self> {
        #[cfg(feature = "arachnea-dns")]
        {
            let dns_core =
                Self::default_arachnea_dns_core_for_proxy_profile(&config.as_config().profile)?;
            return Self::from_resolved_with_arachnea_dns_and_inventory(
                config, dns_core, inventory,
            );
        }

        #[cfg(not(feature = "arachnea-dns"))]
        {
            let parameter_handlers = build_parameter_handlers(&config)?;
            Ok(Self {
                config: Arc::new(config),
                stats: Arc::new(ProxyStats::default()),
                parameter_handlers: Arc::new(parameter_handlers),
                insecure_tls_hosts: Arc::new(RwLock::new(HashSet::new())),
                socks5_local_dns_nodes: Arc::new(RwLock::new(HashSet::new())),
                proxy_pool_states: Arc::new(RwLock::new(BTreeMap::new())),
                proxy_inventory: Some(Arc::new(inventory)),
                dynamic_proxy_selections: None,
                pinned_route: None,
                search_context: None,
                dns_resolver: ProxyDnsResolver::system(),
            })
        }
    }

    /// Creates a proxy core from a resolved config and explicit `arachnea-dns` core.
    ///
    /// # Parameters
    ///
    /// - `config`: Already resolved proxy configuration.
    /// - `dns_core`: DNS core used when hostnames must be resolved locally.
    ///
    /// # Returns
    ///
    /// Proxy core with the supplied DNS resolver installed.
    ///
    /// # Errors
    ///
    /// Returns an error when configured proxy parameter handlers are invalid.
    #[cfg(feature = "arachnea-dns")]
    pub fn from_resolved_with_arachnea_dns(
        config: ResolvedProxyConfig,
        dns_core: arachnea_dns::core::ArachneaDnsCore,
    ) -> Result<Self> {
        Self::from_resolved_with_arachnea_dns_and_inventory(
            config,
            dns_core,
            ProxyInventory::new(crate::core::InventoryConfig::default(), None, None),
        )
    }

    /// Creates a proxy core from a resolved config, explicit DNS core and
    /// dynamic proxy inventory.
    ///
    /// # Parameters
    ///
    /// - `config`: Already resolved proxy configuration.
    /// - `dns_core`: DNS core used when hostnames must be resolved locally.
    /// - `inventory`: Dynamic proxy inventory for country-based routing.
    ///
    /// # Returns
    ///
    /// Proxy core with the supplied DNS resolver and inventory installed.
    ///
    /// # Errors
    ///
    /// Returns an error when configured proxy parameter handlers are invalid.
    #[cfg(feature = "arachnea-dns")]
    pub fn from_resolved_with_arachnea_dns_and_inventory(
        config: ResolvedProxyConfig,
        dns_core: arachnea_dns::core::ArachneaDnsCore,
        inventory: ProxyInventory,
    ) -> Result<Self> {
        let parameter_handlers = build_parameter_handlers(&config)?;
        let dns_core = Arc::new(dns_core);
        let inventory = inventory.with_arachnea_dns(Arc::clone(&dns_core));
        Ok(Self {
            config: Arc::new(config),
            stats: Arc::new(ProxyStats::default()),
            parameter_handlers: Arc::new(parameter_handlers),
            insecure_tls_hosts: Arc::new(RwLock::new(HashSet::new())),
            socks5_local_dns_nodes: Arc::new(RwLock::new(HashSet::new())),
            proxy_pool_states: Arc::new(RwLock::new(BTreeMap::new())),
            proxy_inventory: Some(Arc::new(inventory)),
            dynamic_proxy_selections: None,
            pinned_route: None,
            search_context: None,
            dns_resolver: ProxyDnsResolver::with_arachnea_dns(dns_core),
        })
    }

    /// Starts a configuration builder.
    ///
    /// # Returns
    ///
    /// Proxy configuration builder.
    pub fn builder() -> ProxyConfigBuilder {
        ProxyConfig::builder()
    }

    /// Returns the resolved configuration.
    ///
    /// # Returns
    ///
    /// Effective validated proxy configuration.
    pub fn config(&self) -> &ResolvedProxyConfig {
        &self.config
    }

    /// Returns shared proxy statistics.
    ///
    /// # Returns
    ///
    /// Shared proxy counters.
    pub fn stats(&self) -> &ProxyStats {
        &self.stats
    }

    /// Returns the registered request parameter definitions.
    ///
    /// The returned list combines built-in defaults with definitions declared
    /// by configured or code-installed parameter handlers.
    ///
    /// # Returns
    ///
    /// Parameter definitions used by inbound protocol adapters.
    pub fn parameter_definitions(&self) -> Vec<ParameterDefinition> {
        let mut registry = ParameterRegistry::with_defaults();
        for handler in self.parameter_handlers.iter() {
            registry.register_handler(handler.as_ref());
        }
        registry.into_definitions()
    }

    /// Replaces the exact-host allowlist eligible for an explicit TLS bypass.
    ///
    /// The allowlist is owned by trusted server configuration. Proxy URL options
    /// can request a bypass but cannot add arbitrary hosts to this list.
    pub fn set_insecure_tls_hosts<I, S>(&self, hosts: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let normalized = hosts
            .into_iter()
            .map(|host| {
                host.as_ref()
                    .trim()
                    .trim_end_matches('.')
                    .to_ascii_lowercase()
            })
            .filter(|host| !host.is_empty())
            .collect::<HashSet<_>>();
        if let Ok(mut allowed_hosts) = self.insecure_tls_hosts.write() {
            *allowed_hosts = normalized;
        }
    }

    /// Returns whether one exact host is allowed to disable certificate validation.
    pub fn allows_insecure_tls_for_host(&self, host: &str) -> bool {
        let normalized = host.trim().trim_end_matches('.').to_ascii_lowercase();
        self.insecure_tls_hosts
            .read()
            .is_ok_and(|allowed_hosts| allowed_hosts.contains(&normalized))
    }

    /// Installs an additional code-defined parameter handler on this core handle.
    ///
    /// Existing clones keep their current handler list so runtime changes remain
    /// local to this handle.
    ///
    /// # Parameters
    ///
    /// - `handler`: Parameter handler appended after configured handlers.
    ///
    /// # Returns
    ///
    /// Updated core handle.
    pub fn with_parameter_handler(mut self, handler: impl ProxyParameterHandler + 'static) -> Self {
        let mut handlers = self.parameter_handlers.as_ref().clone();
        handlers.push(Arc::new(handler));
        self.parameter_handlers = Arc::new(handlers);
        self
    }

    /// Installs a dynamic proxy inventory on this core handle.
    ///
    /// Existing clones keep their current inventory. Use this when the core
    /// was created without [`ArachneaProxyCore::from_resolved_with_proxy_inventory`].
    ///
    /// # Parameters
    ///
    /// - `inventory`: Dynamic proxy inventory for country-based routing.
    ///
    /// # Returns
    ///
    /// Updated core handle.
    pub fn with_proxy_inventory(mut self, inventory: ProxyInventory) -> Self {
        self.proxy_inventory = Some(Arc::new(inventory));
        self
    }

    /// Returns a reference to the dynamic proxy inventory, if one is configured.
    ///
    /// # Returns
    ///
    /// Optional reference to the shared inventory.
    pub fn proxy_inventory(&self) -> Option<&Arc<ProxyInventory>> {
        self.proxy_inventory.as_ref()
    }

    /// Appends one proxy hop to the current default chain.
    ///
    /// This method updates only this core handle. Existing clones keep their
    /// current resolved configuration, which avoids surprising route changes in
    /// already-shared clients.
    ///
    /// # Parameters
    ///
    /// - `node`: Proxy hop appended after the existing default chain hops.
    ///
    /// # Errors
    ///
    /// Returns an error when the default chain cannot be extended or the
    /// updated configuration fails validation.
    pub fn append_proxy(&mut self, node: ProxyNode) -> Result<()> {
        let config = self.config.as_config().clone().append_proxy(node)?;
        self.config = Arc::new(config.resolve()?);
        Ok(())
    }

    /// Creates the minimal HTTP convenience client backed by this core.
    ///
    /// # Returns
    ///
    /// Simple HTTP client using this proxy core for outbound streams.
    pub fn http_client(&self) -> crate::core::http::client::SimpleHttpClient {
        crate::core::http::client::SimpleHttpClient::new(self.clone())
    }
}
