//! Record merging, country indexes and IP-country resolution.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, SystemTime};

use super::ProxyInventory;
use crate::core::{
    ProxyDeclaration, ProxyProtocol, ProxyRecord, ProxyRuntimeStatus, PROXY_PROTOCOL_PRIORITY,
};

use super::eligibility::probe_is_expired;

fn should_preserve_runtime_exclusion(record: &ProxyRecord) -> bool {
    matches!(
        record.status,
        ProxyRuntimeStatus::Ko | ProxyRuntimeStatus::AuthenticationRequired
    ) || record.authentication_required == Some(true)
        || record.cooldown_until.is_some()
}

fn has_fresh_runtime_update(record: &ProxyRecord) -> bool {
    record.last_checked.is_some() && !matches!(record.status, ProxyRuntimeStatus::Unknown)
}

/// Deduplicates provider records and identifies the entries that require a
/// network probe.
///
/// A cached endpoint whose last probe is still within `probe_ttl` retains all
/// runtime-derived fields. Provider-owned metadata such as country and
/// availability still comes from the refreshed source record.
pub(super) fn prepare_provider_records(
    records: Vec<ProxyRecord>,
    existing: &HashMap<String, ProxyRecord>,
    now: SystemTime,
    probe_ttl: Duration,
) -> (Vec<ProxyRecord>, Vec<usize>, usize, usize) {
    let loaded_count = records.len();
    let mut grouped = Vec::with_capacity(loaded_count);
    let mut grouped_indices = HashMap::new();
    for mut record in records {
        let authority = record.authority();
        if let Some(index) = grouped_indices.get(&authority).copied() {
            merge_provider_declaration(&mut grouped[index], record);
        } else {
            record.declarations = declarations_from_record(&record);
            grouped_indices.insert(authority, grouped.len());
            grouped.push(record);
        }
    }

    let mut prepared = Vec::with_capacity(grouped.len());
    let mut probe_indices = Vec::new();
    let duplicate_count = loaded_count.saturating_sub(grouped.len());
    let mut cached_count = 0;

    for mut record in grouped {
        let authority = record.authority();
        if let Some(cached) = existing.get(&authority) {
            if !probe_is_expired(cached, now, probe_ttl) {
                retain_cached_runtime(&mut record, cached);
                cached_count += 1;
            } else {
                probe_indices.push(prepared.len());
            }
        } else {
            probe_indices.push(prepared.len());
        }
        prepared.push(record);
    }

    (prepared, probe_indices, duplicate_count, cached_count)
}

fn retain_cached_runtime(record: &mut ProxyRecord, cached: &ProxyRecord) {
    record.protocol = cached.protocol.clone();
    record.status = cached.status.clone();
    record.http_forwarding = cached.http_forwarding;
    record.https_tunnel = cached.https_tunnel;
    record.destination_tls = cached.destination_tls;
    record.proxy_tls_certificate = cached.proxy_tls_certificate;
    record.latency_ms = cached.latency_ms;
    record.failure_count = cached.failure_count;
    record.authentication_required = cached.authentication_required;
    record.destination_failures = cached.destination_failures.clone();
    record.last_checked = cached.last_checked;
    record.last_validated_at = cached.last_validated_at;
    record.health_evidence = cached.health_evidence.clone();
    record.observations = cached.observations.clone();
    record.cooldown_until = cached.cooldown_until;
}

fn merge_provider_declaration(target: &mut ProxyRecord, incoming: ProxyRecord) {
    for declaration in declarations_from_record(&incoming) {
        if !target.declarations.contains(&declaration) {
            target.declarations.push(declaration);
        }
    }

    target.supports_https = merge_https_hint(target.supports_https, incoming.supports_https);
    target.availability = preferred_availability(&target.availability, &incoming.availability);
    if target.country.is_none() {
        target.country = incoming.country;
    }
    target.protocol = preferred_declared_protocol(&target.declarations)
        .or(target.protocol.take())
        .or(incoming.protocol);
}

fn declarations_from_record(record: &ProxyRecord) -> Vec<ProxyDeclaration> {
    if record.declarations.is_empty() {
        vec![ProxyDeclaration {
            source: "legacy".to_string(),
            protocol: record.protocol.clone(),
            supports_https: record.supports_https,
        }]
    } else {
        record.declarations.clone()
    }
}

fn preferred_declared_protocol(declarations: &[ProxyDeclaration]) -> Option<ProxyProtocol> {
    PROXY_PROTOCOL_PRIORITY.iter().find_map(|preferred| {
        declarations
            .iter()
            .any(|declaration| declaration.protocol.as_ref() == Some(preferred))
            .then(|| preferred.clone())
    })
}

pub(super) fn declared_protocol_count(record: &ProxyRecord) -> usize {
    let mut protocols = record
        .declarations
        .iter()
        .filter_map(|declaration| declaration.protocol.as_ref())
        .collect::<Vec<_>>();
    protocols.sort_by_key(|protocol| protocol.to_string());
    protocols.dedup();
    protocols.len()
}

pub(super) fn runtime_protocol_is_undeclared(record: &ProxyRecord) -> bool {
    let Some(protocol) = &record.protocol else {
        return false;
    };
    !record.declarations.is_empty()
        && !record
            .declarations
            .iter()
            .any(|declaration| declaration.protocol.as_ref() == Some(protocol))
}

fn merge_https_hint(left: Option<bool>, right: Option<bool>) -> Option<bool> {
    match (left, right) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), _) | (_, Some(false)) => Some(false),
        (None, None) => None,
    }
}

fn preferred_availability(
    left: &crate::core::ProxyAvailabilityHint,
    right: &crate::core::ProxyAvailabilityHint,
) -> crate::core::ProxyAvailabilityHint {
    use crate::core::ProxyAvailabilityHint::{High, Low, Medium, Unknown};
    match (left, right) {
        (High, _) | (_, High) => High,
        (Medium, _) | (_, Medium) => Medium,
        (Low, _) | (_, Low) => Low,
        (Unknown, Unknown) => Unknown,
    }
}

impl ProxyInventory {
    /// Returns the number of records currently stored.
    pub async fn len(&self) -> usize {
        self.inner.read().await.records.len()
    }

    /// Returns `true` when the inventory is empty.
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    // ── Record management ────────────────────────────────────────────

    /// Adds or updates a batch of proxy records.
    ///
    /// Existing records with the same authority are merged. Hard exclusion
    /// state (`Ko`, authentication required, cooldowns, and destination
    /// failures) is preserved, while a fresh probed source record may refresh
    /// latency, HTTPS support and `last_checked` for otherwise eligible
    /// records.
    ///
    /// When an IP-country resolver is configured, records whose `country` is
    /// `None` and whose `host` is a valid IP address are resolved through the
    /// resolver before insertion. The resolution is bounded by the resolver's
    /// timeout.
    ///
    /// # Returns
    ///
    /// The canonical records retained by the inventory after merge and
    /// country-index updates, in input order.
    pub async fn add_or_update(&self, records: Vec<ProxyRecord>) -> Vec<ProxyRecord> {
        // Resolve missing countries for records whose host is an IP address
        let records = self.resolve_ip_countries(records).await;
        #[cfg(feature = "persistence")]
        let mut records = records;
        #[cfg(feature = "persistence")]
        if let Some(repository) = &self.persistence {
            for record in &mut records {
                if self
                    .inner
                    .read()
                    .await
                    .records
                    .contains_key(&record.authority())
                {
                    continue;
                }
                match repository.get(&record.key()).await {
                    Ok(Some(current)) => {
                        if current.health_evidence.last_positive_at.is_some()
                            || current.cooldown_until.is_some()
                        {
                            let country = record.country.clone();
                            *record = current;
                            if country.is_some() {
                                record.country = country;
                            }
                            continue;
                        }
                    }
                    Err(error) => tracing::warn!(%error, "failed to restore current proxy health"),
                    _ => {}
                }
                match repository.get_failure(&record.key()).await {
                    Ok(Some(failure)) => {
                        let country = record.country.clone();
                        *record = failure;
                        if country.is_some() {
                            record.country = country;
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to restore proxy rediscovery protection")
                    }
                    _ => {}
                }
            }
        }

        let mut inner = self.inner.write().await;
        let mut retained = Vec::with_capacity(records.len());
        let now = SystemTime::now();
        inner.evicted.retain(|_, record| {
            record.health_evidence.evicted_at.is_some_and(|date| {
                now.duration_since(date).unwrap_or_default() < Duration::from_secs(48 * 3600)
            })
        });
        for mut record in records {
            let key = record.authority();
            if !inner.records.contains_key(&key) {
                if let Some(failure) = inner.evicted.get(&key) {
                    let country = record.country.clone();
                    record = failure.clone();
                    if country.is_some() {
                        record.country = country;
                    }
                }
            }
            if record.health_evidence.evicted_at.is_some() {
                if record.cooldown_until.is_some_and(|date| date > now) {
                    continue;
                }
                record.health_evidence.evicted_at = None;
            }
            let old_country = inner.records.get(&key).and_then(|r| r.country.clone());
            let retained_record = if let Some(existing) = inner.records.get(&key) {
                let mut merged = record.clone();
                merged.health_evidence.first_discovered_at = existing
                    .health_evidence
                    .first_discovered_at
                    .or(record.health_evidence.first_discovered_at)
                    .or(Some(now));
                merged.health_evidence.last_positive_at = existing
                    .health_evidence
                    .last_positive_at
                    .max(record.health_evidence.last_positive_at);
                for observation in &existing.observations {
                    merged.record_observation(observation.clone());
                }
                for (phase, checked) in &existing.probe_attempts {
                    let incoming = merged.probe_attempts.entry(phase.clone()).or_default();
                    *incoming = (*incoming).max(*checked);
                }
                if record.probe_attempts.get("tcp") < existing.probe_attempts.get("tcp") {
                    merged.tcp_reachable = existing.tcp_reachable;
                }
                if (should_preserve_runtime_exclusion(existing)
                    && !(record.tcp_reachable == Some(true)
                        && record.health_evidence.last_positive_at
                            > existing.health_evidence.last_positive_at
                        && existing.authentication_required != Some(true)))
                    || !has_fresh_runtime_update(&record)
                {
                    merged.status = existing.status.clone();
                    merged.http_forwarding = existing.http_forwarding;
                    merged.https_tunnel = existing.https_tunnel;
                    merged.destination_tls = existing.destination_tls;
                    merged.proxy_tls_certificate = existing.proxy_tls_certificate;
                    merged.latency_ms = existing.latency_ms;
                    merged.failure_count = existing.failure_count;
                    merged.authentication_required = existing.authentication_required;
                    merged.last_checked = existing.last_checked;
                    merged.last_validated_at = existing.last_validated_at;
                    merged.cooldown_until = existing.cooldown_until;
                }
                merged.destination_failures = existing.destination_failures.clone();

                let country_changed = existing.country.as_deref() != merged.country.as_deref();
                if country_changed {
                    if let Some(ref old) = old_country {
                        if let Some(keys) = inner.country_records.get_mut(old) {
                            keys.retain(|k| k != &key);
                        }
                    }
                }

                inner.records.insert(key.clone(), merged.clone());
                merged
            } else {
                let mut record = record;
                record
                    .health_evidence
                    .first_discovered_at
                    .get_or_insert_with(SystemTime::now);
                inner.records.insert(key.clone(), record.clone());
                record
            };

            if let Some(country) = &retained_record.country {
                let keys = inner.country_records.entry(country.clone()).or_default();
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            retained.push(retained_record);
        }
        drop(inner);

        retained
    }

    /// Resolves missing country codes for records whose host is a valid IP
    /// address, using the configured IP-country resolver.
    ///
    /// This is a bounded synchronous resolution: it waits for the resolver's
    /// response with a configurable timeout and deduplicates concurrent
    /// lookups for the same IP.
    async fn resolve_ip_countries(&self, mut records: Vec<ProxyRecord>) -> Vec<ProxyRecord> {
        let Some(resolver) = &self.ip_country_resolver else {
            return records;
        };

        // Load the resolver from store if not already loaded
        if let Err(error) = resolver.load_from_store().await {
            tracing::warn!(%error, "failed to load IP-country resolver from store");
        }

        let mut resolved_any = false;
        for record in records.iter_mut() {
            if record.country.is_some() {
                continue;
            }

            // Only attempt resolution for records whose host is a valid IP
            let ip: IpAddr = match record.host.parse() {
                Ok(ip) => ip,
                Err(_) => continue,
            };

            match resolver.resolve(&ip).await {
                Ok(Some(country)) => {
                    tracing::debug!(
                        host = %record.host,
                        country = %country,
                        authority = %record.authority(),
                        "resolved missing country code for proxy record"
                    );
                    record.country = Some(country);
                    resolved_any = true;
                }
                Ok(None) => {
                    tracing::trace!(
                        host = %record.host,
                        "IP-country resolver returned no country for proxy record"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        host = %record.host,
                        %error,
                        "IP-country resolution failed for proxy record"
                    );
                }
            }
        }

        if resolved_any {
            tracing::info!(
                total = records.len(),
                "completed IP-country resolution for proxy records during add_or_update"
            );
        }

        records
    }

    /// Removes a proxy record by its normalised authority.
    ///
    /// The persisted record, when a persistence store is configured, is deleted
    /// as well.
    pub async fn remove(&self, authority: &str) {
        let _commit = self.persistence_commits.lock().await;
        let removed_key = {
            let mut inner = self.inner.write().await;
            inner.records.remove(authority).map(|record| {
                if let Some(ref country) = record.country {
                    if let Some(keys) = inner.country_records.get_mut(country) {
                        keys.retain(|k| k != authority);
                    }
                }
                for preference in inner.preferred_by_destination.values_mut() {
                    preference
                        .endpoints
                        .retain(|item| item.endpoint != record.key());
                }
                inner
                    .affinity_bindings
                    .retain(|_, binding| binding.authority != authority);
                record.key()
            })
        };
        #[cfg(feature = "persistence")]
        if let Some(key) = removed_key {
            self.delete_persisted_record(&key).await;
            if let Some(repository) = &self.preferences {
                if let Err(error) = repository.remove_endpoint(&key).await {
                    tracing::warn!(%error, endpoint = %authority, "failed to clean removed proxy preferences");
                }
            }
        }
        #[cfg(not(feature = "persistence"))]
        let _ = removed_key;
    }

    /// Returns a snapshot of all stored records.
    pub async fn all_records(&self) -> Vec<ProxyRecord> {
        self.inner.read().await.records.values().cloned().collect()
    }

    /// Returns records for a specific country.
    pub async fn records_for_country(&self, country: &str) -> Vec<ProxyRecord> {
        let inner = self.inner.read().await;
        inner
            .country_records
            .get(country)
            .map(|keys| {
                keys.iter()
                    .filter_map(|k| inner.records.get(k).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns the countries that have at least one record.
    pub async fn known_countries(&self) -> Vec<String> {
        let mut countries: Vec<String> = self
            .inner
            .read()
            .await
            .country_records
            .keys()
            .cloned()
            .collect();
        countries.sort();
        countries
    }
}
