use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::core::{ProxyNameResolutionMode, ProxyNode, Result, TransportKind};

/// Maximum age of a proxy validation before a subsequent failure removes it.
///
/// Stale proxies remain selectable: this is a conditional eviction threshold,
/// not a hard expiration deadline.
pub const PROXY_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Proxy protocol variant advertised or detected for a record.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
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

/// Preferred protocol order for source selection and runtime detection.
///
/// HTTP is intentionally last because it provides the weakest transport
/// capabilities among the supported proxy protocols.
pub const PROXY_PROTOCOL_PRIORITY: [ProxyProtocol; 5] = [
    ProxyProtocol::Socks5,
    ProxyProtocol::Socks4,
    ProxyProtocol::Https,
    ProxyProtocol::Socks4a,
    ProxyProtocol::Http,
];

/// One source-level declaration attached to a dynamic proxy endpoint.
///
/// Declarations preserve provider claims independently from the protocol and
/// capabilities selected by runtime probing.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProxyDeclaration {
    /// Stable source identifier supplied by the data provider.
    pub source: String,
    /// Protocol advertised by this source, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<ProxyProtocol>,
    /// HTTPS support hint advertised by this source, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_https: Option<bool>,
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

/// Runtime validation state of one proxy capability.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyCapabilityStatus {
    /// The capability has not been tested or the result was inconclusive.
    #[default]
    Unknown,
    /// The capability was validated successfully.
    Available,
    /// The capability was tested and failed.
    Unavailable,
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
    /// Positive health facts; legacy attempt timestamps are not migrated into evidence.
    #[serde(default)]
    pub health_evidence: super::ProxyHealthEvidence,
    /// Latest contextual observations, bounded to 64 contexts per endpoint.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observations: Vec<super::ProxyObservation>,
    /// Persisted attempt times in epoch milliseconds, keyed by tcp/http/https.
    /// Missing entries mean that phase has never been attempted.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub probe_attempts: std::collections::HashMap<String, u64>,
    /// Last TCP precheck result, independent of proxy protocol support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tcp_reachable: Option<bool>,
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
    /// Whether the source advertises HTTPS support.
    ///
    /// Runtime validation is stored separately in [`Self::https_tunnel`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_https: Option<bool>,
    /// Source declarations merged for this endpoint.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declarations: Vec<ProxyDeclaration>,
    /// Runtime health status determined by the last probe.
    #[serde(default)]
    pub status: ProxyRuntimeStatus,
    /// Runtime ability to reach an HTTP destination through the proxy.
    #[serde(default)]
    pub http_forwarding: ProxyCapabilityStatus,
    /// Runtime ability to open a tunnel to the configured HTTPS destination.
    #[serde(default)]
    pub https_tunnel: ProxyCapabilityStatus,
    /// Runtime result of destination HTTPS validation through the tunnel.
    ///
    /// Availability requires a valid TLS handshake and certificate, independently
    /// of the HTTP status returned afterwards.
    #[serde(default)]
    pub destination_tls: ProxyCapabilityStatus,
    /// Runtime validity of the TLS certificate presented by an HTTPS proxy.
    #[serde(default)]
    pub proxy_tls_certificate: ProxyCapabilityStatus,
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
    /// Timestamp of the last successful probe or non-rejected origin response.
    ///
    /// Records older than [`PROXY_CACHE_TTL`] remain usable, but are deleted if
    /// a directly observed endpoint connection fails; destination failures alone
    /// never trigger global eviction.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "option_system_time_serde"
    )]
    pub last_validated_at: Option<SystemTime>,
    /// Cooldown expiry before retrying a globally-failed proxy.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "option_system_time_serde"
    )]
    pub cooldown_until: Option<SystemTime>,
}

/// Native persisted identity of a proxy endpoint.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ProxyKey {
    /// Proxy host, preserved exactly as recorded.
    pub host: String,
    /// Proxy TCP port.
    pub port: u16,
}

impl ProxyRecord {
    /// Records a completed phase without erasing an earlier positive fact.
    ///
    /// # Arguments
    /// - `observation`: Completed, contextualized observation without secrets.
    pub fn record_observation(&mut self, mut observation: super::ProxyObservation) {
        if let Some(index) = self
            .observations
            .iter()
            .position(|existing| existing.context == observation.context)
        {
            let mut existing = self.observations.remove(index);
            observation.last_succeeded_at = observation
                .last_succeeded_at
                .max(existing.last_succeeded_at);
            if existing.completed_at > observation.completed_at {
                existing.last_succeeded_at = existing
                    .last_succeeded_at
                    .max(observation.last_succeeded_at);
                if observation.outcome == super::ProxyObservationOutcome::Succeeded {
                    existing.last_succeeded_at = existing
                        .last_succeeded_at
                        .max(Some(observation.completed_at));
                    self.health_evidence.last_positive_at = self
                        .health_evidence
                        .last_positive_at
                        .max(Some(observation.completed_at));
                }
                self.observations.push(existing);
                return;
            }
        }
        if observation.outcome == super::ProxyObservationOutcome::Succeeded {
            observation.last_succeeded_at = observation
                .last_succeeded_at
                .max(Some(observation.completed_at));
            self.health_evidence.last_positive_at = self
                .health_evidence
                .last_positive_at
                .max(Some(observation.completed_at));
        }
        self.observations.push(observation);
        self.observations.sort_by_key(|item| item.completed_at);
        if self.observations.len() > 64 {
            self.observations.remove(0);
        }
    }

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
        let protocol = self.protocol.as_ref()?;
        let kind = TransportKind::from_proxy_protocol(protocol)?;
        let mut node = self.to_node_with_kind(kind);
        match protocol {
            ProxyProtocol::Socks4 => {
                node = node.with_dns_resolution(ProxyNameResolutionMode::Local);
            }
            ProxyProtocol::Socks4a => {
                node = node.with_dns_resolution(ProxyNameResolutionMode::ProxyOnly);
            }
            ProxyProtocol::Socks5 => {
                node = node.with_dns_resolution(ProxyNameResolutionMode::ProxyThenLocalFallback);
            }
            ProxyProtocol::Http | ProxyProtocol::Https => {}
        }
        Some(node)
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
    /// Allowed ISO country codes to load; their order has no priority. An empty list does not constrain the
    /// provider.
    pub countries: Vec<String>,
}

/// Consumer of incrementally discovered proxy batches. Publishing is awaited,
/// and dropping the load cancels source work without leaving a producer task.
#[async_trait]
pub trait ProxyBatchSink: Send + Sync {
    /// Publishes source records without claiming any runtime validation.
    ///
    /// # Arguments
    /// - `records`: A partial batch; later sources may add declarations for the same endpoint.
    ///
    /// # Errors
    /// Returns an error when the consumer can no longer accept the batch.
    async fn publish(&self, records: Vec<ProxyRecord>) -> Result<()>;
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

    /// Publishes batches as they become available. Legacy providers publish one batch.
    ///
    /// # Arguments
    /// - `request`: Allowed countries for this load.
    /// - `sink`: Awaited consumer sharing the lifetime of the load.
    ///
    /// # Errors
    /// Returns provider or consumer errors; already published batches remain useful.
    async fn load_proxies_progressively(
        &self,
        request: ProxyLoadRequest,
        sink: std::sync::Arc<dyn ProxyBatchSink>,
    ) -> Result<()> {
        sink.publish(self.load_proxies(request).await?).await
    }
}

// ---------------------------------------------------------------------------
// Custom serde helpers for `SystemTime`
// ---------------------------------------------------------------------------

/// Serialises a `SystemTime` as epoch millis (u64).
pub(crate) mod system_time_serde {
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
pub(crate) mod option_system_time_serde {
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
