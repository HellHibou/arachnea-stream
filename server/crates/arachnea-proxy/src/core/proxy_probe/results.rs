//! Internal capability results and their application to proxy records.

use crate::core::{
    ProxyCapabilityStatus, ProxyError, ProxyProtocol, ProxyRecord, ProxyRuntimeStatus, Result,
};
use std::time::SystemTime;

use super::config::ProbeMode;
use super::http::{HttpProbeTarget, HttpsProbeTarget};
use super::ProxyProbe;
/// Complementary capability whose runtime evidence is being measured.
#[derive(Clone, Copy)]
pub(super) enum ProxyCapability {
    /// Ordinary HTTP forwarding through the proxy.
    HttpForwarding,
    /// Tunnel establishment and destination TLS for HTTPS traffic.
    HttpsTunnel,
}
/// Borrowed HTTP or HTTPS target metadata for one complementary measurement.
pub(super) enum CapabilityProbeTarget<'a> {
    /// Absolute-form HTTP forwarding request metadata.
    HttpForwarding(&'a HttpProbeTarget),
    /// HTTPS tunnel and TLS request metadata.
    Https(&'a HttpsProbeTarget),
}
/// Evidence from one measured capability, separating endpoint failure from target uncertainty.
pub(super) struct CapabilityProbeAttempt {
    /// Whether TCP connectivity to the proxy endpoint was observed.
    pub(super) endpoint_reachable: bool,
    /// Whether failure was specifically attributed to connecting to the proxy endpoint.
    pub(super) endpoint_connection_failed: bool,
    /// Measured endpoint connection latency in milliseconds, if available.
    pub(super) latency_ms: Option<u64>,
    /// Measured TLS certificate status for an HTTPS proxy transport.
    pub(super) proxy_tls_certificate: ProxyCapabilityStatus,
    /// Measured status of the capability requested by this attempt.
    pub(super) capability_status: ProxyCapabilityStatus,
    /// Measured TLS status for the destination behind the proxy tunnel.
    pub(super) destination_tls: ProxyCapabilityStatus,
    /// Whether the measured evidence makes this attempt usable under its policy.
    pub(super) usable: bool,
    /// Whether an authentication challenge prevents unauthenticated use.
    pub(super) authentication_required: bool,
    /// Retained failure cause, if measurement did not complete successfully.
    pub(super) error: Option<ProxyError>,
}

impl CapabilityProbeAttempt {
    /// Builds a failed capability attempt, classifying its evidence and retaining the error.
    ///
    /// # Arguments
    /// * `error` - Observed error to classify without changing its cause.
    pub(super) fn failed(error: ProxyError) -> Self {
        let capability_status = failed_capability_status(&error);
        Self {
            endpoint_reachable: false,
            endpoint_connection_failed: false,
            latency_ms: None,
            proxy_tls_certificate: ProxyCapabilityStatus::Unknown,
            capability_status,
            destination_tls: ProxyCapabilityStatus::Unknown,
            usable: false,
            authentication_required: false,
            error: Some(error),
        }
    }

    /// Builds a failed attempt explicitly attributed to endpoint connection failure.
    ///
    /// # Arguments
    /// * `error` - Observed error to classify without changing its cause.
    pub(super) fn endpoint_connection_failed(error: ProxyError) -> Self {
        let mut attempt = Self::failed(error);
        attempt.endpoint_connection_failed = true;
        attempt
    }
}

/// Intermediate probe result that gets applied to a `ProxyRecord`.
#[derive(Default)]
pub(super) struct ProtocolProbeResult {
    /// Whether TCP connectivity to the proxy endpoint was observed.
    pub(super) endpoint_reachable: bool,
    /// Whether failure was specifically attributed to connecting to the proxy endpoint.
    pub(super) endpoint_connection_failed: bool,
    /// Whether at least one complementary capability target was configured.
    pub(super) capability_configured: bool,
    /// Whether the measured evidence makes this attempt usable under its policy.
    pub(super) usable: bool,
    /// Measured endpoint connection latency in milliseconds, if available.
    pub(super) latency_ms: Option<u64>,
    /// Whether an authentication challenge prevents unauthenticated use.
    pub(super) authentication_required: bool,
    /// Measured ability to forward ordinary HTTP requests.
    pub(super) http_forwarding: ProxyCapabilityStatus,
    /// Measured ability to establish an HTTPS destination tunnel.
    pub(super) https_tunnel: ProxyCapabilityStatus,
    /// Measured TLS status for the destination behind the proxy tunnel.
    pub(super) destination_tls: ProxyCapabilityStatus,
    /// Measured TLS certificate status for an HTTPS proxy transport.
    pub(super) proxy_tls_certificate: ProxyCapabilityStatus,
    /// Proxy protocol selected by the measurement, if identified.
    pub(super) protocol: Option<ProxyProtocol>,
    /// Retained failure cause, if measurement did not complete successfully.
    pub(super) error: Option<ProxyError>,
}

impl ProtocolProbeResult {
    /// Merges one capability attempt into the aggregate endpoint result and its observations.
    ///
    /// # Arguments
    /// * `capability` - Capability whose result is merged into the aggregate probe result.
    /// * `attempt` - Measured outcome of one capability probe.
    /// * `host` - Hostname or IP address to resolve or connect to, without a port.
    /// * `port` - TCP port of the proxy endpoint.
    /// * `protocol` - Proxy transport protocol used for the endpoint handshake.
    pub(super) fn record_attempt(
        &mut self,
        capability: ProxyCapability,
        attempt: CapabilityProbeAttempt,
        host: &str,
        port: u16,
        protocol: &ProxyProtocol,
    ) {
        self.endpoint_reachable |= attempt.endpoint_reachable;
        self.endpoint_connection_failed |= attempt.endpoint_connection_failed;
        self.authentication_required |= attempt.authentication_required;
        self.usable |= attempt.usable;
        if let Some(latency_ms) = attempt.latency_ms {
            self.latency_ms = Some(
                self.latency_ms
                    .map_or(latency_ms, |current| current.min(latency_ms)),
            );
        }
        if !matches!(
            attempt.proxy_tls_certificate,
            ProxyCapabilityStatus::Unknown
        ) {
            self.proxy_tls_certificate = attempt.proxy_tls_certificate;
        }
        if !matches!(attempt.destination_tls, ProxyCapabilityStatus::Unknown) {
            self.destination_tls = attempt.destination_tls;
        }

        match capability {
            ProxyCapability::HttpForwarding => self.http_forwarding = attempt.capability_status,
            ProxyCapability::HttpsTunnel => self.https_tunnel = attempt.capability_status,
        }

        if let Some(error) = attempt.error {
            tracing::debug!(
                proxy = %if host.contains(':') {
                    format!("[{host}]:{port}")
                } else {
                    format!("{host}:{port}")
                },
                ?protocol,
                capability = match capability {
                    ProxyCapability::HttpForwarding => "http_forwarding",
                    ProxyCapability::HttpsTunnel => "https_tunnel",
                },
                %error,
                "dynamic proxy capability probe failed"
            );
            if self.error.is_none() {
                self.error = Some(error);
            }
        }
    }
}

/// Returns `true` when the error indicates the proxy requires authentication.
///
/// # Arguments
/// * `error` - Observed error to classify without changing its cause.
///
pub(super) fn is_auth_required(error: &ProxyError) -> bool {
    if let ProxyError::UpstreamRejected(msg) = error {
        msg.contains("status 407")
    } else {
        false
    }
}

/// Maps a successful measurement to availability and classifies failed measurements.
///
/// # Arguments
/// * `result` - Measured capability outcomes whose explicit evidence is retained separately from unknown states.
pub(super) fn capability_status<T>(result: &Result<T>) -> ProxyCapabilityStatus {
    match result {
        Ok(_) => ProxyCapabilityStatus::Available,
        Err(error) => failed_capability_status(error),
    }
}

/// Distinguishes proven capability failure from inconclusive transport or target errors.
///
/// # Arguments
/// * `error` - Observed error to classify without changing its cause.
pub(super) fn failed_capability_status(error: &ProxyError) -> ProxyCapabilityStatus {
    match error {
        ProxyError::Observation {
            phase: crate::core::ProxyObservationPhase::DestinationHttp,
            ..
        } => ProxyCapabilityStatus::Unknown,
        ProxyError::Observation { source, .. } | ProxyError::RequestAttempt { source, .. } => {
            failed_capability_status(source)
        }
        ProxyError::Timeout(_)
        | ProxyError::Io(_)
        | ProxyError::Config(_)
        | ProxyError::RouteUnavailable(_)
        | ProxyError::Dns(_)
        | ProxyError::Unsupported(_) => ProxyCapabilityStatus::Unknown,
        ProxyError::Tls(_)
        | ProxyError::UpstreamRejected(_)
        | ProxyError::HttpConnectRejected { .. } => ProxyCapabilityStatus::Unknown,
        ProxyError::Protocol(_)
        | ProxyError::InvalidDestination(_)
        | ProxyError::AccessDenied(_) => ProxyCapabilityStatus::Unavailable,
    }
}

impl ProxyProbe {
    // ── internal helpers ──────────────────────────────────────────────

    /// Updates a proxy record from measured evidence and returns any retained probe error.
    ///
    /// # Arguments
    /// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
    /// * `result` - Measured capability outcomes whose explicit evidence is retained separately from unknown states.
    pub(super) fn apply_result(
        &self,
        record: &mut ProxyRecord,
        result: ProtocolProbeResult,
    ) -> Option<ProxyError> {
        if let Some(protocol) = &result.protocol {
            record.protocol = Some(protocol.clone());
        }
        record.latency_ms = result.latency_ms;
        if result.http_forwarding != ProxyCapabilityStatus::Unknown {
            record.http_forwarding = result.http_forwarding;
        }
        if result.https_tunnel != ProxyCapabilityStatus::Unknown {
            record.https_tunnel = result.https_tunnel;
        }
        if result.destination_tls == ProxyCapabilityStatus::Available {
            record.destination_tls = result.destination_tls;
        }
        record.proxy_tls_certificate = result.proxy_tls_certificate;

        if result.authentication_required {
            record.status = ProxyRuntimeStatus::AuthenticationRequired;
            record.authentication_required = Some(true);
            record.failure_count += 1;
            return result.error;
        }
        let relaxed_fallback = matches!(self.config.mode, ProbeMode::Relaxed)
            && result.endpoint_reachable
            && self.relaxed_fallback_is_usable(record, &result);
        if result.endpoint_reachable {
            record.tcp_reachable = Some(true);
            if !result.authentication_required {
                record.status = ProxyRuntimeStatus::Ok;
            }
        }
        if result.usable
            || (result.endpoint_reachable && !result.capability_configured)
            || relaxed_fallback
        {
            record.status = ProxyRuntimeStatus::Ok;
            if result.usable || !result.capability_configured {
                record.last_validated_at = Some(SystemTime::now());
            }
            record.authentication_required = Some(false);
            record.failure_count = 0;
        } else if result.endpoint_connection_failed {
            record.status = ProxyRuntimeStatus::Ko;
            record.authentication_required = Some(false);
            record.failure_count += 1;
        }

        if result.endpoint_connection_failed
            || (!result.usable && result.capability_configured && !relaxed_fallback)
        {
            result.error
        } else {
            None
        }
    }

    /// Checks whether relaxed endpoint evidence permits fallback without overriding explicit failures.
    ///
    /// # Arguments
    /// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
    /// * `result` - Measured capability outcomes whose explicit evidence is retained separately from unknown states.
    pub(super) fn relaxed_fallback_is_usable(
        &self,
        record: &ProxyRecord,
        result: &ProtocolProbeResult,
    ) -> bool {
        if result.authentication_required || record.authentication_required == Some(true) {
            return false;
        }
        let protocol = result.protocol.as_ref().or(record.protocol.as_ref());
        let proxy_transport_usable = !matches!(
            result.proxy_tls_certificate,
            ProxyCapabilityStatus::Unavailable
        );
        let http_fallback = self.config.http_probe_url.is_some()
            && proxy_transport_usable
            && matches!(result.http_forwarding, ProxyCapabilityStatus::Unknown)
            && protocol.is_some();
        let https_fallback = self.config.https_probe_url.is_some()
            && proxy_transport_usable
            && !matches!(result.destination_tls, ProxyCapabilityStatus::Unavailable)
            && match result.https_tunnel {
                ProxyCapabilityStatus::Available => true,
                ProxyCapabilityStatus::Unavailable => false,
                ProxyCapabilityStatus::Unknown => match protocol {
                    Some(
                        ProxyProtocol::Socks4 | ProxyProtocol::Socks4a | ProxyProtocol::Socks5,
                    ) => true,
                    Some(ProxyProtocol::Http | ProxyProtocol::Https) => {
                        record.supports_https == Some(true)
                    }
                    None => false,
                },
            };

        http_fallback || https_fallback
    }
}
