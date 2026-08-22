//! Persistence bridge between [`ProxyRecord`] and the generic persistence
//! store.
//!
//! Proxy records are persisted as named fields (the serialized record itself),
//! keyed by authority, in the single `proxy-inventory` namespace. The country
//! lives in the `country` field so records can be queried per country through
//! [`arachnea_core::persistence::PersistenceTransaction::find_by_fields`].

use std::time::{Duration, SystemTime};

use arachnea_core::persistence::PersistedRecord;

use crate::core::{ProxyError, ProxyRecord, Result};

/// Format version of persisted proxy records.
pub const PROXY_RECORD_FORMAT_VERSION: u32 = 1;

/// Namespace used to persist dynamic proxy records.
///
/// All proxies share this namespace; filtering by country is done through
/// field queries rather than separate files.
pub const PROXY_NAMESPACE: &str = "proxy-inventory";

/// Default time-to-live applied to cached proxy records without their own
/// freshness information (`last_checked` absent).
///
/// No proxy record stays in the cache indefinitely: expired records are
/// pruned by the backend on read and are ineligible for selection anyway,
/// which naturally triggers the provider fallback.
pub const PROXY_CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

impl From<&ProxyRecord> for PersistedRecord {
    fn from(record: &ProxyRecord) -> Self {
        let now = SystemTime::now();
        let fields = serde_json::to_value(record)
            .ok()
            .and_then(|value| value.as_object().cloned());
        PersistedRecord {
            format_version: PROXY_RECORD_FORMAT_VERSION,
            fields,
            expires_at: Some(proxy_cache_expires_at(record, now)),
            updated_at: now,
        }
    }
}

impl TryFrom<&PersistedRecord> for ProxyRecord {
    type Error = ProxyError;

    fn try_from(record: &PersistedRecord) -> Result<Self> {
        if record.format_version != PROXY_RECORD_FORMAT_VERSION {
            return Err(ProxyError::Config(format!(
                "unsupported proxy record format version {}",
                record.format_version
            )));
        }
        let Some(fields) = record.fields.as_ref() else {
            return Err(ProxyError::Config(
                "proxy record has no persisted fields".to_string(),
            ));
        };
        serde_json::from_value(serde_json::Value::Object(fields.clone())).map_err(|err| {
            ProxyError::Config(format!("failed to deserialize proxy record: {err}"))
        })
    }
}

/// Computes the cache expiry of a proxy record.
///
/// The expiry is the earliest of the active cooldown end and the probe
/// freshness window (`last_checked` + [`PROXY_CACHE_TTL`]). Records without
/// any freshness information expire after [`PROXY_CACHE_TTL`] from `now`.
fn proxy_cache_expires_at(record: &ProxyRecord, now: SystemTime) -> SystemTime {
    let ttl_expiry = record
        .last_checked
        .map(|last_checked| last_checked + PROXY_CACHE_TTL)
        .unwrap_or(now + PROXY_CACHE_TTL);
    match record.cooldown_until {
        Some(cooldown_until) => cooldown_until.min(ttl_expiry),
        None => ttl_expiry,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{ProxyProtocol, ProxyRuntimeStatus};

    fn sample_record() -> ProxyRecord {
        ProxyRecord {
            protocol: Some(ProxyProtocol::Http),
            host: "203.0.113.10".to_string(),
            port: 8080,
            country: Some("FR".to_string()),
            supports_https: None,
            status: ProxyRuntimeStatus::Ok,
            latency_ms: Some(120),
            failure_count: 0,
            authentication_required: None,
            availability: Default::default(),
            destination_failures: Vec::new(),
            last_checked: None,
            cooldown_until: None,
        }
    }

    #[test]
    fn conversion_roundtrip_preserves_record() {
        let record = sample_record();
        let persisted = PersistedRecord::from(&record);
        assert_eq!(persisted.format_version, PROXY_RECORD_FORMAT_VERSION);
        let restored = ProxyRecord::try_from(&persisted).expect("conversion succeeds");
        assert_eq!(restored, record);
    }

    #[test]
    fn persisted_fields_are_queryable_by_country() {
        let record = sample_record();
        let persisted = PersistedRecord::from(&record);
        let fields = persisted.fields.expect("fields present");
        assert_eq!(
            fields.get("country").cloned(),
            Some(serde_json::Value::String("FR".to_string()))
        );
    }

    #[test]
    fn expiry_uses_global_ttl_without_freshness() {
        let record = sample_record();
        let persisted = PersistedRecord::from(&record);
        let expires_at = persisted.expires_at.expect("expiry set");
        let age = expires_at
            .duration_since(SystemTime::now())
            .expect("expiry in the future");
        assert!(age <= PROXY_CACHE_TTL);
        assert!(age > PROXY_CACHE_TTL - Duration::from_secs(5));
    }

    #[test]
    fn expiry_is_min_of_cooldown_and_probe_window() {
        let mut record = sample_record();
        let last_checked = SystemTime::now() - Duration::from_secs(60);
        record.last_checked = Some(last_checked);
        record.cooldown_until = Some(SystemTime::now() + Duration::from_secs(3_600));
        let persisted = PersistedRecord::from(&record);
        let expires_at = persisted.expires_at.expect("expiry set");
        // The probe window (last_checked + TTL) is far later than the cooldown;
        // the earliest bound wins.
        assert_eq!(expires_at, record.cooldown_until.unwrap());

        record.cooldown_until = None;
        let persisted = PersistedRecord::from(&record);
        let expires_at = persisted.expires_at.expect("expiry set");
        assert_eq!(expires_at, last_checked + PROXY_CACHE_TTL);
    }

    #[test]
    fn unknown_format_version_is_rejected() {
        let record = sample_record();
        let mut persisted = PersistedRecord::from(&record);
        persisted.format_version = PROXY_RECORD_FORMAT_VERSION + 1;
        assert!(ProxyRecord::try_from(&persisted).is_err());
    }

    #[test]
    fn missing_fields_are_rejected() {
        let persisted = PersistedRecord {
            format_version: PROXY_RECORD_FORMAT_VERSION,
            fields: None,
            expires_at: None,
            updated_at: SystemTime::now(),
        };
        assert!(ProxyRecord::try_from(&persisted).is_err());
    }
}