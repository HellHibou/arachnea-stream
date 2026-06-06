use std::collections::{BTreeMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, RwLock};
use std::{fmt, io};

use tokio::net::{lookup_host, TcpStream, UdpSocket};
use tokio::time;

use crate::core::transport::{http, socks, tls};
#[cfg(feature = "arachnea-dns")]
use crate::core::ProxyProfile;
use crate::core::{
    build_parameter_handler, ApplicationProtocol, ConnectMetadata, ConnectRequest, Destination,
    DestinationAddress, HttpRequestStream, HttpRequestTargetForm, ParameterDefinition,
    ParameterRegistry, ProxyChain, ProxyConfig, ProxyConfigBuilder, ProxyError,
    ProxyNameResolutionMode, ProxyNode, ProxyParameterHandler, ProxyPoolMemberState,
    ProxyPoolMemberStatus, ProxyStats, ProxyStream, ResolvedProxyConfig, Result,
    Socks5UdpAssociation, TransportKind,
};

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
}

/// In-process proxy core used by libraries, servers and client connectors.
#[derive(Clone)]
pub struct ArachneaProxyCore {
    config: Arc<ResolvedProxyConfig>,
    stats: Arc<ProxyStats>,
    parameter_handlers: Arc<Vec<Arc<dyn ProxyParameterHandler>>>,
    socks5_local_dns_nodes: Arc<RwLock<HashSet<String>>>,
    proxy_pool_states: Arc<RwLock<BTreeMap<String, ProxyPoolRuntimeState>>>,
    #[cfg(feature = "arachnea-dns")]
    dns_core: Option<Arc<arachnea_dns::core::ArachneaDnsCore>>,
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
        debug.field("arachnea_dns_enabled", &self.dns_core.is_some());
        debug.finish()
    }
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
                socks5_local_dns_nodes: Arc::new(RwLock::new(HashSet::new())),
                proxy_pool_states: Arc::new(RwLock::new(BTreeMap::new())),
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
        let parameter_handlers = build_parameter_handlers(&config)?;
        Ok(Self {
            config: Arc::new(config),
            stats: Arc::new(ProxyStats::default()),
            parameter_handlers: Arc::new(parameter_handlers),
            socks5_local_dns_nodes: Arc::new(RwLock::new(HashSet::new())),
            proxy_pool_states: Arc::new(RwLock::new(BTreeMap::new())),
            dns_core: Some(Arc::new(dns_core)),
        })
    }

    /// Builds the default DNS configuration that corresponds to a proxy profile.
    ///
    /// The mapping intentionally avoids hard-coded public resolvers. Profiles
    /// that need a concrete forwarder source use the system resolver as the
    /// default upstream so callers can replace it with a private resolver by
    /// installing their own `ArachneaDnsCore`.
    ///
    /// # Parameters
    ///
    /// - `profile`: Proxy profile whose DNS counterpart should be built.
    ///
    /// # Returns
    ///
    /// DNS core configuration initialized from the equivalent DNS profile.
    #[cfg(feature = "arachnea-dns")]
    pub fn arachnea_dns_config_for_proxy_profile(
        profile: &ProxyProfile,
    ) -> arachnea_dns::core::CoreConfig {
        use arachnea_dns::core::{Upstream, UsageProfile as DnsUsageProfile};

        match profile {
            ProxyProfile::Direct => DnsUsageProfile::SystemRelay.config(),
            ProxyProfile::SingleProxy => {
                DnsUsageProfile::SingleForwarder(Upstream::system(None)).config()
            }
            ProxyProfile::MultiForwarder => {
                DnsUsageProfile::Resilience(vec![Upstream::system(None)]).config()
            }
            ProxyProfile::Privacy => {
                DnsUsageProfile::Privacy(vec![Upstream::system(None)]).config()
            }
            ProxyProfile::CensorshipResistance => DnsUsageProfile::Resilience(Vec::new()).config(),
            ProxyProfile::Advanced => DnsUsageProfile::Advanced.config(),
        }
    }

    /// Builds the default DNS core that corresponds to a proxy profile.
    ///
    /// # Parameters
    ///
    /// - `profile`: Proxy profile whose DNS counterpart should be built.
    ///
    /// # Returns
    ///
    /// DNS core initialized from the equivalent DNS profile.
    ///
    /// # Errors
    ///
    /// Returns an error when the DNS profile cannot be resolved.
    #[cfg(feature = "arachnea-dns")]
    fn default_arachnea_dns_core_for_proxy_profile(
        profile: &ProxyProfile,
    ) -> Result<arachnea_dns::core::ArachneaDnsCore> {
        arachnea_dns::core::ArachneaDnsCore::new(Self::arachnea_dns_config_for_proxy_profile(
            profile,
        ))
        .map_err(|error| ProxyError::Dns(error.to_string()))
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

    /// Returns runtime status for members of a proxy pool.
    ///
    /// Members remain `Untested` until the pool is first used by this core
    /// instance. Once a pool has selected a working member, that selection is
    /// cached and reported through the `selected` flag.
    ///
    /// # Parameters
    ///
    /// - `pool_name`: Egress pool to inspect.
    ///
    /// # Returns
    ///
    /// Status entries in configured upstream order.
    ///
    /// # Errors
    ///
    /// Returns an error when the pool is not declared.
    pub fn proxy_pool_statuses(&self, pool_name: &str) -> Result<Vec<ProxyPoolMemberState>> {
        let pool = self.config.egress_pool(pool_name).ok_or_else(|| {
            ProxyError::RouteUnavailable(format!("egress pool '{pool_name}' not found"))
        })?;
        let state = self
            .proxy_pool_states
            .read()
            .ok()
            .and_then(|states| states.get(pool_name).cloned());
        Ok(pool
            .proxy_nodes
            .iter()
            .map(|node| ProxyPoolMemberState {
                upstream: node.name.clone(),
                status: state
                    .as_ref()
                    .and_then(|state| state.statuses.get(&node.name).cloned())
                    .unwrap_or(ProxyPoolMemberStatus::Untested),
                selected: state
                    .as_ref()
                    .and_then(|state| state.selected.as_deref())
                    .is_some_and(|selected| selected == node.name),
            })
            .collect())
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

    /// Opens a stream to a destination using route policy.
    ///
    /// # Parameters
    ///
    /// - `destination`: Target destination requested by the caller.
    ///
    /// # Returns
    ///
    /// Connected proxy stream.
    ///
    /// # Errors
    ///
    /// Returns an error when policy validation, route selection, or connection
    /// setup fails.
    pub async fn connect(&self, destination: Destination) -> Result<ProxyStream> {
        self.connect_request(ConnectRequest::new(destination)).await
    }

    /// Opens a direct connected UDP socket to a destination.
    ///
    /// Proxied UDP is intentionally kept explicit: non-direct UDP routes return
    /// an unsupported error until a transport-specific association is selected.
    ///
    /// # Parameters
    ///
    /// - `destination`: UDP target destination.
    ///
    /// # Returns
    ///
    /// Connected UDP socket for direct routes.
    ///
    /// # Errors
    ///
    /// Returns an error when policy validation fails, the route is proxied, or
    /// socket setup fails.
    pub async fn connect_udp(&self, destination: Destination) -> Result<UdpSocket> {
        self.connect_udp_request(ConnectRequest::new(destination))
            .await
    }

    /// Opens a direct connected UDP socket using a full connect request.
    ///
    /// # Parameters
    ///
    /// - `request`: UDP destination and client context for route selection.
    ///
    /// # Returns
    ///
    /// Connected UDP socket for direct routes.
    ///
    /// # Errors
    ///
    /// Returns an error when policy validation fails, the route is proxied, or
    /// socket setup fails.
    pub async fn connect_udp_request(&self, request: ConnectRequest) -> Result<UdpSocket> {
        self.config
            .as_config()
            .security
            .validate_destination(&request.destination)?;
        let decision = self
            .config
            .as_config()
            .routing
            .decide(&self.config, &request)?;
        let chain = self.apply_parameter_handlers(&decision.chain, &request)?;
        let resolved_chain = self.resolve_proxy_pools_in_chain(&chain, &request).await?;
        if resolved_chain
            .chain
            .nodes
            .iter()
            .any(|node| node.kind != TransportKind::Direct)
        {
            return Err(ProxyError::Unsupported(
                "proxied udp requires a transport-specific association",
            ));
        }
        let destination_addr = self.resolve_destination_addr(&request.destination).await?;
        let bind_addr = if destination_addr.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        };
        let socket = UdpSocket::bind(bind_addr).await?;
        socket.connect(destination_addr).await?;
        Ok(socket)
    }

    /// Opens a SOCKS5 UDP association when the selected UDP route uses one SOCKS5 upstream.
    ///
    /// Multi-hop UDP is deliberately not hidden here: routes with more than one
    /// proxy hop return an unsupported error until transport-specific semantics
    /// are implemented for each chain shape.
    ///
    /// # Parameters
    ///
    /// - `destination`: UDP target used for route selection and policy checks.
    ///
    /// # Returns
    ///
    /// SOCKS5 UDP association for a single SOCKS5 upstream route.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected route is not a single SOCKS5 upstream
    /// or the association handshake fails.
    pub async fn connect_socks5_udp_association(
        &self,
        destination: Destination,
    ) -> Result<Socks5UdpAssociation> {
        self.connect_socks5_udp_association_request(ConnectRequest::new(destination))
            .await
    }

    /// Opens a SOCKS5 UDP association using a full connect request.
    ///
    /// # Parameters
    ///
    /// - `request`: UDP destination and client context for route selection.
    ///
    /// # Returns
    ///
    /// SOCKS5 UDP association for a single SOCKS5 upstream route.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected route is not a single SOCKS5 upstream
    /// or the association handshake fails.
    pub async fn connect_socks5_udp_association_request(
        &self,
        request: ConnectRequest,
    ) -> Result<Socks5UdpAssociation> {
        self.config
            .as_config()
            .security
            .validate_destination(&request.destination)?;
        let decision = self
            .config
            .as_config()
            .routing
            .decide(&self.config, &request)?;
        let chain = self.apply_parameter_handlers(&decision.chain, &request)?;
        let resolved_chain = self.resolve_proxy_pools_in_chain(&chain, &request).await?;
        let proxy_nodes: Vec<_> = resolved_chain
            .chain
            .nodes
            .iter()
            .filter(|node| node.kind != TransportKind::Direct)
            .cloned()
            .collect();
        if proxy_nodes.len() != 1
            || !matches!(
                proxy_nodes[0].kind,
                TransportKind::Socks5 | TransportKind::TorSocks
            )
        {
            return Err(ProxyError::Unsupported(
                "socks5 udp associate requires a single socks5 upstream route",
            ));
        }

        let node = &proxy_nodes[0];
        let mut control = self
            .connect_first_proxy(node, &resolved_chain.chain)
            .await?;
        let credentials = node.credentials()?;
        let relay_endpoint = socks::udp_associate(
            &mut control,
            self.config.as_config().timeouts.handshake_duration(),
            credentials.as_ref(),
        )
        .await?;
        control.metadata_mut().route_name = Some(decision.selected_chain);
        control.metadata_mut().transport_kind = Some(node.kind.clone());
        control.metadata_mut().hops.push(node.name.clone());
        let relay_addr = self.resolve_udp_relay_addr(node, &relay_endpoint).await?;
        let bind_addr = if relay_addr.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        };
        let socket = UdpSocket::bind(bind_addr).await?;
        self.stats.record_opened();
        Ok(Socks5UdpAssociation::new(socket, relay_addr, control))
    }

    /// Opens a stream using a full connect request.
    ///
    /// # Parameters
    ///
    /// - `request`: Destination and client context for route selection.
    ///
    /// # Returns
    ///
    /// Connected proxy stream.
    ///
    /// # Errors
    ///
    /// Returns an error when policy validation, routing, or transport setup
    /// fails.
    pub async fn connect_request(&self, request: ConnectRequest) -> Result<ProxyStream> {
        self.config
            .as_config()
            .security
            .validate_destination(&request.destination)?;
        let decision = self
            .config
            .as_config()
            .routing
            .decide(&self.config, &request)?;
        let chain = self.apply_parameter_handlers(&decision.chain, &request)?;
        let retry_budget = self.proxy_pool_retry_budget(&chain);

        for attempt in 0..=retry_budget {
            let resolved_chain = self.resolve_proxy_pools_in_chain(&chain, &request).await?;
            match self.connect_chain(&resolved_chain.chain, &request).await {
                Ok(mut stream) => {
                    stream.metadata_mut().route_name = Some(decision.selected_chain);
                    self.stats.record_opened();
                    return Ok(stream);
                }
                Err(_) if attempt < retry_budget && !resolved_chain.pool_selections.is_empty() => {
                    self.mark_proxy_pool_selections_ko(&resolved_chain.pool_selections);
                    continue;
                }
                Err(error) => {
                    self.stats.record_failed();
                    return Err(error);
                }
            }
        }

        self.stats.record_failed();
        Err(ProxyError::RouteUnavailable(
            "proxy pool retry budget exhausted".to_string(),
        ))
    }

    /// Opens a stream for relaying one explicit HTTP request.
    ///
    /// When the selected route ends with an HTTP or HTTPS proxy and the target
    /// protocol is plain HTTP, this method connects to that proxy and asks the
    /// caller to send an absolute-form request target. Other routes connect to
    /// the final destination and expect origin-form request targets.
    ///
    /// # Parameters
    ///
    /// - `request`: HTTP destination and client context for route selection.
    ///
    /// # Returns
    ///
    /// Connected stream plus the HTTP request-target form expected by the next
    /// peer.
    ///
    /// # Errors
    ///
    /// Returns an error when policy validation, routing, or connection setup
    /// fails.
    pub async fn connect_http_request(&self, request: ConnectRequest) -> Result<HttpRequestStream> {
        self.config
            .as_config()
            .security
            .validate_destination(&request.destination)?;
        let decision = self
            .config
            .as_config()
            .routing
            .decide(&self.config, &request)?;
        let chain = self.apply_parameter_handlers(&decision.chain, &request)?;
        let retry_budget = self.proxy_pool_retry_budget(&chain);

        for attempt in 0..=retry_budget {
            let resolved_chain = self.resolve_proxy_pools_in_chain(&chain, &request).await?;
            let use_absolute_form = request.destination.protocol == ApplicationProtocol::Http
                && resolved_chain
                    .chain
                    .nodes
                    .iter()
                    .rev()
                    .find(|node| node.kind != TransportKind::Direct)
                    .is_some_and(|node| {
                        matches!(
                            node.kind,
                            TransportKind::HttpProxy | TransportKind::HttpsProxy
                        )
                    });

            let result = if use_absolute_form {
                self.connect_chain_to_http_proxy(&resolved_chain.chain)
                    .await
            } else {
                self.connect_chain(&resolved_chain.chain, &request).await
            };

            let proxy_authorization = if use_absolute_form {
                selected_http_proxy_credentials(&resolved_chain.chain)?.map(|credentials| {
                    format!("Basic {}", http::basic_authorization_value(&credentials))
                })
            } else {
                None
            };

            match result {
                Ok(mut stream) => {
                    stream.metadata_mut().route_name = Some(decision.selected_chain);
                    self.stats.record_opened();
                    return Ok(HttpRequestStream {
                        stream,
                        target_form: if use_absolute_form {
                            HttpRequestTargetForm::AbsoluteForm
                        } else {
                            HttpRequestTargetForm::OriginForm
                        },
                        proxy_authorization,
                    });
                }
                Err(_) if attempt < retry_budget && !resolved_chain.pool_selections.is_empty() => {
                    self.mark_proxy_pool_selections_ko(&resolved_chain.pool_selections);
                    continue;
                }
                Err(error) => {
                    self.stats.record_failed();
                    return Err(error);
                }
            }
        }

        self.stats.record_failed();
        Err(ProxyError::RouteUnavailable(
            "proxy pool retry budget exhausted".to_string(),
        ))
    }

    /// Creates the minimal HTTP convenience client backed by this core.
    ///
    /// # Returns
    ///
    /// Simple HTTP client using this proxy core for outbound streams.
    pub fn http_client(&self) -> crate::core::http::client::SimpleHttpClient {
        crate::core::http::client::SimpleHttpClient::new(self.clone())
    }

    /// Applies configured request parameter handlers to a selected chain.
    ///
    /// # Parameters
    ///
    /// - `chain`: Chain selected by the route policy.
    /// - `request`: Request whose client context feeds parameter handlers.
    ///
    /// # Returns
    ///
    /// Chain with any request-local extra proxies appended.
    ///
    /// # Errors
    ///
    /// Returns an error when a handler fails.
    fn apply_parameter_handlers(
        &self,
        chain: &ProxyChain,
        request: &ConnectRequest,
    ) -> Result<ProxyChain> {
        let mut effective_chain = chain.clone();
        for handler in self.parameter_handlers.iter() {
            let decision = handler.proxy_from_parameters(&request.client_context)?;
            if let Some(proxy) = decision.extra_proxy {
                effective_chain.nodes.push(proxy);
            }
            if decision.stop {
                break;
            }
        }
        Ok(effective_chain)
    }

    /// Replaces proxy pool markers in a chain with selected concrete upstreams.
    ///
    /// # Parameters
    ///
    /// - `chain`: Chain that may contain `ProxyPool` marker nodes.
    /// - `request`: Request whose destination is used for first-use pool checks.
    ///
    /// # Returns
    ///
    /// Chain containing only concrete direct or proxy nodes plus selected pool
    /// members.
    ///
    /// # Errors
    ///
    /// Returns an error when a pool is undeclared or no candidate works.
    async fn resolve_proxy_pools_in_chain(
        &self,
        chain: &ProxyChain,
        request: &ConnectRequest,
    ) -> Result<ResolvedProxyChain> {
        let mut resolved = chain.clone();
        resolved.nodes = Vec::with_capacity(chain.nodes.len());
        let mut pool_selections = Vec::new();
        for (index, node) in chain.nodes.iter().enumerate() {
            if node.kind == TransportKind::ProxyPool {
                let (target, check_mode) =
                    proxy_pool_check_target(chain, index, &request.destination)?;
                let selected = self
                    .select_proxy_pool_member(&node.name, &target, check_mode)
                    .await?;
                pool_selections.push(ProxyPoolSelection {
                    pool_name: node.name.clone(),
                    upstream: selected.name.clone(),
                });
                resolved.nodes.push(selected);
            } else {
                resolved.nodes.push(node.clone());
            }
        }
        Ok(ResolvedProxyChain {
            chain: resolved,
            pool_selections,
        })
    }

    /// Selects and caches one working upstream from a proxy pool.
    ///
    /// # Parameters
    ///
    /// - `pool_name`: Egress pool name referenced by the chain.
    /// - `target`: Destination used to test candidates.
    /// - `check_mode`: Compatibility check required at this chain position.
    ///
    /// # Returns
    ///
    /// Concrete proxy node selected from the pool.
    ///
    /// # Errors
    ///
    /// Returns an error when the pool is undeclared, a member is missing, or
    /// all candidates fail their compatibility checks.
    async fn select_proxy_pool_member(
        &self,
        pool_name: &str,
        target: &Destination,
        check_mode: ProxyPoolCheckMode,
    ) -> Result<ProxyNode> {
        let pool = self.config.egress_pool(pool_name).cloned().ok_or_else(|| {
            ProxyError::RouteUnavailable(format!("egress pool '{pool_name}' not found"))
        })?;
        self.mark_proxy_pool_members_untested(&pool);

        loop {
            if let Some(selected) = self.first_ok_proxy_pool_member(&pool) {
                self.store_proxy_pool_selected_member(&pool.name, &selected.name);
                return Ok(selected);
            }

            let candidates = self.next_untested_proxy_pool_batch(&pool)?;
            if candidates.is_empty() {
                return Err(ProxyError::RouteUnavailable(format!(
                    "egress pool '{}' has no working proxy candidates",
                    pool.name
                )));
            }

            let mut handles = Vec::with_capacity(candidates.len());
            for candidate in candidates {
                let core = self.clone();
                let target = target.clone();
                let pool_name = pool.name.clone();
                handles.push(tokio::spawn(async move {
                    let result = core
                        .check_proxy_pool_member(&pool_name, &candidate, &target, check_mode)
                        .await;
                    (candidate, result)
                }));
            }

            let mut statuses = BTreeMap::new();
            for handle in handles {
                let (candidate, result) = handle.await.map_err(|error| {
                    ProxyError::RouteUnavailable(format!("proxy pool worker failed: {error}"))
                })?;
                match result {
                    Ok(()) => {
                        statuses.insert(candidate.name.clone(), ProxyPoolMemberStatus::Ok);
                    }
                    Err(error) => {
                        tracing::debug!(
                            proxy_pool = %pool.name,
                            proxy_node = %candidate.name,
                            %error,
                            "proxy pool candidate failed compatibility check"
                        );
                        statuses.insert(candidate.name.clone(), ProxyPoolMemberStatus::Ko);
                    }
                }
            }
            self.merge_proxy_pool_statuses(&pool.name, statuses);
        }
    }

    /// Returns the first working pool member in configured order.
    ///
    /// # Parameters
    ///
    /// - `pool`: Egress pool to inspect.
    ///
    /// # Returns
    ///
    /// First `Ok` member when any.
    fn first_ok_proxy_pool_member(&self, pool: &crate::core::EgressPool) -> Option<ProxyNode> {
        let states = self.proxy_pool_states.read().ok()?;
        let state = states.get(&pool.name)?;
        if let Some(selected) = &state.selected {
            if state.statuses.get(selected) == Some(&ProxyPoolMemberStatus::Ok) {
                return pool
                    .proxy_nodes
                    .iter()
                    .find(|node| node.name == *selected)
                    .cloned();
            }
        }
        pool.proxy_nodes
            .iter()
            .find(|node| state.statuses.get(&node.name) == Some(&ProxyPoolMemberStatus::Ok))
            .cloned()
    }

    /// Returns the next untested pool member batch.
    ///
    /// # Parameters
    ///
    /// - `pool`: Egress pool whose untested members should be selected.
    ///
    /// # Returns
    ///
    /// Concrete upstream nodes for at most `pool.test_batch_size` untested members.
    ///
    /// # Errors
    ///
    /// Returns an error when the pool references a missing upstream.
    fn next_untested_proxy_pool_batch(
        &self,
        pool: &crate::core::EgressPool,
    ) -> Result<Vec<ProxyNode>> {
        let statuses = self
            .proxy_pool_states
            .read()
            .ok()
            .and_then(|states| states.get(&pool.name).map(|state| state.statuses.clone()))
            .unwrap_or_default();
        Ok(pool
            .proxy_nodes
            .iter()
            .filter(|node| {
                statuses
                    .get(&node.name)
                    .is_none_or(|status| *status == ProxyPoolMemberStatus::Untested)
            })
            .take(pool.test_batch_size)
            .cloned()
            .collect())
    }

    /// Initializes untested status entries for a proxy pool.
    ///
    /// # Parameters
    ///
    /// - `pool`: Egress pool being prepared.
    fn mark_proxy_pool_members_untested(&self, pool: &crate::core::EgressPool) {
        if let Ok(mut states) = self.proxy_pool_states.write() {
            let state = states.entry(pool.name.clone()).or_default();
            for node in &pool.proxy_nodes {
                state
                    .statuses
                    .entry(node.name.clone())
                    .or_insert(ProxyPoolMemberStatus::Untested);
            }
        }
    }

    /// Stores the selected proxy pool member.
    ///
    /// # Parameters
    ///
    /// - `pool_name`: Egress pool name that selected a member.
    /// - `selected`: Upstream node selected for future requests from this pool.
    fn store_proxy_pool_selected_member(&self, pool_name: &str, selected: &str) {
        if let Ok(mut states) = self.proxy_pool_states.write() {
            states.entry(pool_name.to_string()).or_default().selected = Some(selected.to_string());
        }
    }

    /// Merges status updates into one proxy pool runtime state.
    ///
    /// # Parameters
    ///
    /// - `pool_name`: Egress pool name that received updates.
    /// - `statuses`: Status updates keyed by upstream node name.
    fn merge_proxy_pool_statuses(
        &self,
        pool_name: &str,
        statuses: BTreeMap<String, ProxyPoolMemberStatus>,
    ) {
        if let Ok(mut states) = self.proxy_pool_states.write() {
            let state = states.entry(pool_name.to_string()).or_default();
            for (upstream, status) in statuses {
                state.statuses.insert(upstream, status);
            }
            if state
                .selected
                .as_ref()
                .and_then(|selected| state.statuses.get(selected))
                == Some(&ProxyPoolMemberStatus::Ko)
            {
                state.selected = None;
            }
        }
    }

    /// Marks selected pool members as failed after a connection setup failure.
    ///
    /// # Parameters
    ///
    /// - `selections`: Pool selections used by the failed connection attempt.
    fn mark_proxy_pool_selections_ko(&self, selections: &[ProxyPoolSelection]) {
        if let Ok(mut states) = self.proxy_pool_states.write() {
            for selection in selections {
                let state = states.entry(selection.pool_name.clone()).or_default();
                state
                    .statuses
                    .insert(selection.upstream.clone(), ProxyPoolMemberStatus::Ko);
                if state.selected.as_deref() == Some(selection.upstream.as_str()) {
                    state.selected = None;
                }
            }
        }
    }

    /// Returns the number of retry attempts allowed for pool-backed chains.
    ///
    /// # Parameters
    ///
    /// - `chain`: Chain that may contain proxy pool markers.
    ///
    /// # Returns
    ///
    /// Sum of pool member counts referenced by the chain.
    fn proxy_pool_retry_budget(&self, chain: &ProxyChain) -> usize {
        chain
            .nodes
            .iter()
            .filter(|node| node.kind == TransportKind::ProxyPool)
            .filter_map(|node| self.config.egress_pool(&node.name))
            .map(|pool| pool.proxy_nodes.len())
            .sum()
    }

    /// Checks whether one pool member can support this chain position.
    ///
    /// Final HTTP or HTTPS proxy candidates for plain HTTP forwarding are
    /// checked with endpoint reachability. Other candidates use the normal
    /// single-node chain path, including SOCKS DNS fallback behavior. The
    /// successful stream is dropped immediately; later traffic opens its own
    /// stream through the cached member.
    ///
    /// # Parameters
    ///
    /// - `pool_name`: Pool name used for diagnostic metadata.
    /// - `node`: Candidate proxy node to test.
    /// - `target`: Destination used for the compatibility check.
    /// - `check_mode`: Compatibility check required at this chain position.
    ///
    /// # Errors
    ///
    /// Returns an error when the candidate is not a concrete proxy or when the
    /// endpoint, TLS setup, or protocol handshake fails.
    async fn check_proxy_pool_member(
        &self,
        pool_name: &str,
        node: &ProxyNode,
        target: &Destination,
        check_mode: ProxyPoolCheckMode,
    ) -> Result<()> {
        if matches!(node.kind, TransportKind::Direct | TransportKind::ProxyPool) {
            return Err(ProxyError::Config(format!(
                "egress pool '{pool_name}' member '{}' is not a concrete proxy",
                node.name
            )));
        }
        let chain = ProxyChain::single(format!("egress_pool:{pool_name}"), node.clone());
        if check_mode == ProxyPoolCheckMode::HttpForward
            && matches!(
                node.kind,
                TransportKind::HttpProxy | TransportKind::HttpsProxy
            )
        {
            let stream = self.connect_first_proxy(node, &chain).await?;
            if node.kind == TransportKind::HttpsProxy {
                drop(self.wrap_https_proxy_stream(stream, node).await?);
            }
            return Ok(());
        }
        self.connect_chain(&chain, &ConnectRequest::new(target.clone()))
            .await
            .map(|_| ())
    }

    /// Connects a request through a selected proxy chain.
    ///
    /// # Parameters
    ///
    /// - `chain`: Resolved proxy chain selected by routing.
    /// - `request`: Original connect request and client context.
    ///
    /// # Returns
    ///
    /// A connected proxy stream for the destination.
    ///
    /// # Errors
    ///
    /// Returns an error when any hop is invalid, unsupported, times out, or
    /// rejects the tunnel.
    async fn connect_chain(
        &self,
        chain: &ProxyChain,
        request: &ConnectRequest,
    ) -> Result<ProxyStream> {
        let retry_budget = chain
            .nodes
            .iter()
            .filter(|node| {
                matches!(node.kind, TransportKind::Socks5 | TransportKind::TorSocks)
                    && node.effective_name_resolution()
                        == ProxyNameResolutionMode::ProxyThenLocalFallback
            })
            .count();

        for attempt in 0..=retry_budget {
            match self.connect_chain_once(chain, request).await {
                Ok(stream) => return Ok(stream),
                Err(error) if attempt < retry_budget => {
                    if self
                        .flag_socks5_local_dns_fallback(chain, &request.destination, &error)
                        .await
                    {
                        continue;
                    }
                    return Err(error);
                }
                Err(error) => return Err(error),
            }
        }

        Err(ProxyError::RouteUnavailable(
            "socks5 dns fallback retry budget exhausted".to_string(),
        ))
    }

    /// Opens a stream through a selected proxy chain once.
    ///
    /// # Parameters
    ///
    /// - `chain`: Resolved proxy chain selected by routing.
    /// - `request`: Original connect request and client context.
    ///
    /// # Returns
    ///
    /// A connected proxy stream for the destination.
    ///
    /// # Errors
    ///
    /// Returns an error from the first failed hop without applying retry logic.
    async fn connect_chain_once(
        &self,
        chain: &ProxyChain,
        request: &ConnectRequest,
    ) -> Result<ProxyStream> {
        let proxy_nodes: Vec<_> = chain
            .nodes
            .iter()
            .filter(|node| node.kind != TransportKind::Direct)
            .cloned()
            .collect();

        if proxy_nodes.is_empty() {
            let destination = self.resolve_destination_to_ip(&request.destination).await?;
            return crate::core::transport::direct::connect_tcp(
                &destination,
                self.config.as_config().timeouts.connect_duration(),
            )
            .await;
        }

        let first = &proxy_nodes[0];
        let mut stream = self.connect_first_proxy(first, chain).await?;

        for (index, node) in proxy_nodes.iter().enumerate() {
            if node.kind == TransportKind::HttpsProxy {
                let endpoint = node.endpoint.as_deref().ok_or_else(|| {
                    ProxyError::Config(format!("node '{}' has no endpoint", node.name))
                })?;
                let server_name = endpoint_host(endpoint)?;
                let metadata = stream.metadata().clone();
                let tls_stream = tls::client_tls(
                    stream,
                    server_name,
                    self.config.as_config().timeouts.handshake_duration(),
                    node.verify_tls,
                )
                .await?;
                stream = ProxyStream::new(tls_stream, metadata);
            }

            let target = if let Some(next) = proxy_nodes.get(index + 1) {
                let endpoint = next.endpoint.as_deref().ok_or_else(|| {
                    ProxyError::Config(format!("node '{}' has no endpoint", next.name))
                })?;
                Destination::from_authority(endpoint, crate::core::ApplicationProtocol::Tcp)?
            } else {
                request.destination.clone()
            };

            self.connect_proxy_hop(&mut stream, node, &target).await?;
            stream.metadata_mut().hops.push(node.name.clone());
            stream.metadata_mut().transport_kind = Some(node.kind.clone());
        }

        Ok(stream)
    }

    /// Connects through a chain up to its final HTTP proxy hop.
    ///
    /// # Parameters
    ///
    /// - `chain`: Chain whose last non-direct hop must be an HTTP or HTTPS proxy.
    ///
    /// # Returns
    ///
    /// A stream connected to the final HTTP proxy, ready for absolute-form
    /// request forwarding.
    ///
    /// # Errors
    ///
    /// Returns an error when the chain has no HTTP proxy endpoint, contains
    /// unsupported hops, or a hop connection fails.
    async fn connect_chain_to_http_proxy(&self, chain: &ProxyChain) -> Result<ProxyStream> {
        let proxy_nodes: Vec<_> = chain
            .nodes
            .iter()
            .filter(|node| node.kind != TransportKind::Direct)
            .cloned()
            .collect();
        let final_index = proxy_nodes
            .iter()
            .rposition(|node| {
                matches!(
                    node.kind,
                    TransportKind::HttpProxy | TransportKind::HttpsProxy
                )
            })
            .ok_or_else(|| {
                ProxyError::RouteUnavailable(format!(
                    "chain '{}' does not end at an http proxy",
                    chain.name
                ))
            })?;
        if final_index + 1 != proxy_nodes.len() {
            return Err(ProxyError::RouteUnavailable(format!(
                "chain '{}' has non-http nodes after its http proxy",
                chain.name
            )));
        }

        let first = &proxy_nodes[0];
        let mut stream = self.connect_first_proxy(first, chain).await?;

        for (index, node) in proxy_nodes.iter().enumerate() {
            if index == final_index {
                if node.kind == TransportKind::HttpsProxy {
                    stream = self.wrap_https_proxy_stream(stream, node).await?;
                }
                stream.metadata_mut().hops.push(node.name.clone());
                stream.metadata_mut().transport_kind = Some(node.kind.clone());
                return Ok(stream);
            }

            if node.kind == TransportKind::HttpsProxy {
                stream = self.wrap_https_proxy_stream(stream, node).await?;
            }

            let next = &proxy_nodes[index + 1];
            let endpoint = next.endpoint.as_deref().ok_or_else(|| {
                ProxyError::Config(format!("node '{}' has no endpoint", next.name))
            })?;
            let target = Destination::from_authority(endpoint, ApplicationProtocol::Tcp)?;
            self.connect_proxy_hop(&mut stream, node, &target).await?;
            stream.metadata_mut().hops.push(node.name.clone());
            stream.metadata_mut().transport_kind = Some(node.kind.clone());
        }

        Err(ProxyError::RouteUnavailable(format!(
            "chain '{}' did not produce an http proxy stream",
            chain.name
        )))
    }

    /// Opens the TCP connection to the first proxy node in a chain.
    ///
    /// # Parameters
    ///
    /// - `first`: First non-direct proxy node.
    /// - `chain`: Chain used for route metadata.
    ///
    /// # Returns
    ///
    /// A proxy stream connected to `first`.
    ///
    /// # Errors
    ///
    /// Returns an error when the node has no endpoint, the endpoint is invalid,
    /// or the TCP connection fails.
    async fn connect_first_proxy(
        &self,
        first: &ProxyNode,
        chain: &ProxyChain,
    ) -> Result<ProxyStream> {
        let endpoint = first
            .endpoint
            .as_deref()
            .ok_or_else(|| ProxyError::Config(format!("node '{}' has no endpoint", first.name)))?;
        let first_destination = Destination::from_authority(endpoint, ApplicationProtocol::Tcp)?;
        let first_addr = self.resolve_destination_addr(&first_destination).await?;
        let tcp = time::timeout(
            self.config.as_config().timeouts.connect_duration(),
            TcpStream::connect(first_addr),
        )
        .await
        .map_err(|_| ProxyError::Timeout("connect to first proxy"))??;

        Ok(ProxyStream::new(
            tcp,
            ConnectMetadata {
                route_name: Some(chain.name.clone()),
                transport_kind: Some(first.kind.clone()),
                hops: Vec::new(),
            },
        ))
    }

    /// Wraps a proxy stream in client TLS for an HTTPS proxy hop.
    ///
    /// # Parameters
    ///
    /// - `stream`: Existing stream connected to the HTTPS proxy endpoint.
    /// - `node`: HTTPS proxy node whose endpoint supplies the TLS server name.
    ///
    /// # Returns
    ///
    /// A `ProxyStream` containing the TLS-protected stream.
    ///
    /// # Errors
    ///
    /// Returns an error when the endpoint is invalid or TLS setup fails.
    async fn wrap_https_proxy_stream(
        &self,
        stream: ProxyStream,
        node: &ProxyNode,
    ) -> Result<ProxyStream> {
        let endpoint = node
            .endpoint
            .as_deref()
            .ok_or_else(|| ProxyError::Config(format!("node '{}' has no endpoint", node.name)))?;
        let server_name = endpoint_host(endpoint)?;
        let metadata = stream.metadata().clone();
        let tls_stream = tls::client_tls(
            stream,
            server_name,
            self.config.as_config().timeouts.handshake_duration(),
            node.verify_tls,
        )
        .await?;
        Ok(ProxyStream::new(tls_stream, metadata))
    }

    /// Performs the protocol handshake for one proxy hop toward a target.
    ///
    /// # Parameters
    ///
    /// - `stream`: Stream already connected to `node`.
    /// - `node`: Proxy node whose protocol should be used for the handshake.
    /// - `target`: Destination requested through `node`.
    ///
    /// # Errors
    ///
    /// Returns an error when the proxy protocol rejects the target or the
    /// transport kind is unsupported.
    async fn connect_proxy_hop(
        &self,
        stream: &mut ProxyStream,
        node: &ProxyNode,
        target: &Destination,
    ) -> Result<()> {
        let target = self.target_for_proxy_node(node, target).await?;
        match node.kind {
            TransportKind::HttpProxy | TransportKind::HttpsProxy => {
                let credentials = node.credentials()?;
                http::connect_tunnel(
                    stream,
                    &target,
                    self.config.as_config().timeouts.handshake_duration(),
                    credentials.as_ref(),
                )
                .await
            }
            TransportKind::Socks5 | TransportKind::TorSocks => {
                let credentials = node.credentials()?;
                socks::connect_tunnel(
                    stream,
                    &target,
                    self.config.as_config().timeouts.handshake_duration(),
                    credentials.as_ref(),
                )
                .await
            }
            TransportKind::Socks4 => {
                socks::connect_tunnel_v4(
                    stream,
                    &target,
                    self.config.as_config().timeouts.handshake_duration(),
                    node.username.as_deref(),
                )
                .await
            }
            TransportKind::ExternalTunnel => {
                Err(ProxyError::Unsupported("external tunnel transport"))
            }
            TransportKind::ProxyPool => Err(ProxyError::Unsupported(
                "unresolved proxy pool marker in transport chain",
            )),
            TransportKind::MasqueUdp => {
                Err(ProxyError::Unsupported("masque connect-udp transport"))
            }
            TransportKind::Direct => Ok(()),
        }
    }

    /// Resolves the relay endpoint returned by a SOCKS5 UDP association.
    ///
    /// # Parameters
    ///
    /// - `node`: SOCKS5 node that returned the relay endpoint.
    /// - `relay_endpoint`: Relay endpoint returned by the upstream handshake.
    ///
    /// # Returns
    ///
    /// A concrete socket address usable for UDP relay packets.
    ///
    /// # Errors
    ///
    /// Returns an error when the endpoint is invalid or DNS resolution fails.
    async fn resolve_udp_relay_addr(
        &self,
        node: &ProxyNode,
        relay_endpoint: &str,
    ) -> Result<SocketAddr> {
        let mut relay = Destination::from_authority(relay_endpoint, ApplicationProtocol::Udp)?;
        let relay_is_unspecified = match relay.address {
            crate::core::DestinationAddress::Ip(ip) => ip.is_unspecified(),
            crate::core::DestinationAddress::Host(_) => false,
        };
        if relay_is_unspecified {
            let endpoint = node.endpoint.as_deref().ok_or_else(|| {
                ProxyError::Config(format!("node '{}' has no endpoint", node.name))
            })?;
            let proxy_host = endpoint_host(endpoint)?;
            relay.address = proxy_host
                .parse::<IpAddr>()
                .map(crate::core::DestinationAddress::Ip)
                .unwrap_or_else(|_| crate::core::DestinationAddress::Host(proxy_host.to_string()));
        }

        match relay.address {
            crate::core::DestinationAddress::Ip(ip) => Ok(SocketAddr::new(ip, relay.port)),
            crate::core::DestinationAddress::Host(_) => self.resolve_destination_addr(&relay).await,
        }
    }

    /// Resolves a destination to a concrete socket address.
    ///
    /// # Parameters
    ///
    /// - `destination`: Destination whose hostname may need DNS resolution.
    ///
    /// # Returns
    ///
    /// Socket address with the destination port attached.
    ///
    /// # Errors
    ///
    /// Returns an error when DNS resolution fails or produces no usable address.
    async fn resolve_destination_addr(&self, destination: &Destination) -> Result<SocketAddr> {
        match &destination.address {
            DestinationAddress::Ip(ip) => Ok(SocketAddr::new(*ip, destination.port)),
            DestinationAddress::Host(host) => {
                let ip = self.resolve_host_ip(host).await?;
                Ok(SocketAddr::new(ip, destination.port))
            }
        }
    }

    /// Resolves a hostname destination into an IP destination.
    ///
    /// # Parameters
    ///
    /// - `destination`: Destination that may contain a hostname.
    ///
    /// # Returns
    ///
    /// Destination with a literal IP address when resolution was required.
    ///
    /// # Errors
    ///
    /// Returns an error when DNS resolution fails.
    async fn resolve_destination_to_ip(&self, destination: &Destination) -> Result<Destination> {
        let socket_addr = self.resolve_destination_addr(destination).await?;
        Ok(Destination {
            address: DestinationAddress::Ip(socket_addr.ip()),
            port: socket_addr.port(),
            protocol: destination.protocol.clone(),
        })
    }

    /// Resolves a hostname destination into an IPv4 destination.
    ///
    /// SOCKS4 cannot carry IPv6 addresses, so local DNS mode must select an
    /// IPv4 address before the protocol handshake.
    ///
    /// # Parameters
    ///
    /// - `destination`: Destination that may contain a hostname.
    ///
    /// # Returns
    ///
    /// Destination with a literal IPv4 address when resolution was required.
    ///
    /// # Errors
    ///
    /// Returns an error when DNS resolution fails or no IPv4 address is
    /// available.
    async fn resolve_destination_to_ipv4(&self, destination: &Destination) -> Result<Destination> {
        let ip = match &destination.address {
            DestinationAddress::Ip(IpAddr::V4(ip)) => IpAddr::V4(*ip),
            DestinationAddress::Ip(IpAddr::V6(_)) => {
                return Err(ProxyError::InvalidDestination(
                    "socks4 cannot encode ipv6 destinations".to_string(),
                ));
            }
            DestinationAddress::Host(host) => self.resolve_host_ipv4(host).await?,
        };
        Ok(Destination {
            address: DestinationAddress::Ip(ip),
            port: destination.port,
            protocol: destination.protocol.clone(),
        })
    }

    /// Resolves a hostname through the optional DNS core or the system resolver.
    ///
    /// # Parameters
    ///
    /// - `host`: Hostname or textual IP address to resolve.
    ///
    /// # Returns
    ///
    /// First resolved IP address.
    ///
    /// # Errors
    ///
    /// Returns an error when resolution fails or returns no addresses.
    async fn resolve_host_ip(&self, host: &str) -> Result<IpAddr> {
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(ip);
        }

        #[cfg(feature = "arachnea-dns")]
        if let Some(dns_core) = &self.dns_core {
            return dns_core
                .resolve_ip(host)
                .await
                .map_err(|error| ProxyError::Dns(error.to_string()))?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    ProxyError::Dns(format!("hostname '{host}' resolved to no address"))
                });
        }

        lookup_host((host, 0))
            .await
            .map_err(dns_io_error)?
            .next()
            .map(|addr| addr.ip())
            .ok_or_else(|| ProxyError::Dns(format!("hostname '{host}' resolved to no address")))
    }

    /// Resolves a hostname into an IPv4 address.
    ///
    /// # Parameters
    ///
    /// - `host`: Hostname or textual IP address to resolve.
    ///
    /// # Returns
    ///
    /// First resolved IPv4 address.
    ///
    /// # Errors
    ///
    /// Returns an error when resolution fails or returns no IPv4 address.
    async fn resolve_host_ipv4(&self, host: &str) -> Result<IpAddr> {
        if let Ok(ip) = host.parse::<IpAddr>() {
            return match ip {
                IpAddr::V4(_) => Ok(ip),
                IpAddr::V6(_) => Err(ProxyError::Dns(format!(
                    "hostname '{host}' resolved to no ipv4 address"
                ))),
            };
        }

        #[cfg(feature = "arachnea-dns")]
        if let Some(dns_core) = &self.dns_core {
            return dns_core
                .resolve_ip(host)
                .await
                .map_err(|error| ProxyError::Dns(error.to_string()))?
                .into_iter()
                .find(|ip| ip.is_ipv4())
                .ok_or_else(|| {
                    ProxyError::Dns(format!("hostname '{host}' resolved to no ipv4 address"))
                });
        }

        lookup_host((host, 0))
            .await
            .map_err(dns_io_error)?
            .find(|addr| addr.ip().is_ipv4())
            .map(|addr| addr.ip())
            .ok_or_else(|| {
                ProxyError::Dns(format!("hostname '{host}' resolved to no ipv4 address"))
            })
    }

    /// Resolves a proxy handshake target when a node cannot receive hostnames.
    ///
    /// # Parameters
    ///
    /// - `node`: Proxy node that will receive the target.
    /// - `target`: Original destination for the proxy handshake.
    ///
    /// # Returns
    ///
    /// Original target when hostnames can be forwarded, otherwise an IP target.
    ///
    /// # Errors
    ///
    /// Returns an error when hostname resolution is required and fails.
    async fn target_for_proxy_node(
        &self,
        node: &ProxyNode,
        target: &Destination,
    ) -> Result<Destination> {
        match node.effective_name_resolution() {
            ProxyNameResolutionMode::Local if node.kind == TransportKind::Socks4 => {
                self.resolve_destination_to_ipv4(target).await
            }
            ProxyNameResolutionMode::Local => self.resolve_destination_to_ip(target).await,
            ProxyNameResolutionMode::ProxyOnly => Ok(target.clone()),
            ProxyNameResolutionMode::ProxyThenLocalFallback => {
                if self.socks5_node_uses_remembered_local_dns(node) {
                    self.resolve_destination_to_ip(target).await
                } else {
                    Ok(target.clone())
                }
            }
        }
    }

    /// Flags one SOCKS5 fallback node for local DNS resolution after a remote DNS failure.
    ///
    /// # Parameters
    ///
    /// - `chain`: Chain whose SOCKS5 nodes may be flagged.
    /// - `target`: Destination that failed through remote proxy resolution.
    /// - `error`: Error returned by the previous chain attempt.
    ///
    /// # Returns
    ///
    /// `true` when a node was flagged and the caller should retry the chain.
    async fn flag_socks5_local_dns_fallback(
        &self,
        chain: &ProxyChain,
        target: &Destination,
        error: &ProxyError,
    ) -> bool {
        if !is_socks5_remote_dns_failure(error)
            || !matches!(&target.address, DestinationAddress::Host(_))
            || self.resolve_destination_to_ip(target).await.is_err()
        {
            return false;
        }

        let Some(node) = chain.nodes.iter().find(|node| {
            matches!(node.kind, TransportKind::Socks5 | TransportKind::TorSocks)
                && node.effective_name_resolution()
                    == ProxyNameResolutionMode::ProxyThenLocalFallback
                && !self.socks5_node_uses_remembered_local_dns(node)
        }) else {
            return false;
        };

        self.remember_socks5_local_dns_node(node);
        tracing::debug!(
            proxy_node = %node.name,
            "remembering socks5 node for local dns resolution after remote dns failure"
        );
        true
    }

    /// Returns whether a SOCKS5 fallback node is remembered as local-DNS-only.
    ///
    /// # Parameters
    ///
    /// - `node`: Proxy node to inspect.
    ///
    /// # Returns
    ///
    /// `true` when previous fallback handling marked this node.
    fn socks5_node_uses_remembered_local_dns(&self, node: &ProxyNode) -> bool {
        self.socks5_local_dns_nodes
            .read()
            .map(|nodes| nodes.contains(&node.name))
            .unwrap_or(false)
    }

    /// Remembers a SOCKS5 fallback node as requiring local DNS resolution.
    ///
    /// # Parameters
    ///
    /// - `node`: Proxy node to flag by stable name.
    fn remember_socks5_local_dns_node(&self, node: &ProxyNode) {
        if let Ok(mut nodes) = self.socks5_local_dns_nodes.write() {
            nodes.insert(node.name.clone());
        }
    }
}

/// Returns whether a SOCKS5 error likely means remote hostname resolution failed.
///
/// # Parameters
///
/// - `error`: Proxy error returned by a SOCKS5 handshake.
///
/// # Returns
///
/// `true` for SOCKS5 host-unreachable or general-failure replies.
fn is_socks5_remote_dns_failure(error: &ProxyError) -> bool {
    let ProxyError::UpstreamRejected(message) = error else {
        return false;
    };
    message.contains("socks5 reply code 4") || message.contains("socks5 reply code 1")
}

/// Maps system resolver failures into the proxy DNS error category.
///
/// # Parameters
///
/// - `error`: I/O error returned by the system resolver.
///
/// # Returns
///
/// Proxy DNS error with resolver context.
fn dns_io_error(error: io::Error) -> ProxyError {
    ProxyError::Dns(error.to_string())
}

/// Extracts the host portion from a `host:port` endpoint.
///
/// # Parameters
///
/// - `endpoint`: Endpoint string that may contain a bracketed IPv6 address.
///
/// # Returns
///
/// Host value without IPv6 brackets.
///
/// # Errors
///
/// Returns an error when the endpoint is not a valid authority.
fn endpoint_host(endpoint: &str) -> Result<&str> {
    if let Some(rest) = endpoint.strip_prefix('[') {
        let end = rest
            .find(']')
            .ok_or_else(|| ProxyError::InvalidDestination(endpoint.to_string()))?;
        return Ok(&rest[..end]);
    }
    let (host, _) = endpoint
        .rsplit_once(':')
        .ok_or_else(|| ProxyError::InvalidDestination(endpoint.to_string()))?;
    Ok(host)
}

/// Loads credentials for the final HTTP proxy in a chain.
///
/// # Parameters
///
/// - `chain`: Chain selected for an HTTP request.
///
/// # Returns
///
/// Optional credentials for the last HTTP or HTTPS proxy hop.
///
/// # Errors
///
/// Returns an error when credentials are configured but cannot be loaded.
fn selected_http_proxy_credentials(
    chain: &ProxyChain,
) -> Result<Option<crate::core::ProxyNodeCredentials>> {
    chain
        .nodes
        .iter()
        .rev()
        .find(|node| {
            matches!(
                node.kind,
                TransportKind::HttpProxy | TransportKind::HttpsProxy
            )
        })
        .map(ProxyNode::credentials)
        .unwrap_or(Ok(None))
}

/// Returns the target and check mode used to test a proxy pool marker.
///
/// A pool followed by another concrete proxy must support tunneling to that
/// next proxy endpoint. A final pool serving plain HTTP may select an HTTP
/// proxy that is reachable but does not allow `CONNECT` to port 80, because the
/// actual request will be relayed in absolute-form.
///
/// # Parameters
///
/// - `chain`: Chain containing the proxy pool marker.
/// - `pool_index`: Index of the pool marker inside `chain.nodes`.
/// - `destination`: Original destination requested by the caller.
///
/// # Returns
///
/// Pair containing the probe target and required check mode.
///
/// # Errors
///
/// Returns an error when a later concrete proxy hop has no endpoint or an
/// invalid endpoint.
fn proxy_pool_check_target(
    chain: &ProxyChain,
    pool_index: usize,
    destination: &Destination,
) -> Result<(Destination, ProxyPoolCheckMode)> {
    for node in chain.nodes.iter().skip(pool_index + 1) {
        if node.kind == TransportKind::Direct {
            continue;
        }
        if node.kind == TransportKind::ProxyPool {
            continue;
        }
        let endpoint = node
            .endpoint
            .as_deref()
            .ok_or_else(|| ProxyError::Config(format!("node '{}' has no endpoint", node.name)))?;
        return Ok((
            Destination::from_authority(endpoint, ApplicationProtocol::Tcp)?,
            ProxyPoolCheckMode::Tunnel,
        ));
    }

    let check_mode = if destination.protocol == ApplicationProtocol::Http {
        ProxyPoolCheckMode::HttpForward
    } else {
        ProxyPoolCheckMode::Tunnel
    };
    Ok((destination.clone(), check_mode))
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
