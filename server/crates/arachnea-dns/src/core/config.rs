use crate::core::{DnsError, DomainPattern, LocalRecord, Result};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf};

const EMBEDDED_ROOT_HINTS: &str = include_str!("../../resources/root_hints.txt");

/// Root configuration object for the reusable DNS core.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreConfig {
    /// High-level profile used to seed defaults.
    pub profile: UsageProfile,
    /// Resolver behavior and timeout settings.
    pub resolver: ResolverConfig,
    /// Outbound DNS resolvers used in forwarder mode.
    pub upstreams: Vec<Upstream>,
    /// Per-core in-memory cache configuration.
    pub cache: CacheConfig,
    /// Privacy-related options.
    pub privacy: PrivacyConfig,
    /// Security-related options.
    pub security: SecurityConfig,
    /// Locally answered records.
    pub local_records: Vec<LocalRecord>,
    /// Domain block rules.
    pub blocklists: Vec<crate::core::BlockRule>,
    /// Smart DNS routing rules.
    pub smart_dns: crate::core::SmartDnsConfig,
    /// Advanced ordered policy rules.
    pub rules: Vec<crate::core::Rule>,
    /// Proxy targets used by Smart DNS rules.
    pub proxy_targets: Vec<crate::core::ProxyTarget>,
}

impl CoreConfig {
    /// Validates and normalizes the final configuration.
    ///
    /// # Returns
    ///
    /// Effective configuration with derived values populated.
    ///
    /// # Errors
    ///
    /// Returns an error when validation fails or root hints cannot be resolved.
    pub fn resolve_effective_config(&self) -> Result<ResolvedConfig> {
        validate_features(self)?;
        validate_config(self)?;

        let mut effective = self.clone();
        effective.resolver.root_hints = resolve_root_hints(&effective.resolver)?;
        effective.local_records = effective
            .local_records
            .into_iter()
            .map(|mut record| {
                record.name = crate::core::normalize_name(&record.name);
                record
            })
            .collect();

        Ok(ResolvedConfig { config: effective })
    }
}

/// Fully expanded configuration after profile defaults and validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedConfig {
    /// Concrete configuration used by a running core instance.
    pub config: CoreConfig,
}

/// High-level usage profiles that generate explicit configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageProfile {
    /// Use the operating system resolver.
    SystemRelay,

    /// Forward every query to one configured upstream.
    ///
    /// # Parameters
    ///
    /// - `Upstream`: Upstream that receives every forwarded query.
    SingleForwarder(Upstream),

    /// Prefer availability and fallback behavior.
    ///
    /// # Parameters
    ///
    /// - `Vec<Upstream>`: Upstreams used by the failover strategy.
    Resilience(Vec<Upstream>),

    /// Prefer encrypted/private upstream behavior.
    ///
    /// # Parameters
    ///
    /// - `Vec<Upstream>`: Private upstreams queried by the forwarder.
    Privacy(Vec<Upstream>),

    /// Prefer integrity and strict fallbacks.
    ///
    /// # Parameters
    ///
    /// - `Vec<Upstream>`: Secure upstreams queried by the forwarder.
    /// - `DnssecState`: DNSSEC validation mode applied to the profile.
    Secure(Vec<Upstream>, DnssecState),

    /// Require explicit settings.
    Advanced,
}

impl UsageProfile {
    /// Builds a complete starting configuration from this usage profile.
    ///
    /// # Returns
    ///
    /// Core configuration initialized for the profile defaults.
    pub fn config(self) -> CoreConfig {
        let mut config = CoreConfig {
            profile: self,
            resolver: ResolverConfig::default(),
            upstreams: Vec::new(),
            cache: CacheConfig::default(),
            privacy: PrivacyConfig::default(),
            security: SecurityConfig::default(),
            local_records: Vec::new(),
            blocklists: Vec::new(),
            smart_dns: crate::core::SmartDnsConfig::default(),
            rules: Vec::new(),
            proxy_targets: Vec::new(),
        };

        match &config.profile {
            UsageProfile::SystemRelay => config.resolver.mode = ResolutionMode::Stub,
            UsageProfile::SingleForwarder(upstream) => {
                config.resolver.mode = ResolutionMode::Forwarder;
                config.upstreams.push(upstream.clone());
            }
            UsageProfile::Resilience(upstreams) => {
                config.resolver.mode = ResolutionMode::Recursive;
                config.resolver.upstream_strategy = UpstreamStrategy::Failover;
                config.resolver.nxdomain_strategy =
                    crate::core::NxdomainStrategy::FallbackOnNxdomain;
                config.upstreams = upstreams.clone();
            }
            UsageProfile::Privacy(upstreams) => {
                config.resolver.mode = ResolutionMode::Forwarder;
                config.privacy.ecs = EcsPolicy::Disabled;
                config.privacy.log_qnames = false;
                config.privacy.query_padding = true;
                config.resolver.allow_insecure_fallback = false;
                config.upstreams = upstreams.clone();
            }
            UsageProfile::Secure(upstreams, dnssec_state) => {
                config.resolver.mode = ResolutionMode::Forwarder;
                config.resolver.allow_insecure_fallback = false;
                config.security.dnssec = dnssec_state.clone();
                config.upstreams = upstreams.clone();
            }
            UsageProfile::Advanced => {}
        }

        config
    }
}

/// Resolver mode, retries and upstream strategy settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolverConfig {
    /// Resolution strategy.
    pub mode: ResolutionMode,
    /// Request timeout in milliseconds.
    pub timeout_ms: u64,
    /// Number of retry attempts.
    pub retries: u8,
    /// Whether upstreams may be queried in parallel.
    pub parallel_upstreams: bool,
    /// Whether insecure fallbacks are allowed.
    pub allow_insecure_fallback: bool,
    /// Upstream selection strategy.
    pub upstream_strategy: UpstreamStrategy,
    /// NXDOMAIN fallback strategy.
    pub nxdomain_strategy: crate::core::NxdomainStrategy,
    /// Root DNS server IP addresses used by recursive mode.
    #[serde(default)]
    pub root_hints: Vec<String>,
    /// Optional external root hints file, one IP address per line.
    #[serde(default)]
    pub root_hints_file: Option<PathBuf>,
}

impl Default for ResolverConfig {
    /// Returns resolver defaults using UDP and standard timeouts.
    fn default() -> Self {
        Self {
            mode: ResolutionMode::Stub,
            timeout_ms: 2_000,
            retries: 2,
            parallel_upstreams: false,
            allow_insecure_fallback: false,
            upstream_strategy: UpstreamStrategy::FixedOrder,
            nxdomain_strategy: crate::core::NxdomainStrategy::TrustFirst,
            root_hints: Vec::new(),
            root_hints_file: None,
        }
    }
}

/// High-level resolution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionMode {
    /// Use the system resolver from application code.
    Stub,
    /// Forward to configured upstreams.
    Forwarder,
    /// Recursive mode placeholder for v1 architecture.
    Recursive,
}

/// Upstream selection algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamStrategy {
    FixedOrder,
    RoundRobin,
    Race,
    Failover,
    Consensus,
}

/// Outbound DNS server description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Upstream {
    /// Human-readable upstream name.
    pub name: String,
    /// Transport protocol.
    pub transport: Transport,
    /// Network endpoint.
    pub endpoint: UpstreamEndpoint,
    /// Optional upstream-specific timeout.
    pub timeout_ms: Option<u64>,
    /// Whether TLS verification is required for encrypted transports.
    pub verify_tls: bool,
}

impl Upstream {
    /// Creates a system relay upstream.
    ///
    /// # Parameters
    ///
    /// - `timeout_ms`: Request timeout in milliseconds, or `None` for the default timeout.
    ///
    /// # Returns
    ///
    /// System relay upstream declaration.
    pub fn system(timeout_ms: Option<u64>) -> Self {
        Self {
            name: "system".to_owned(),
            transport: Transport::System,
            endpoint: UpstreamEndpoint::System,
            timeout_ms,
            verify_tls: true,
        }
    }

    /// Creates a classic UDP upstream.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable upstream name.
    /// - `socket_addr`: UDP socket address of the upstream resolver.
    ///
    /// # Returns
    ///
    /// UDP upstream declaration.
    pub fn udp(name: impl Into<String>, socket_addr: SocketAddr) -> Self {
        Self {
            name: name.into(),
            transport: Transport::Udp,
            endpoint: UpstreamEndpoint::Socket(socket_addr),
            timeout_ms: None,
            verify_tls: true,
        }
    }

    /// Creates a DNS-over-HTTPS upstream.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable upstream name.
    /// - `url`: HTTPS endpoint URL of the upstream resolver.
    /// - `timeout_ms`: Request timeout in milliseconds, or `None` for the default timeout.
    ///
    /// # Returns
    ///
    /// DNS-over-HTTPS upstream declaration.
    pub fn doh(name: impl Into<String>, url: impl Into<String>, timeout_ms: Option<u64>) -> Self {
        Self {
            name: name.into(),
            transport: Transport::Doh,
            endpoint: UpstreamEndpoint::Url(url.into()),
            timeout_ms,
            verify_tls: true,
        }
    }

    /// Creates a classic DoQ upstream.
    ///
    /// # Parameters
    ///
    /// - `name`: Stable upstream name.
    /// - `host`: DNS server hostname.
    /// - `port`: DNS server port.
    /// - `timeout_ms`: Request timeout in milliseconds, or `None` for the default timeout.
    ///
    /// # Returns
    ///
    /// DoQ upstream declaration.
    pub fn doq(
        name: impl Into<String>,
        host: impl Into<String>,
        port: u16,
        timeout_ms: Option<u64>,
    ) -> Self {
        Self {
            name: name.into(),
            transport: Transport::Doq,
            endpoint: UpstreamEndpoint::HostPort {
                host: host.into(),
                port,
            },
            timeout_ms,
            verify_tls: true,
        }
    }
}

/// DNS transport protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    System,
    Udp,
    Tcp,
    Dot,
    Doh,
    Doq,
    Odoh,
    Recursive,
}

/// Endpoint details for an upstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum UpstreamEndpoint {
    System,
    Socket(SocketAddr),
    Url(String),
    HostPort { host: String, port: u16 },
}

/// In-memory DNS cache settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub enabled: bool,
    pub max_entries: usize,
    pub min_ttl_seconds: u32,
    pub max_ttl_seconds: u32,
    pub negative_cache: bool,
    #[serde(default)]
    pub aggressive_dnssec_negative_cache: bool,
    pub serve_stale: bool,
    pub max_stale_seconds: u32,
    pub stale_response_ttl_seconds: u32,
}

impl Default for CacheConfig {
    /// Returns cache defaults suitable for normal resolver use.
    fn default() -> Self {
        Self {
            enabled: false,
            max_entries: 100_000,
            min_ttl_seconds: 0,
            max_ttl_seconds: 86_400,
            negative_cache: true,
            aggressive_dnssec_negative_cache: false,
            serve_stale: false,
            max_stale_seconds: 86_400,
            stale_response_ttl_seconds: 30,
        }
    }
}

/// Privacy controls applied to resolution and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    pub qname_minimisation: bool,
    pub ecs: EcsPolicy,
    pub odoh: bool,
    pub query_padding: bool,
    pub log_qnames: bool,
}

impl Default for PrivacyConfig {
    /// Returns privacy defaults without ECS forwarding.
    fn default() -> Self {
        Self {
            qname_minimisation: false,
            ecs: EcsPolicy::Disabled,
            odoh: false,
            query_padding: false,
            log_qnames: false,
        }
    }
}

/// EDNS Client Subnet policy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EcsPolicy {
    Disabled,
    AnonymizedPrefix,
    Passthrough,
    CustomPrefix(String),
}

/// Security controls for DNSSEC, cookies and rate limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub dnssec: DnssecState,
    pub dns_cookies: bool,
    pub rate_limit_per_client_per_second: Option<u32>,
}

impl Default for SecurityConfig {
    /// Returns security defaults with DNSSEC disabled.
    fn default() -> Self {
        Self {
            dnssec: DnssecState::Off,
            dns_cookies: false,
            rate_limit_per_client_per_second: Some(50),
        }
    }
}

/// DNSSEC validation mode exposed in metadata and configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnssecState {
    Off,
    Passthrough,
    ReportOnly,
    Opportunistic,
    Strict,
}

/// Builder for constructing a core instance from profiles or full config.
#[derive(Default)]
pub struct CoreConfigBuilder {
    profile: Option<UsageProfile>,
    overrides: Option<CoreConfig>,
}

impl CoreConfigBuilder {
    /// Sets the profile used when no full config override is provided.
    ///
    /// # Parameters
    ///
    /// - `profile`: Usage profile to use.
    ///
    /// # Returns
    ///
    /// Updated builder.
    pub fn profile(mut self, profile: UsageProfile) -> Self {
        self.profile = Some(profile);
        self
    }

    /// Uses a complete configuration instead of profile defaults.
    ///
    /// # Parameters
    ///
    /// - `config`: Complete core configuration override.
    ///
    /// # Returns
    ///
    /// Updated builder.
    pub fn config(mut self, config: CoreConfig) -> Self {
        self.overrides = Some(config);
        self
    }

    /// Builds and validates an `ArachneaDnsCore`.
    pub fn build(self) -> Result<crate::core::ArachneaDnsCore> {
        let config = self
            .overrides
            .unwrap_or_else(|| self.profile.unwrap_or(UsageProfile::SystemRelay).config());
        crate::core::ArachneaDnsCore::new(config)
    }
}

/// Validates cross-field constraints for a DNS core configuration.
///
/// # Parameters
///
/// - `config`: DNS core configuration to validate.
///
/// # Errors
///
/// Returns an error when cache, upstream, local record, or blocklist settings
/// are invalid.
fn validate_config(config: &CoreConfig) -> Result<()> {
    if config.cache.enabled && config.cache.max_entries == 0 {
        return Err(DnsError::InvalidConfig(
            "cache.max_entries must be greater than zero".to_owned(),
        ));
    }

    if matches!(config.resolver.mode, ResolutionMode::Forwarder) && config.upstreams.is_empty() {
        return Err(DnsError::InvalidConfig(
            "forwarder mode requires at least one upstream".to_owned(),
        ));
    }

    if matches!(config.resolver.mode, ResolutionMode::Recursive) && !cfg!(feature = "recursive") {
        return Err(DnsError::InvalidConfig(
            "recursive mode requires the `recursive` feature".to_owned(),
        ));
    }

    if config.security.dnssec != DnssecState::Off && !cfg!(feature = "dnssec") {
        return Err(DnsError::InvalidConfig(
            "DNSSEC validation requires the `dnssec` feature".to_owned(),
        ));
    }

    for local in &config.local_records {
        let _ = DomainPattern::new(&local.name);
    }

    Ok(())
}

/// Validates that declared transports are enabled by compiled features.
///
/// # Parameters
///
/// - `config`: DNS core configuration whose upstreams are checked.
///
/// # Errors
///
/// Returns an error when a configured transport requires a disabled feature.
fn validate_features(config: &CoreConfig) -> Result<()> {
    for upstream in &config.upstreams {
        match upstream.transport {
            Transport::Dot if !cfg!(feature = "dot") => {
                return Err(DnsError::InvalidConfig(
                    "DoT upstream requires the `dot` feature".to_owned(),
                ));
            }
            Transport::Doh if !cfg!(feature = "doh") => {
                return Err(DnsError::InvalidConfig(
                    "DoH upstream requires the `doh` feature".to_owned(),
                ));
            }
            Transport::Doq if !cfg!(feature = "doq") => {
                return Err(DnsError::InvalidConfig(
                    "DoQ upstream requires the `doq` feature".to_owned(),
                ));
            }
            Transport::Odoh if !cfg!(feature = "odoh") => {
                return Err(DnsError::InvalidConfig(
                    "ODoH upstream requires the `odoh` feature".to_owned(),
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Resolves root hints from a file override or embedded defaults.
///
/// # Parameters
///
/// - `resolver`: Resolver configuration containing root hint settings.
///
/// # Returns
///
/// Root hint lines used by recursive resolution.
///
/// # Errors
///
/// Returns an error when the configured root hints file cannot be read or
/// parsed.
fn resolve_root_hints(resolver: &ResolverConfig) -> Result<Vec<String>> {
    let raw = if !resolver.root_hints.is_empty() {
        resolver.root_hints.join("\n")
    } else if let Some(path) = &resolver.root_hints_file {
        std::fs::read_to_string(path).map_err(|error| {
            DnsError::InvalidConfig(format!(
                "cannot read root_hints_file {}: {error}",
                path.display()
            ))
        })?
    } else {
        EMBEDDED_ROOT_HINTS.to_owned()
    };

    let hints = parse_root_hints(&raw)?;
    if hints.is_empty() {
        return Err(DnsError::InvalidConfig(
            "root hints must contain at least one IP address".to_owned(),
        ));
    }
    Ok(hints)
}

/// Parses root hint text into non-empty entries.
///
/// # Parameters
///
/// - `raw`: Root hint file contents.
///
/// # Returns
///
/// Non-comment root hint lines.
///
/// # Errors
///
/// Returns an error when no usable root hints are present.
fn parse_root_hints(raw: &str) -> Result<Vec<String>> {
    raw.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let value = line.split_once('#').map_or(line, |(value, _)| value).trim();
            (!value.is_empty()).then_some((index + 1, value))
        })
        .map(|(line, value)| {
            value.parse::<std::net::IpAddr>().map_err(|error| {
                DnsError::InvalidConfig(format!(
                    "invalid root hint IP address on line {line}: {value}: {error}"
                ))
            })?;
            Ok(value.to_owned())
        })
        .collect()
}
