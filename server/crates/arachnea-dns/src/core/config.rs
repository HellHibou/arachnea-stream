use crate::core::{DnsError, DomainPattern, LocalRecord, Result};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf};

/// Embedded default root DNS server hints.
///
/// This constant contains the default list of root DNS server IP addresses
/// that are used for recursive DNS resolution when no custom root hints
/// are provided. The data is embedded in the binary at compile time.
use crate::DNS_EMBEDDED_ROOT_HINTS as EMBEDDED_ROOT_HINTS;

/// Root configuration object for the reusable DNS core.
///
/// This struct contains the complete configuration for the DNS resolver core,
/// including resolver behavior, upstream servers, caching, privacy and
/// security settings, and various policy rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreConfig {
    /// High-level profile used to seed defaults.
    ///
    /// The usage profile that provides default values for other configuration
    /// fields. Profiles like "Privacy" or "Secure" set appropriate defaults
    /// for privacy and security settings.
    pub profile: UsageProfile,
    /// Resolver behavior and timeout settings.
    ///
    /// Configuration for the DNS resolver, including resolution mode,
    /// timeouts, retry behavior, and upstream selection strategy.
    pub resolver: ResolverConfig,
    /// Outbound DNS resolvers used in forwarder mode.
    ///
    /// List of upstream DNS servers to which queries are forwarded when
    /// operating in forwarder mode.
    pub upstreams: Vec<Upstream>,
    /// Per-core in-memory cache configuration.
    ///
    /// Configuration for the in-memory DNS cache, including size limits,
    /// TTL bounds, and negative caching behavior.
    pub cache: CacheConfig,
    /// Privacy-related options.
    ///
    /// Privacy settings including QNAME minimization, ECS (EDNS Client
    /// Subnet) handling, and query logging preferences.
    pub privacy: PrivacyConfig,
    /// Security-related options.
    ///
    /// Security settings including DNSSEC validation mode, DNS cookies,
    /// and rate limiting configuration.
    pub security: SecurityConfig,
    /// Locally answered records.
    ///
    /// Domain names and their associated resource records that are
    /// answered locally without querying upstream servers.
    pub local_records: Vec<LocalRecord>,
    /// Domain block rules.
    ///
    /// Rules that block or modify responses for specific domain patterns,
    /// used for content filtering and access control.
    pub blocklists: Vec<crate::core::BlockRule>,
    /// Smart DNS routing rules.
    ///
    /// Rules that may answer queries directly or return proxy-target fallback
    /// records based on domain patterns. Upstream route actions are parsed but
    /// not applied yet.
    pub smart_dns: crate::core::SmartDnsConfig,
    /// Advanced ordered policy rules.
    ///
    /// Ordered list of policy rules that are evaluated in sequence to
    /// determine how to handle DNS queries. Rules can allow, block, or answer
    /// queries; rewrite and route actions are parsed but not applied yet.
    pub rules: Vec<crate::core::Rule>,
    /// Proxy targets used by Smart DNS rules.
    ///
    /// Named proxy targets that provide fallback records. Dynamic proxy
    /// selection and load balancing are not implemented.
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
///
/// These profiles provide convenient presets for common DNS resolver
/// configurations, setting appropriate defaults for privacy, security,
/// and upstream selection behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageProfile {
    /// Use the operating system resolver.
    ///
    /// This profile configures the resolver to use the system's built-in
    /// DNS resolution capabilities, typically through /etc/resolv.conf
    /// on Unix-like systems.
    SystemRelay,

    /// Forward every query to one configured upstream.
    ///
    /// The wrapped upstream receives every forwarded query, providing simple
    /// forwarding behavior.
    SingleForwarder(Upstream),

    /// Prefer availability and fallback behavior.
    ///
    /// The wrapped upstreams are used by the failover strategy, which the
    /// current forwarder applies by trying them in declaration order.
    Resilience(Vec<Upstream>),

    /// Prefer encrypted/private upstream behavior.
    ///
    /// The wrapped upstreams are queried by the forwarder and privacy-oriented
    /// defaults are set. The profile does not itself enforce encrypted
    /// transports.
    Privacy(Vec<Upstream>),

    /// Prefer integrity and strict fallbacks.
    ///
    /// The wrapped upstreams are queried by the forwarder and the supplied
    /// DNSSEC state is stored. Only the `Strict` state enables validation in
    /// the current implementation.
    Secure(Vec<Upstream>, DnssecState),

    /// Require explicit settings.
    ///
    /// This profile provides no automatic configuration and requires
    /// all settings to be specified explicitly, giving full control
    /// to the administrator.
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
                config.security.dnssec = *dnssec_state;
                config.upstreams = upstreams.clone();
            }
            UsageProfile::Advanced => {}
        }

        config
    }
}

/// Resolver mode, retries and upstream strategy settings.
///
/// `retries`, `parallel_upstreams`, `allow_insecure_fallback` and
/// `upstream_strategy` are part of the configuration contract but are not yet
/// enforced by the forwarder, which tries upstreams in declaration order.
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
    /// Returns system-resolver defaults with the standard retry and timeout budgets.
    fn default() -> Self {
        Self {
            mode: crate::DNS_RESOLVER_DEFAULT_MODE,
            timeout_ms: crate::DNS_RESOLVER_DEFAULT_TIMEOUT_MS,
            retries: crate::DNS_RESOLVER_DEFAULT_RETRIES,
            parallel_upstreams: crate::DNS_RESOLVER_DEFAULT_PARALLEL_UPSTREAMS,
            allow_insecure_fallback: crate::DNS_RESOLVER_DEFAULT_ALLOW_INSECURE_FALLBACK,
            upstream_strategy: crate::DNS_RESOLVER_DEFAULT_UPSTREAM_STRATEGY,
            nxdomain_strategy: crate::DNS_RESOLVER_DEFAULT_NXDOMAIN_STRATEGY,
            root_hints: Vec::new(),
            root_hints_file: None,
        }
    }
}

/// High-level resolution mode.
///
/// This enum specifies the fundamental operating mode of the DNS resolver,
/// determining how DNS queries are processed and answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionMode {
    /// Use the system resolver from application code.
    ///
    /// In stub mode, the resolver delegates to the operating system's
    /// DNS resolution facilities rather than implementing its own
    /// DNS protocol handling.
    Stub,
    /// Forward to configured upstreams.
    ///
    /// In forwarder mode, the resolver sends DNS queries to configured
    /// upstream DNS servers and returns their responses to clients.
    Forwarder,
    /// Full recursive resolution from root hints.
    ///
    /// Requires the `recursive` feature. When upstreams are configured, the
    /// forwarder path is used instead of the recursive resolver.
    Recursive,
}

/// Upstream selection algorithm.
///
/// The current forwarder always tries upstreams in declaration order. The
/// other variants are accepted configuration values but are not implemented
/// as distinct selection algorithms yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamStrategy {
    /// FixedOrder,
    ///
    /// Queries are always sent to upstreams in the exact order they
    /// are configured. If the first upstream fails, the next one is tried,
    /// and so on.
    FixedOrder,
    /// RoundRobin,
    ///
    /// Upstreams are selected in a round-robin fashion, distributing
    /// queries evenly across all available upstreams.
    RoundRobin,
    /// Race,
    ///
    /// All upstreams are queried simultaneously, and the first response
    /// to arrive is used. This provides the fastest response time but
    /// uses more network resources.
    Race,
    /// Failover,
    ///
    /// Upstreams are tried in order until one succeeds. If an upstream
    /// fails or times out, the next one in the list is attempted.
    Failover,
    /// Consensus,
    ///
    /// Multiple upstreams are queried, and their responses are compared.
    /// Only answers that achieve consensus (agreement) among multiple
    /// upstreams are accepted.
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
    /// Declared TLS verification preference.
    ///
    /// The current transport clients use their library defaults and do not
    /// branch on this field.
    pub verify_tls: bool,
}

impl Upstream {
    /// Creates a system relay upstream.
    ///
    /// # Arguments
    ///
    /// - `timeout_ms`: Request timeout in milliseconds, or `None` for the default timeout.
    ///
    /// # Returns
    ///
    /// System relay upstream declaration.
    pub fn system(timeout_ms: Option<u64>) -> Self {
        Self {
            name: crate::DNS_SYSTEM_UPSTREAM_NAME.to_owned(),
            transport: Transport::System,
            endpoint: UpstreamEndpoint::System,
            timeout_ms,
            verify_tls: crate::DNS_UPSTREAM_DEFAULT_VERIFY_TLS,
        }
    }

    /// Creates a classic UDP upstream.
    ///
    /// # Arguments
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
            verify_tls: crate::DNS_UPSTREAM_DEFAULT_VERIFY_TLS,
        }
    }

    /// Creates a DNS-over-HTTPS upstream.
    ///
    /// # Arguments
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
            verify_tls: crate::DNS_UPSTREAM_DEFAULT_VERIFY_TLS,
        }
    }

    /// Creates a classic DoQ upstream.
    ///
    /// # Arguments
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
            verify_tls: crate::DNS_UPSTREAM_DEFAULT_VERIFY_TLS,
        }
    }
}

/// DNS transport protocol.
///
/// This enum specifies the network protocols used to communicate with
/// upstream DNS servers. Different protocols offer varying levels of
/// security, privacy, and performance characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// System,
    ///
    /// Use the operating system's built-in DNS resolution facilities.
    System,
    /// Udp,
    ///
    /// Classic DNS over UDP (port 53), the traditional DNS protocol.
    Udp,
    /// Tcp,
    ///
    /// Classic DNS over TCP (port 53), used for large responses or when
    /// UDP is blocked.
    Tcp,
    /// Dot,
    ///
    /// DNS over TLS (port 853), provides encrypted DNS queries using
    /// TLS for confidentiality and integrity.
    Dot,
    /// Doh,
    ///
    /// DNS over HTTPS (typically port 443), encapsulates DNS queries
    /// in HTTP/HTTPS for better compatibility with existing infrastructure.
    Doh,
    /// Doq,
    ///
    /// DNS over QUIC, uses the QUIC protocol (UDP-based) for encrypted
    /// DNS with reduced connection setup latency.
    Doq,
    /// Odoh,
    ///
    /// Declared Oblivious DNS over HTTPS transport. No ODoH implementation is
    /// wired yet; selecting it currently fails as an unsupported transport.
    Odoh,
    /// Recursive,
    ///
    /// Indicates that the resolver should perform recursive resolution
    /// starting from root servers rather than forwarding to upstreams.
    Recursive,
}

/// Endpoint details for an upstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum UpstreamEndpoint {
    /// Use the operating system resolver.
    System,
    /// A specific IP:port socket address.
    Socket(SocketAddr),
    /// A URL endpoint, used for DoH.
    Url(String),
    /// A hostname-and-port pair, used for DoT and DoQ; ODoH would use the same
    /// shape once implemented.
    HostPort {
        /// Server hostname.
        host: String,
        /// Server port.
        port: u16,
    },
}

/// In-memory DNS cache settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Whether caching is enabled.
    pub enabled: bool,
    /// Maximum number of entries in the cache.
    pub max_entries: usize,
    /// Minimum TTL in seconds for cached entries.
    pub min_ttl_seconds: u32,
    /// Maximum TTL in seconds for cached entries.
    pub max_ttl_seconds: u32,
    /// Whether negative responses (NXDOMAIN, NODATA) may be cached.
    pub negative_cache: bool,
    /// Whether DNSSEC NSEC proofs may synthesize negative answers.
    #[serde(default)]
    pub aggressive_dnssec_negative_cache: bool,
    /// Whether stale entries may be served when fresh data is unavailable.
    pub serve_stale: bool,
    /// Maximum time in seconds a stale entry is kept before eviction.
    pub max_stale_seconds: u32,
    /// TTL in seconds assigned to stale entries when served to clients.
    pub stale_response_ttl_seconds: u32,
}

impl Default for CacheConfig {
    /// Returns cache defaults suitable for normal resolver use.
    fn default() -> Self {
        Self {
            enabled: crate::DNS_CACHE_DEFAULT_ENABLED,
            max_entries: crate::DNS_CACHE_DEFAULT_MAX_ENTRIES,
            min_ttl_seconds: crate::DNS_CACHE_DEFAULT_MIN_TTL_SECONDS,
            max_ttl_seconds: crate::DNS_CACHE_DEFAULT_MAX_TTL_SECONDS,
            negative_cache: crate::DNS_CACHE_DEFAULT_NEGATIVE_CACHE,
            aggressive_dnssec_negative_cache: crate::DNS_CACHE_DEFAULT_AGGRESSIVE_NEGATIVE_CACHE,
            serve_stale: crate::DNS_CACHE_DEFAULT_SERVE_STALE,
            max_stale_seconds: crate::DNS_CACHE_DEFAULT_MAX_STALE_SECONDS,
            stale_response_ttl_seconds: crate::DNS_CACHE_DEFAULT_STALE_RESPONSE_TTL_SECONDS,
        }
    }
}

/// Privacy controls applied to resolution and metadata.
///
/// The current forwarder does not yet apply QNAME minimisation, ECS options,
/// ODoH, query padding, or QNAME log filtering. These fields are configuration
/// placeholders and are not enforced on the wire or in logs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    /// Whether QNAME minimisation is enabled.
    pub qname_minimisation: bool,
    /// EDNS Client Subnet policy.
    pub ecs: EcsPolicy,
    /// Whether Oblivious DNS over HTTPS is enabled.
    pub odoh: bool,
    /// Whether DNS query padding is enabled.
    pub query_padding: bool,
    /// Whether query names may be written to logs.
    pub log_qnames: bool,
}

impl Default for PrivacyConfig {
    /// Returns privacy defaults without ECS forwarding.
    fn default() -> Self {
        Self {
            qname_minimisation: crate::DNS_PRIVACY_DEFAULT_QNAME_MINIMISATION,
            ecs: crate::DNS_PRIVACY_DEFAULT_ECS,
            odoh: crate::DNS_PRIVACY_DEFAULT_ODOH,
            query_padding: crate::DNS_PRIVACY_DEFAULT_QUERY_PADDING,
            log_qnames: crate::DNS_PRIVACY_DEFAULT_LOG_QNAMES,
        }
    }
}

/// EDNS Client Subnet policy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EcsPolicy {
    /// ECS is disabled; no subnet information is sent.
    Disabled,
    /// Subnet prefix is anonymised before forwarding.
    AnonymizedPrefix,
    /// Subnet information is forwarded as received.
    Passthrough,
    /// A custom subnet prefix is used for all queries.
    CustomPrefix(String),
}

/// Security controls for DNSSEC, cookies and rate limits.
///
/// `dns_cookies` is not enforced yet. The optional per-client rate limit is
/// applied by the server listeners, not by the core resolver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// DNSSEC validation state.
    pub dnssec: DnssecState,
    /// Whether DNS cookies are enabled.
    pub dns_cookies: bool,
    /// Optional per-client rate limit per second.
    pub rate_limit_per_client_per_second: Option<u32>,
}

impl Default for SecurityConfig {
    /// Returns security defaults with DNSSEC disabled.
    fn default() -> Self {
        Self {
            dnssec: crate::DNS_SECURITY_DEFAULT_DNSSEC,
            dns_cookies: crate::DNS_SECURITY_DEFAULT_COOKIES,
            rate_limit_per_client_per_second: crate::DNS_SECURITY_DEFAULT_RATE_LIMIT,
        }
    }
}

/// DNSSEC validation mode exposed in metadata and configuration.
///
/// Only `Off` and `Strict` currently change resolution. `Strict` uses the
/// Hickory path for supported upstream transports; the intermediate modes are
/// copied into answer metadata but do not perform distinct validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnssecState {
    /// Off,
    ///
    /// DNSSEC validation is disabled. No cryptographic verification
    /// of DNS responses is performed.
    Off,
    /// Passthrough,
    ///
    /// Declared passthrough mode. Current resolution performs no validation
    /// and does not expose DNSSEC records through the public record model.
    Passthrough,
    /// ReportOnly,
    ///
    /// Declared report-only mode. Current resolution does not validate or
    /// report per-answer results; the state is only copied into metadata.
    ReportOnly,
    /// Opportunistic,
    ///
    /// Declared opportunistic mode. Current resolution does not perform
    /// opportunistic validation; the state is only copied into metadata.
    Opportunistic,
    /// Strict,
    ///
    /// Full DNSSEC validation is required through the Hickory path for UDP,
    /// TCP, DoT and DoQ upstreams. Other transports, or builds without the
    /// `dnssec` feature, fail instead of falling back silently.
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
    /// # Arguments
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
    /// # Arguments
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
    ///
    /// # Returns
    ///
    /// Core instance configured from the override or selected profile.
    ///
    /// # Errors
    ///
    /// Returns an error when the effective configuration fails validation or
    /// root hints cannot be resolved.
    pub fn build(self) -> Result<crate::core::ArachneaDnsCore> {
        let config = self
            .overrides
            .unwrap_or_else(|| self.profile.unwrap_or(UsageProfile::SystemRelay).config());
        crate::core::ArachneaDnsCore::new(config)
    }
}

/// Validates cross-field constraints for a DNS core configuration.
///
/// # Arguments
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
/// # Arguments
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
/// # Arguments
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
/// # Arguments
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

#[cfg(test)]
#[path = "../../tests/core/config.rs"]
mod tests;
