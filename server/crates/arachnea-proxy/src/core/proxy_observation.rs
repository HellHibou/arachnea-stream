//! Bounded, persistable facts about proxy and destination exchanges.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::ProxyProtocol;

/// Phase actually observed, rather than inferred from a later failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyObservationPhase {
    /// Direct connection to the proxy endpoint.
    EndpointConnect,
    /// Proxy protocol negotiation and authentication.
    ProxyProtocol,
    /// Validated TLS connection to an HTTPS proxy.
    ProxyTls,
    /// Tunnel negotiation for a particular destination.
    Tunnel,
    /// Validated TLS connection to the destination.
    DestinationTls,
    /// HTTP response received from the destination.
    DestinationHttp,
}

/// Attribution of a negative observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyFailureScope {
    /// Failure directly observed at the proxy endpoint.
    Proxy,
    /// Failure of a particular proxy protocol or its authentication.
    Protocol,
    /// Failure limited to the destination exchange; its cause may be ambiguous.
    Destination,
}

/// Result of a completed phase. Cancellation is not a failure observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyObservationOutcome {
    /// The phase completed successfully, independently of application acceptance.
    Succeeded,
    /// A negative fact with an explicit scope and a bounded, non-secret cause.
    Failed {
        /// Component to which the observation applies.
        scope: ProxyFailureScope,
        /// Diagnostic category, never request content or credentials.
        cause: String,
    },
    /// The available facts cannot establish capability or global failure.
    Inconclusive,
}

/// Identity of a probe observation; changes in policy do not reuse old attempts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProxyProbeContext {
    /// Phase being observed.
    pub phase: ProxyObservationPhase,
    /// Protocol used for this exchange.
    pub protocol: Option<ProxyProtocol>,
    /// Target URL without credentials, query parameters or fragment.
    pub target: Option<String>,
    /// Probe HTTP method, when applicable.
    pub method: Option<String>,
    /// Versioned identity of the probe policy and timeout configuration.
    pub configuration: String,
}

/// Latest result and retained positive fact for one observation context.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProxyObservation {
    /// Context of the actual exchange.
    pub context: ProxyProbeContext,
    /// Latest completed result.
    pub outcome: ProxyObservationOutcome,
    /// UTC completion time, persisted as epoch milliseconds.
    #[serde(with = "super::proxy_record::system_time_serde")]
    pub completed_at: SystemTime,
    /// Most recent positive observation, not erased by a subsequent failure.
    #[serde(default, with = "super::proxy_record::option_system_time_serde")]
    pub last_succeeded_at: Option<SystemTime>,
    /// Observed HTTP status; presence is not application acceptance.
    pub http_status: Option<u16>,
}

/// Health dates independent of attempt dates and provider claims.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProxyHealthEvidence {
    /// Eviction tombstone: retained for bounded rediscovery protection, never selectable.
    #[serde(default, with = "super::proxy_record::option_system_time_serde")]
    pub evicted_at: Option<SystemTime>,
    /// Initial inventory discovery, used only for retention of unvalidated records.
    #[serde(default, with = "super::proxy_record::option_system_time_serde")]
    pub first_discovered_at: Option<SystemTime>,
    /// Last positively observed connection or exchange through the proxy.
    #[serde(default, with = "super::proxy_record::option_system_time_serde")]
    pub last_positive_at: Option<SystemTime>,
}

/// Evidence about delivery of the business request, not its idempotence.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProxyRequestDelivery {
    /// Transport proves that no business request was sent.
    NotSent,
    /// Delivery is uncertain; this is the conservative default.
    #[default]
    PossiblySent,
    /// Response headers were received; body collection may still fail.
    ResponseReceived,
}
