//! Eligibility checks and probe freshness.

use std::time::{Duration, SystemTime};

use super::PROXY_TRANSIENT_FAILURE_COOLDOWN;
use crate::core::{
    Destination, ProbeMode, ProxyCapabilityStatus, ProxyProtocol, ProxyRecord, ProxyRuntimeStatus,
};

/// Returns wall-clock milliseconds since the Unix epoch, clamped to the `u64` range.
pub(super) fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

/// Checks whether the same probe phase was attempted within its validity window.
///
/// # Arguments
/// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
/// * `phase` - Probe phase whose attempt or observation is being recorded.
/// * `ttl` - Validity window for the last recorded probe attempt.
pub(super) fn recent_probe_attempt(record: &ProxyRecord, phase: &str, ttl: Duration) -> bool {
    record.probe_attempts.get(phase).is_some_and(|started| {
        epoch_millis().saturating_sub(*started) < ttl.as_millis().min(u64::MAX as u128) as u64
    })
}

/// Allows an inconclusive complementary trial only with usable relaxed-mode endpoint evidence.
///
/// # Arguments
/// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
/// * `mode` - Strict or relaxed policy governing acceptance of unmeasured capabilities.
pub(super) fn complementary_trial_allowed(record: &ProxyRecord, mode: ProbeMode) -> bool {
    mode == ProbeMode::Relaxed
        && record.tcp_reachable == Some(true)
        && record.protocol.is_some()
        && record.authentication_required != Some(true)
        && (record.protocol != Some(ProxyProtocol::Https)
            || record.proxy_tls_certificate == ProxyCapabilityStatus::Available)
}

/// Returns whether the requested capability still needs a runtime measurement.
///
/// # Arguments
/// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
/// * `require_https` - Whether selection requires an HTTPS tunnel and destination TLS capability.
///
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
///
/// # Arguments
/// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
/// * `country` - ISO alpha-2 country code associated with the requested inventory partition.
/// * `require_https` - Whether selection requires an HTTPS tunnel and destination TLS capability.
/// * `destination` - Final origin used for capability checks and destination-specific exclusions.
/// * `now` - Reference wall-clock time used for age and cooldown comparisons.
/// * `probe_mode` - Strict or relaxed policy governing acceptance of unmeasured capabilities.
///
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

/// Checks country, measured capabilities, authentication, eviction and active cooldowns.
///
/// # Arguments
/// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
/// * `country` - ISO alpha-2 country code associated with the requested inventory partition.
/// * `require_https` - Whether selection requires an HTTPS tunnel and destination TLS capability.
/// * `destination` - Final origin used for capability checks and destination-specific exclusions.
/// * `now` - Reference wall-clock time used for age and cooldown comparisons.
/// * `probe_mode` - Strict or relaxed policy governing acceptance of unmeasured capabilities.
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

/// Reports missing or older-than-TTL runtime measurements; future timestamps are not expired.
///
/// # Arguments
/// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
/// * `now` - Reference wall-clock time used for age and cooldown comparisons.
/// * `probe_ttl` - Maximum age for reusing measured runtime capabilities.
pub(super) fn probe_is_expired(record: &ProxyRecord, now: SystemTime, probe_ttl: Duration) -> bool {
    let Some(last_checked) = record.last_checked else {
        return true;
    };

    now.duration_since(last_checked)
        .map(|age| age > probe_ttl)
        .unwrap_or(false)
}

/// Checks exact scheme, host and port exclusions, capping ambiguous failures to the transient window.
///
/// # Arguments
/// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
/// * `destination` - Final origin used for capability checks and destination-specific exclusions.
/// * `now` - Reference wall-clock time used for age and cooldown comparisons.
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

/// Applies the existing capability policy to an HTTPS destination.
///
/// # Arguments
/// - `record`: Cached endpoint evidence.
/// - `probe_mode`: Strict or relaxed interpretation of source declarations.
pub(super) fn can_reach_https_destination(record: &ProxyRecord, probe_mode: ProbeMode) -> bool {
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

/// Applies strict or relaxed HTTP capability eligibility without modifying observations.
///
/// # Arguments
/// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
/// * `probe_mode` - Strict or relaxed policy governing acceptance of unmeasured capabilities.
fn can_reach_http_destination(record: &ProxyRecord, probe_mode: ProbeMode) -> bool {
    match (probe_mode, record.http_forwarding) {
        (_, ProxyCapabilityStatus::Available) => true,
        (_, ProxyCapabilityStatus::Unavailable)
        | (ProbeMode::Strict, ProxyCapabilityStatus::Unknown) => false,
        (ProbeMode::Relaxed, ProxyCapabilityStatus::Unknown) => record.protocol.is_some(),
    }
}

/// Maps HTTP and HTTPS destinations to their scheme and all other protocols to `tcp`.
///
/// # Arguments
/// * `destination` - Final origin used for capability checks and destination-specific exclusions.
pub(super) fn destination_scheme(destination: &Destination) -> String {
    match destination.protocol {
        crate::core::ApplicationProtocol::Http => "http".to_string(),
        crate::core::ApplicationProtocol::Https => "https".to_string(),
        _ => "tcp".to_string(),
    }
}
