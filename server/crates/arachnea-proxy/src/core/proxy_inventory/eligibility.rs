//! Eligibility checks, probe freshness and selection diagnostics.

use std::time::{Duration, SystemTime};

use super::PROXY_TRANSIENT_FAILURE_COOLDOWN;
use crate::core::{
    Destination, ProbeMode, ProxyCapabilityStatus, ProxyProtocol, ProxyRecord, ProxyRuntimeStatus,
};

#[cfg(feature = "performance-diagnostics")]
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct SelectionStats {
    pub(super) total: usize,
    pub(super) ok: usize,
    pub(super) unknown: usize,
    pub(super) ko: usize,
    pub(super) authentication_required: usize,
    pub(super) authentication_required_flag: usize,
    pub(super) cooldown: usize,
    pub(super) probe_expired: usize,
    pub(super) destination_cooldown: usize,
    pub(super) https_rejected: usize,
    pub(super) http_forward_available: usize,
    pub(super) http_forward_unavailable: usize,
    pub(super) http_forward_unknown: usize,
    pub(super) https_tunnel_available: usize,
    pub(super) https_tunnel_unavailable: usize,
    pub(super) https_tunnel_unknown: usize,
    pub(super) destination_tls_available: usize,
    pub(super) destination_tls_unavailable: usize,
    pub(super) destination_tls_unknown: usize,
    pub(super) proxy_tls_valid: usize,
    pub(super) proxy_tls_invalid: usize,
    pub(super) proxy_tls_unknown: usize,
    pub(super) eligible: usize,
}

#[cfg(feature = "performance-diagnostics")]
pub(super) fn selection_stats<'a>(
    records: impl IntoIterator<Item = &'a ProxyRecord>,
    country: &str,
    require_https: bool,
    destination: Option<&Destination>,
    now: SystemTime,
    probe_ttl: Duration,
    probe_mode: ProbeMode,
) -> SelectionStats {
    let mut stats = SelectionStats::default();

    for record in records {
        stats.total += 1;
        match record.status {
            ProxyRuntimeStatus::Ok => stats.ok += 1,
            ProxyRuntimeStatus::Unknown => stats.unknown += 1,
            ProxyRuntimeStatus::Ko => stats.ko += 1,
            ProxyRuntimeStatus::AuthenticationRequired => stats.authentication_required += 1,
        }

        if record.authentication_required == Some(true) {
            stats.authentication_required_flag += 1;
        }

        match record.http_forwarding {
            ProxyCapabilityStatus::Available => stats.http_forward_available += 1,
            ProxyCapabilityStatus::Unavailable => stats.http_forward_unavailable += 1,
            ProxyCapabilityStatus::Unknown => stats.http_forward_unknown += 1,
        }
        match record.https_tunnel {
            ProxyCapabilityStatus::Available => stats.https_tunnel_available += 1,
            ProxyCapabilityStatus::Unavailable => stats.https_tunnel_unavailable += 1,
            ProxyCapabilityStatus::Unknown => stats.https_tunnel_unknown += 1,
        }
        match record.destination_tls {
            ProxyCapabilityStatus::Available => stats.destination_tls_available += 1,
            ProxyCapabilityStatus::Unavailable => stats.destination_tls_unavailable += 1,
            ProxyCapabilityStatus::Unknown => stats.destination_tls_unknown += 1,
        }
        match record.proxy_tls_certificate {
            ProxyCapabilityStatus::Available => stats.proxy_tls_valid += 1,
            ProxyCapabilityStatus::Unavailable => stats.proxy_tls_invalid += 1,
            ProxyCapabilityStatus::Unknown => stats.proxy_tls_unknown += 1,
        }

        if record.cooldown_until.is_some_and(|cooldown| cooldown > now) {
            stats.cooldown += 1;
        }

        if probe_is_expired(record, now, probe_ttl) {
            stats.probe_expired += 1;
        }

        if destination
            .is_some_and(|destination| has_active_destination_cooldown(record, destination, now))
        {
            stats.destination_cooldown += 1;
        }

        if require_https && !can_reach_https_destination(record, probe_mode) {
            stats.https_rejected += 1;
        }

        if is_eligible(record, country, require_https, destination, now, probe_mode) {
            stats.eligible += 1;
        }
    }

    stats
}

pub(super) fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub(super) fn recent_probe_attempt(record: &ProxyRecord, phase: &str, ttl: Duration) -> bool {
    record.probe_attempts.get(phase).is_some_and(|started| {
        epoch_millis().saturating_sub(*started) < ttl.as_millis().min(u64::MAX as u128) as u64
    })
}

pub(super) fn complementary_trial_allowed(record: &ProxyRecord, mode: ProbeMode) -> bool {
    mode == ProbeMode::Relaxed
        && record.tcp_reachable == Some(true)
        && record.protocol.is_some()
        && record.authentication_required != Some(true)
        && (record.protocol != Some(ProxyProtocol::Https)
            || record.proxy_tls_certificate == ProxyCapabilityStatus::Available)
}

/// Returns whether the requested capability still needs a runtime measurement.
pub(super) fn needs_capability_probe(record: &ProxyRecord, require_https: bool) -> bool {
    record.tcp_reachable == Some(false)
        || record.last_checked.is_none()
        || record.protocol.is_none()
        || matches!(record.status, ProxyRuntimeStatus::Unknown)
        || if require_https {
            record.https_tunnel == ProxyCapabilityStatus::Unknown
                || record.destination_tls == ProxyCapabilityStatus::Unknown
                || (record.protocol == Some(ProxyProtocol::Https)
                    && record.proxy_tls_certificate == ProxyCapabilityStatus::Unknown)
        } else {
            record.http_forwarding == ProxyCapabilityStatus::Unknown
        }
}

/// Includes untested records for lazy probing while preserving explicit
/// capability failures, authentication exclusions, and active cooldowns.
pub(super) fn is_selection_candidate(
    record: &ProxyRecord,
    country: &str,
    require_https: bool,
    destination: Option<&Destination>,
    now: SystemTime,
    probe_mode: ProbeMode,
) -> bool {
    if record.proxy_tls_certificate == ProxyCapabilityStatus::Unavailable {
        return false;
    }
    let mut candidate = record.clone();
    if candidate.status == ProxyRuntimeStatus::Unknown
        || (candidate.status == ProxyRuntimeStatus::Ko
            && candidate.tcp_reachable == Some(false)
            && candidate.cooldown_until.is_some_and(|until| until <= now))
    {
        candidate.status = ProxyRuntimeStatus::Ok;
    }
    if candidate.http_forwarding == ProxyCapabilityStatus::Unknown {
        candidate.http_forwarding = ProxyCapabilityStatus::Available;
    }
    if candidate.https_tunnel == ProxyCapabilityStatus::Unknown {
        candidate.https_tunnel = ProxyCapabilityStatus::Available;
    }
    if candidate.destination_tls == ProxyCapabilityStatus::Unknown {
        candidate.destination_tls = ProxyCapabilityStatus::Available;
    }
    is_eligible(
        &candidate,
        country,
        require_https,
        destination,
        now,
        probe_mode,
    )
}

pub(super) fn is_eligible(
    record: &ProxyRecord,
    country: &str,
    require_https: bool,
    destination: Option<&Destination>,
    now: SystemTime,
    probe_mode: ProbeMode,
) -> bool {
    if record.health_evidence.evicted_at.is_some()
        || record.proxy_tls_certificate == ProxyCapabilityStatus::Unavailable
    {
        return false;
    }
    if record.country.as_deref() != Some(country) {
        return false;
    }

    if !matches!(record.status, ProxyRuntimeStatus::Ok) {
        return false;
    }

    if record.authentication_required == Some(true) {
        return false;
    }

    if let Some(cooldown) = record.cooldown_until {
        if cooldown > now {
            return false;
        }
    }

    if destination
        .is_some_and(|destination| has_active_destination_cooldown(record, destination, now))
    {
        return false;
    }

    if require_https {
        if !can_reach_https_destination(record, probe_mode) {
            return false;
        }
    } else if !can_reach_http_destination(record, probe_mode) {
        return false;
    }

    true
}

pub(super) fn probe_is_expired(record: &ProxyRecord, now: SystemTime, probe_ttl: Duration) -> bool {
    let Some(last_checked) = record.last_checked else {
        return true;
    };

    now.duration_since(last_checked)
        .map(|age| age > probe_ttl)
        .unwrap_or(false)
}

pub(super) fn has_active_destination_cooldown(
    record: &ProxyRecord,
    destination: &Destination,
    now: SystemTime,
) -> bool {
    let scheme = destination_scheme(destination);
    let host = destination.host_for_protocol();
    record.destination_failures.iter().any(|failure| {
        failure.scheme == scheme
            && failure.host == host
            && failure.port == destination.port
            && failure.cooldown_until.is_some_and(|cooldown| {
                let cooldown =
                    if failure.reason == crate::core::ProxyDestinationFailureReason::Other {
                        cooldown.min(
                            failure
                                .last_failed
                                .checked_add(PROXY_TRANSIENT_FAILURE_COOLDOWN)
                                .unwrap_or(cooldown),
                        )
                    } else {
                        cooldown
                    };
                cooldown > now
            })
    })
}

fn can_reach_https_destination(record: &ProxyRecord, probe_mode: ProbeMode) -> bool {
    if matches!(record.destination_tls, ProxyCapabilityStatus::Unavailable) {
        return false;
    }

    match probe_mode {
        ProbeMode::Strict => {
            matches!(record.https_tunnel, ProxyCapabilityStatus::Available)
                && matches!(record.destination_tls, ProxyCapabilityStatus::Available)
        }
        ProbeMode::Relaxed => match record.https_tunnel {
            ProxyCapabilityStatus::Available => {
                !matches!(record.destination_tls, ProxyCapabilityStatus::Unavailable)
            }
            ProxyCapabilityStatus::Unavailable => false,
            ProxyCapabilityStatus::Unknown => match record.protocol {
                Some(ProxyProtocol::Socks4 | ProxyProtocol::Socks4a | ProxyProtocol::Socks5) => {
                    true
                }
                Some(ProxyProtocol::Http | ProxyProtocol::Https) => {
                    record.supports_https == Some(true)
                }
                None => false,
            },
        },
    }
}

fn can_reach_http_destination(record: &ProxyRecord, probe_mode: ProbeMode) -> bool {
    match (probe_mode, record.http_forwarding) {
        (_, ProxyCapabilityStatus::Available) => true,
        (_, ProxyCapabilityStatus::Unavailable)
        | (ProbeMode::Strict, ProxyCapabilityStatus::Unknown) => false,
        (ProbeMode::Relaxed, ProxyCapabilityStatus::Unknown) => record.protocol.is_some(),
    }
}

pub(super) fn destination_scheme(destination: &Destination) -> String {
    match destination.protocol {
        crate::core::ApplicationProtocol::Http => "http".to_string(),
        crate::core::ApplicationProtocol::Https => "https".to_string(),
        _ => "tcp".to_string(),
    }
}
