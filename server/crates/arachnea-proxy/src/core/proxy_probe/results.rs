//! Internal capability results and their application to proxy records.

use crate::core::{
    ProxyCapabilityStatus, ProxyError, ProxyProtocol, ProxyRecord, ProxyRuntimeStatus, Result,
};
use std::time::SystemTime;

use super::config::ProbeMode;
use super::http::{HttpProbeTarget, HttpsProbeTarget};
use super::ProxyProbe;

#[derive(Clone, Copy)]
pub(super) enum ProxyCapability {
    HttpForwarding,
    HttpsTunnel,
}

pub(super) enum CapabilityProbeTarget<'a> {
    HttpForwarding(&'a HttpProbeTarget),
    Https(&'a HttpsProbeTarget),
}

pub(super) struct CapabilityProbeAttempt {
    pub(super) endpoint_reachable: bool,
    pub(super) endpoint_connection_failed: bool,
    pub(super) latency_ms: Option<u64>,
    pub(super) proxy_tls_certificate: ProxyCapabilityStatus,
    pub(super) capability_status: ProxyCapabilityStatus,
    pub(super) destination_tls: ProxyCapabilityStatus,
    pub(super) usable: bool,
    pub(super) authentication_required: bool,
    pub(super) error: Option<ProxyError>,
}

impl CapabilityProbeAttempt {
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

    pub(super) fn endpoint_connection_failed(error: ProxyError) -> Self {
        let mut attempt = Self::failed(error);
        attempt.endpoint_connection_failed = true;
        attempt
    }
}

/// Intermediate probe result that gets applied to a `ProxyRecord`.
#[derive(Default)]
pub(super) struct ProtocolProbeResult {
    pub(super) endpoint_reachable: bool,
    pub(super) endpoint_connection_failed: bool,
    pub(super) capability_configured: bool,
    pub(super) usable: bool,
    pub(super) latency_ms: Option<u64>,
    pub(super) authentication_required: bool,
    pub(super) http_forwarding: ProxyCapabilityStatus,
    pub(super) https_tunnel: ProxyCapabilityStatus,
    pub(super) destination_tls: ProxyCapabilityStatus,
    pub(super) proxy_tls_certificate: ProxyCapabilityStatus,
    pub(super) protocol: Option<ProxyProtocol>,
    pub(super) error: Option<ProxyError>,
}

impl ProtocolProbeResult {
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
pub(super) fn is_auth_required(error: &ProxyError) -> bool {
    if let ProxyError::UpstreamRejected(msg) = error {
        msg.contains("status 407")
    } else {
        false
    }
}

pub(super) fn capability_status<T>(result: &Result<T>) -> ProxyCapabilityStatus {
    match result {
        Ok(_) => ProxyCapabilityStatus::Available,
        Err(error) => failed_capability_status(error),
    }
}

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
