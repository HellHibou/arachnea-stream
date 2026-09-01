use std::time::SystemTime;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::core::{ProxyNode, Result, TransportKind};

/// Proxy protocol variant advertised or detected for a record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ProxyProtocol {
    /// Cleartext HTTP forward proxy.
    Http,
    /// HTTP proxy over TLS.
    Https,
    /// SOCKS4 (TCP CONNECT with IPv4 address, no hostname).
    Socks4,
    /// SOCKS4a (TCP CONNECT with hostname resolved by the proxy).
    Socks4a,
    /// SOCKS5 (TCP CONNECT with optional hostname forwarding).
    Socks5,
}

/// Runtime health status determined by the last probe.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyRuntimeStatus {
    /// Not yet probed.
    Unknown,
    /// The proxy responded correctly to the last probe.
    Ok,
    /// The proxy failed the last probe.
    Ko,
    /// The proxy requires authentication that the dynamic pool cannot provide.
    AuthenticationRequired,
}

impl Default for ProxyRuntimeStatus {
    fn default() -> Self {
        Self::Unknown
    }
}

/// Optional availability hint advertised by the proxy source.
///
/// This hint must never override the runtime [`ProxyRuntimeStatus`]. It may
/// only be used as a tie-breaker between two proxies that are already
/// functionally equivalent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyAvailabilityHint {
    /// No availability information from the source.
    Unknown,
    /// Low availability according to the source.
    Low,
    /// Medium availability according to the source.
    Medium,
    /// High availability according to the source.
    High,
}

impl Default for ProxyAvailabilityHint {
    fn default() -> Self {
        Self::Unknown
    }
}

/// Reason why a proxy failed for a specific destination.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyDestinationFailureReason {
    /// The remote proxy refused the TCP connection.
    ConnectRefused,
    /// TLS handshake through the proxy failed.
    TlsFailed,
    /// The destination blocked or blacklisted this proxy.
    BlockedByOrigin,
    /// The connection timed out.
    Timeout,
    /// A non-classified failure.
    Other,
}

/// Per-destination failure recorded against a proxy that is otherwise healthy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProxyDestinationFailure {
    /// URI scheme used for the failed request (e.g. `https`).
    pub scheme: String,
    /// Destination host that rejected or failed.
    pub host: String,
    /// Destination port.
    pub port: u16,
    /// Classification of the failure.
    pub reason: ProxyDestinationFailureReason,
    /// Consecutive failure count for this origin.
    #[serde(default)]
    pub failure_count: u32,
    /// When this origin last failed.
    #[serde(with = "system_time_serde")]
    pub last_failed: SystemTime,
    /// Optional cooldown expiry before retrying this origin.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "option_system_time_serde"
    )]
    pub cooldown_until: Option<SystemTime>,
}

/// Canonical proxy record used for loading, probing, storing and selecting
/// dynamic proxies.
///
/// `ProxyRecord` is the single mutable representation throughout the dynamic
/// proxy lifecycle. It is converted into a [`ProxyNode`] only at the last
/// moment, after normalisation, protocol resolution and selection.
///
/// `host` accepts an IPv4 address, an IPv6 address, or a hostname. IPv6
/// addresses are automatically bracketed during conversion to [`ProxyNode`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProxyRecord {
    /// Proxy protocol when known.
    ///
    /// When `None`, the probe may attempt protocol detection and fill this
    /// field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<ProxyProtocol>,
    /// Proxy host — an IPv4, IPv6, or hostname string.
    pub host: String,
    /// Proxy port.
    pub port: u16,
    /// Optional ISO country code associated with this proxy.
    ///
    /// When present, this value is trusted and not re-verified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// Whether the proxy supports HTTPS tunnelling.
    ///
    /// `Some(true)` means the source or a probe confirmed HTTPS support.
    /// `Some(false)` means the proxy must not be selected for HTTPS
    /// destinations. `None` means untested or unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_https: Option<bool>,
    /// Runtime health status determined by the last probe.
    #[serde(default)]
    pub status: ProxyRuntimeStatus,
    /// Measured latency in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    /// Consecutive global failure count.
    #[serde(default)]
    pub failure_count: u32,
    /// Whether the proxy requires authentication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authentication_required: Option<bool>,
    /// Availability hint advertised by the source.
    #[serde(default)]
    pub availability: ProxyAvailabilityHint,
    /// Per-destination failures for an otherwise healthy proxy.
    #[serde(default)]
    pub destination_failures: Vec<ProxyDestinationFailure>,
    /// Timestamp of the last probe.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "option_system_time_serde"
    )]
    pub last_checked: Option<SystemTime>,
    /// Cooldown expiry before retrying a globally-failed proxy.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "option_system_time_serde"
    )]
    pub cooldown_until: Option<SystemTime>,
}

/// Native persisted identity of a proxy endpoint.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ProxyKey {
    /// Proxy host, preserved exactly as recorded.
    pub host: String,
    /// Proxy TCP port.
    pub port: u16,
}

impl ProxyRecord {
    /// Returns the native endpoint key used by persistent stores.
    pub fn key(&self) -> ProxyKey {
        ProxyKey {
            host: self.host.clone(),
            port: self.port,
        }
    }

    /// Returns the endpoint authority with IPv6 bracketing applied.
    ///
    /// # Returns
    ///
    /// Authority string formatted as `host:port` or `[ipv6]:port`.
    pub fn authority(&self) -> String {
        format_authority(&self.host, self.port)
    }

    /// Attempts to convert this record into a [`ProxyNode`].
    ///
    /// Returns `None` when `protocol` is absent because a concrete transport
    /// kind cannot be determined without it.
    ///
    /// # Returns
    ///
    /// Proxy node when a protocol is available.
    pub fn try_to_node(&self) -> Option<ProxyNode> {
        let kind = TransportKind::from_proxy_protocol(self.protocol.as_ref()?)?;
        Some(self.to_node_with_kind(kind))
    }

    /// Converts this record into a [`ProxyNode`] using an explicit transport
    /// kind. This is useful when a probe resolved the protocol externally.
    ///
    /// # Parameters
    ///
    /// - `kind`: Concrete transport kind to use.
    ///
    /// # Returns
    ///
    /// Proxy node configured from this record.
    pub fn into_node_with_kind(&self, kind: TransportKind) -> ProxyNode {
        self.to_node_with_kind(kind)
    }

    /// Creates a node name from the record metadata.
    fn node_name(&self) -> String {
        let protocol_tag = self
            .protocol
            .as_ref()
            .map(|p| format!("{p}-"))
            .unwrap_or_default();
        format!("dynamic-{}{}:{}", protocol_tag, self.host, self.port)
    }

    fn to_node_with_kind(&self, kind: TransportKind) -> ProxyNode {
        let endpoint = self.authority();
        let send_hostname = !matches!(kind, TransportKind::Socks4);
        ProxyNode {
            name: self.node_name(),
            kind,
            endpoint: Some(endpoint),
            send_hostname,
            dns_resolution: None,
            username: None,
            password_env: None,
            verify_tls: true,
        }
    }
}

impl TransportKind {
    /// Maps a [`ProxyProtocol`] to the corresponding [`TransportKind`].
    ///
    /// # Parameters
    ///
    /// - `protocol`: Proxy protocol variant.
    ///
    /// # Returns
    ///
    /// Corresponding transport kind, or `None` when the protocol is not
    /// recognised.
    fn from_proxy_protocol(protocol: &ProxyProtocol) -> Option<Self> {
        match protocol {
            ProxyProtocol::Http => Some(Self::HttpProxy),
            ProxyProtocol::Https => Some(Self::HttpsProxy),
            ProxyProtocol::Socks4 => Some(Self::Socks4),
            ProxyProtocol::Socks4a => Some(Self::Socks4),
            ProxyProtocol::Socks5 => Some(Self::Socks5),
        }
    }
}

/// Formats a host and port into an authority string, bracketing IPv6 addresses.
///
/// # Parameters
///
/// - `host`: Hostname, IPv4, or IPv6 address.
/// - `port`: Port number.
///
/// # Returns
///
/// Authority string formatted as `host:port` or `[ipv6]:port`.
fn format_authority(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

impl std::fmt::Display for ProxyProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http => write!(f, "http"),
            Self::Https => write!(f, "https"),
            Self::Socks4 => write!(f, "socks4"),
            Self::Socks4a => write!(f, "socks4a"),
            Self::Socks5 => write!(f, "socks5"),
        }
    }
}

/// Parameters for requesting a dynamic proxy load.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProxyLoadRequest {
    /// Optional country code to filter by.
    pub country: Option<String>,
}

/// Provider that loads dynamic proxy records from an external source.
///
/// Implementations are typically provided by `arachnea-scrapyfy` and wired
/// into the proxy inventory at startup.
#[async_trait]
pub trait ProxyDataProvider: Send + Sync {
    /// Load proxy records matching the given request parameters.
    ///
    /// # Parameters
    ///
    /// - `request`: Load parameters such as an optional country filter.
    ///
    /// # Returns
    ///
    /// A list of proxy records loaded from the external source.
    async fn load_proxies(&self, request: ProxyLoadRequest) -> Result<Vec<ProxyRecord>>;
}

// ---------------------------------------------------------------------------
// Custom serde helpers for `SystemTime`
// ---------------------------------------------------------------------------

/// Serialises a `SystemTime` as epoch millis (u64).
mod system_time_serde {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let millis = time
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64;
        serializer.serialize_u64(millis)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + Duration::from_millis(millis))
    }
}

/// Serialises an `Option<SystemTime>` as epoch millis or null.
mod option_system_time_serde {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(time: &Option<SystemTime>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match time {
            Some(t) => {
                let millis = t
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO)
                    .as_millis() as u64;
                serializer.serialize_some(&millis)
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<SystemTime>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<u64>::deserialize(deserializer)?;
        Ok(opt.map(|millis| UNIX_EPOCH + Duration::from_millis(millis)))
    }
}
