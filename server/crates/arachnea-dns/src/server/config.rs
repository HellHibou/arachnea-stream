use crate::core::{
    BlockAction, BlockRule, CoreConfig, DnssecState, DomainPattern, LocalRecord, Record,
    RecordData, RecordType, Upstream, UsageProfile,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::Path};

/// Listener and network safety settings for the DNS server.
/// 
/// This struct contains configuration for the DNS server's network
/// listeners, including bind addresses and access control settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Whether server listeners should be started.
    /// 
    /// If false, the server will not start any listeners and will not
    /// accept DNS queries. This can be useful for testing or when using
    /// the resolver in library mode.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// UDP listen address.
    /// 
    /// The socket address (IP:port) on which the DNS server will listen
    /// for UDP DNS queries. UDP is the primary transport for DNS.
    #[serde(default = "default_listen")]
    pub listen_udp: SocketAddr,
    /// TCP listen address.
    /// 
    /// The socket address (IP:port) on which the DNS server will listen
    /// for TCP DNS queries. TCP is used for large responses or when
    /// UDP is blocked.
    #[serde(default = "default_listen")]
    pub listen_tcp: SocketAddr,
    /// Network ACL settings.
    /// 
    /// Access control configuration that determines which clients are
    /// allowed to query the DNS server and which features they can use.
    #[serde(default)]
    pub network: NetworkConfig,
}

impl Default for ServerConfig {
    /// Returns loopback DNS server defaults.
    fn default() -> Self {
        Self {
            enabled: true,
            listen_udp: default_listen(),
            listen_tcp: default_listen(),
            network: NetworkConfig::default(),
        }
    }
}

/// CIDR lists used to control access to a network-exposed server.
/// 
/// This struct defines network-based access control lists that determine
/// which clients can query the DNS server and which features they can use.
/// Access control is based on IP address ranges specified in CIDR notation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Clients allowed to use the server.
    /// 
    /// List of CIDR networks that are permitted to query this DNS server.
    /// If empty, the default behavior depends on the listen address:
    /// loopback addresses allow loopback clients, other addresses are
    /// more restrictive.
    #[serde(default)]
    pub allow_clients: Vec<String>,
    /// Clients denied even if broader allow rules match.
    /// 
    /// List of CIDR networks that are explicitly denied access, even if
    /// they would otherwise be allowed by allow_clients rules. Deny
    /// rules take precedence over allow rules.
    #[serde(default)]
    pub deny_clients: Vec<String>,
    /// Clients allowed to use recursive behavior.
    /// 
    /// List of CIDR networks that are permitted to use recursive DNS
    /// resolution. This is a subset of allow_clients that have permission
    /// for more resource-intensive recursive queries.
    #[serde(default)]
    pub allow_recursion: Vec<String>,
}

/// TOML file configuration accepted by the server crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConfig {
    /// High-level profile name.
    #[serde(default)]
    pub profile: FileProfile,
    /// Server listener settings.
    #[serde(default)]
    pub server: ServerConfig,
    /// Optional core resolver override.
    #[serde(default)]
    pub resolver: Option<crate::core::ResolverConfig>,
    /// Compact single-forwarder section.
    #[serde(default)]
    pub forwarder: Option<FileForwarder>,
    /// Explicit upstream list.
    #[serde(default)]
    pub upstreams: Vec<crate::core::Upstream>,
    /// Optional cache override.
    #[serde(default)]
    pub cache: Option<crate::core::CacheConfig>,
    /// Optional privacy override.
    #[serde(default)]
    pub privacy: Option<crate::core::PrivacyConfig>,
    /// Optional security override.
    #[serde(default)]
    pub security: Option<crate::core::SecurityConfig>,
    /// Local record entries in user-friendly string form.
    #[serde(default)]
    pub local_records: Vec<FileLocalRecord>,
    /// Simple block rules.
    #[serde(default)]
    pub blocklists: Vec<FileBlocklist>,
}

impl FileConfig {
    /// Loads a TOML server config file.
    ///
    /// # Parameters
    ///
    /// - `path`: Path to the TOML file.
    ///
    /// # Returns
    ///
    /// Parsed file configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or parsed.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let raw = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("cannot read {}", path.as_ref().display()))?;
        toml::from_str(&raw).context("invalid TOML configuration")
    }

    /// Converts the file format into a validated core configuration.
    pub fn into_core_config(self) -> Result<CoreConfig> {
        let profile = self.profile.clone();
        let explicit_upstreams = self.upstreams;
        let forwarder_upstream = self
            .forwarder
            .map(FileForwarder::into_upstream)
            .transpose()?;
        let profile_upstreams = match &forwarder_upstream {
            Some(upstream) => vec![upstream.clone()],
            None => explicit_upstreams.clone(),
        };
        let profile_dnssec = self
            .security
            .as_ref()
            .map(|security| security.dnssec)
            .unwrap_or(DnssecState::ReportOnly);
        let mut config = profile
            .clone()
            .into_usage_profile(profile_upstreams, profile_dnssec)?
            .config();

        if let Some(resolver) = self.resolver {
            config.resolver = resolver;
        }
        if !explicit_upstreams.is_empty() {
            config.upstreams = explicit_upstreams;
        }
        if let Some(upstream) = forwarder_upstream {
            config.resolver.mode = crate::core::ResolutionMode::Forwarder;
            config.upstreams = vec![upstream];
        }
        if let Some(cache) = self.cache {
            config.cache = cache;
        }
        if let Some(privacy) = self.privacy {
            config.privacy = privacy;
        }
        if let Some(security) = self.security {
            config.security = security;
        }
        config.local_records = self
            .local_records
            .into_iter()
            .map(FileLocalRecord::try_into)
            .collect::<Result<_>>()?;
        config.blocklists = self
            .blocklists
            .into_iter()
            .map(FileBlocklist::into_block_rule)
            .collect();

        Ok(config)
    }
}

/// Compact forwarder section used by the sample configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileForwarder {
    /// Transport used to reach the upstream.
    pub transport: crate::core::Transport,
    /// URL or socket endpoint, depending on transport.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Host used by TLS/QUIC transports.
    #[serde(default)]
    pub host: Option<String>,
    /// Port override.
    #[serde(default)]
    pub port: Option<u16>,
    /// TLS verification setting for encrypted transports.
    #[serde(default)]
    pub verify_tls: Option<bool>,
    /// Request timeout in milliseconds.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Optional upstream name.
    #[serde(default)]
    pub name: Option<String>,
}

impl FileForwarder {
    /// Converts a TOML forwarder entry into a core upstream.
    ///
    /// # Returns
    ///
    /// Core upstream equivalent to this file entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the transport/endpoint shape is invalid.
    fn into_upstream(self) -> Result<crate::core::Upstream> {
        let endpoint = match self.transport {
            crate::core::Transport::Udp | crate::core::Transport::Tcp => {
                let socket = match (self.endpoint, self.host, self.port) {
                    (Some(endpoint), _, _) => endpoint.parse()?,
                    (None, Some(host), Some(port)) => format!("{host}:{port}").parse()?,
                    _ => anyhow::bail!("UDP/TCP forwarder requires endpoint or host + port"),
                };
                crate::core::UpstreamEndpoint::Socket(socket)
            }
            crate::core::Transport::Doh => {
                let Some(endpoint) = self.endpoint else {
                    anyhow::bail!("DoH forwarder requires endpoint URL");
                };
                crate::core::UpstreamEndpoint::Url(endpoint)
            }
            crate::core::Transport::Dot
            | crate::core::Transport::Doq
            | crate::core::Transport::Odoh => {
                let Some(host) = self.host else {
                    anyhow::bail!("{:?} forwarder requires host", self.transport);
                };
                crate::core::UpstreamEndpoint::HostPort {
                    host,
                    port: self.port.unwrap_or(match self.transport {
                        crate::core::Transport::Dot => 853,
                        _ => 443,
                    }),
                }
            }
            crate::core::Transport::System => crate::core::UpstreamEndpoint::System,
            crate::core::Transport::Recursive => {
                anyhow::bail!("recursive is a resolver mode, not a single forwarder transport");
            }
        };

        Ok(crate::core::Upstream {
            name: self.name.unwrap_or_else(|| "single-forwarder".to_owned()),
            transport: self.transport,
            endpoint,
            timeout_ms: self.timeout_ms,
            verify_tls: self.verify_tls.unwrap_or(true),
        })
    }
}

/// Profile names accepted in TOML files.
/// 
/// This enum defines the high-level usage profiles that can be specified
/// in TOML configuration files. These profiles provide convenient presets
/// for common DNS server configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileProfile {
    /// SystemRelay,
    /// 
    /// Configure the server to use the operating system's DNS resolver.
    /// This is useful for testing or when you want to delegate to the
    /// system's DNS configuration.
    SystemRelay,
    /// SingleForwarder,
    /// 
    /// Configure the server to forward all queries to a single upstream
    /// DNS server. This provides simple forwarding behavior.
    SingleForwarder,
    /// Resilience,
    /// 
    /// Configure the server for high availability with multiple upstream
    /// servers and failover behavior. This profile ensures that queries
    /// can be resolved even if some upstreams are unavailable.
    Resilience,
    /// Privacy,
    /// 
    /// Configure the server to prioritize privacy by using encrypted
    /// transports and disabling features that might leak client information.
    /// This profile is suitable for privacy-conscious users.
    Privacy,
    /// Secure,
    /// 
    /// Configure the server to emphasize security and data integrity.
    /// This profile enables DNSSEC validation and uses secure upstream
    /// transports to protect against DNS spoofing and manipulation.
    Secure,
    /// Advanced,
    /// 
    /// Configure the server with no automatic presets, requiring all
    /// settings to be specified explicitly. This profile gives full control
    /// to the administrator for custom configurations.
    Advanced,
}

impl Default for FileProfile {
    /// Returns the system relay file profile.
    fn default() -> Self {
        Self::SystemRelay
    }
}

impl FileProfile {
    /// Converts a file profile into a core usage profile.
    ///
    /// # Parameters
    ///
    /// - `upstreams`: Upstreams declared by the file sections for this profile.
    /// - `dnssec_state`: DNSSEC state to embed in secure profiles.
    ///
    /// # Returns
    ///
    /// Core usage profile equivalent to this file profile.
    ///
    /// # Errors
    ///
    /// Returns an error when `single_forwarder` does not have exactly one upstream.
    fn into_usage_profile(
        self,
        upstreams: Vec<Upstream>,
        dnssec_state: DnssecState,
    ) -> Result<UsageProfile> {
        match self {
            Self::SystemRelay => Ok(UsageProfile::SystemRelay),
            Self::SingleForwarder => {
                if upstreams.len() != 1 {
                    anyhow::bail!(
                        "profile `single_forwarder` requires exactly one [forwarder] or [[upstreams]] entry"
                    );
                }
                Ok(UsageProfile::SingleForwarder(
                    upstreams.into_iter().next().expect("length checked above"),
                ))
            }
            Self::Resilience => Ok(UsageProfile::Resilience(upstreams)),
            Self::Privacy => Ok(UsageProfile::Privacy(upstreams)),
            Self::Secure => Ok(UsageProfile::Secure(upstreams, dnssec_state)),
            Self::Advanced => Ok(UsageProfile::Advanced),
        }
    }
}

/// Local record entry as written in TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileLocalRecord {
    /// Owner name.
    pub name: String,
    /// Records in compact textual form, such as `A 192.0.2.1 60`.
    pub records: Vec<String>,
    /// Whether the name should be treated as a wildcard suffix.
    #[serde(default)]
    pub wildcard: bool,
}

impl TryFrom<FileLocalRecord> for LocalRecord {
    /// Error type used when converting file local records.
    type Error = anyhow::Error;

    /// Converts a file local record into a core local record.
    ///
    /// # Parameters
    ///
    /// - `value`: File local record parsed from TOML.
    ///
    /// # Returns
    ///
    /// Core local record.
    ///
    /// # Errors
    ///
    /// Returns an error when any record string cannot be parsed.
    fn try_from(value: FileLocalRecord) -> Result<Self> {
        let records = value
            .records
            .iter()
            .map(|raw| parse_record(&value.name, raw))
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            name: value.name,
            records,
            wildcard: value.wildcard,
            priority_over_upstream: true,
        })
    }
}

/// File-backed block rule in the initial simple format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileBlocklist {
    /// Block rule name.
    pub name: String,
    /// Domain pattern to block.
    pub source: String,
    /// Block behavior.
    #[serde(default = "default_block_action")]
    pub action: BlockAction,
}

impl FileBlocklist {
    /// Converts a file blocklist entry into a core block rule.
    ///
    /// # Returns
    ///
    /// Core block rule equivalent to this file entry.
    fn into_block_rule(self) -> BlockRule {
        BlockRule {
            name: self.name,
            pattern: DomainPattern::new(&self.source),
            action: self.action,
            enabled: true,
        }
    }
}

/// Parses one textual local record.
///
/// # Parameters
///
/// - `default_name`: Domain name used when the record omits an explicit name.
/// - `raw`: Record text containing type and address data.
///
/// # Returns
///
/// Parsed core record.
///
/// # Errors
///
/// Returns an error when the record format, type, or address is invalid.
fn parse_record(default_name: &str, raw: &str) -> Result<Record> {
    let parts = raw.split_whitespace().collect::<Vec<_>>();
    anyhow::ensure!(parts.len() >= 2, "invalid record `{raw}`");

    let record_type = parts[0]
        .parse::<RecordType>()
        .with_context(|| format!("unsupported record type in `{raw}`"))?;

    let (data_index, ttl) = if parts.len() >= 3
        && parts
            .last()
            .and_then(|part| part.parse::<u32>().ok())
            .is_some()
    {
        (1, parts.last().unwrap().parse::<u32>()?)
    } else {
        (1, 60)
    };

    let data = match record_type {
        RecordType::A => RecordData::A {
            address: parts[data_index].parse()?,
        },
        RecordType::AAAA => RecordData::AAAA {
            address: parts[data_index].parse()?,
        },
        RecordType::CNAME => RecordData::CNAME {
            name: parts[data_index].to_owned(),
        },
        RecordType::MX => {
            anyhow::ensure!(
                parts.len() >= data_index + 2,
                "MX requires preference and exchange"
            );
            RecordData::MX {
                preference: parts[data_index].parse()?,
                exchange: parts[data_index + 1].to_owned(),
            }
        }
        RecordType::TXT => {
            let data_end = if parts
                .last()
                .and_then(|part| part.parse::<u32>().ok())
                .is_some()
            {
                parts.len().saturating_sub(1)
            } else {
                parts.len()
            };
            RecordData::TXT {
                value: parts[data_index..data_end].join(" "),
            }
        }
        RecordType::SRV => {
            anyhow::ensure!(
                parts.len() >= data_index + 4,
                "SRV requires priority weight port target"
            );
            RecordData::SRV {
                priority: parts[data_index].parse()?,
                weight: parts[data_index + 1].parse()?,
                port: parts[data_index + 2].parse()?,
                target: parts[data_index + 3].to_owned(),
            }
        }
    };

    Ok(Record {
        name: default_name.to_owned(),
        ttl,
        data,
    })
}

/// Returns the default enabled flag for serde.
///
/// # Returns
///
/// Always returns `true`.
fn default_enabled() -> bool {
    true
}

/// Returns the default DNS listen address.
///
/// # Returns
///
/// Loopback UDP/TCP DNS listen address.
fn default_listen() -> SocketAddr {
    "127.0.0.1:5353"
        .parse()
        .expect("valid default listen address")
}

/// Returns the default block action for blocklist entries.
///
/// # Returns
///
/// NXDOMAIN block action, which responds to blocked domains with
/// a "domain does not exist" error.
fn default_block_action() -> BlockAction {
    BlockAction::Nxdomain
}
