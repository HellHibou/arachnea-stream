//! Contextual health updates, accepted responses and failure-driven eviction.

#[cfg(feature = "persistence")]
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};

use super::{ProxyInventory, PROXY_TRANSIENT_FAILURE_COOLDOWN};
use crate::core::{Destination, ProxyRecord, ProxyRuntimeStatus, PROXY_CACHE_TTL};

use super::eligibility::destination_scheme;

fn proxy_validation_is_stale(record: &ProxyRecord, now: SystemTime) -> bool {
    let Some(last_validated_at) = record
        .health_evidence
        .last_positive_at
        .or(record.health_evidence.first_discovered_at)
    else {
        return false;
    };

    now.duration_since(last_validated_at)
        .map(|age| age > PROXY_CACHE_TTL)
        .unwrap_or(false)
}

/// Removes only ambiguous failures strictly older than acceptance by the same origin.
pub(in crate::core) fn clear_superseded_destination_failures(
    record: &mut ProxyRecord,
    destination: &Destination,
    accepted_at: SystemTime,
) -> bool {
    let previous_len = record.destination_failures.len();
    let scheme = destination_scheme(destination);
    let host = destination.host_for_protocol();
    record.destination_failures.retain(|failure| {
        !(failure.reason == crate::core::ProxyDestinationFailureReason::Other
            && failure.scheme == scheme
            && failure.host == host
            && failure.port == destination.port
            && failure.last_failed < accepted_at)
    });
    record.destination_failures.len() != previous_len
}

impl ProxyInventory {
    // ── Failure tracking ─────────────────────────────────────────────

    /// Stores a contextual phase observation without synthesizing global failure.
    ///
    /// # Arguments
    /// - `authority`: Endpoint to which the observation belongs.
    /// - `observation`: Completed phase, never a synthetic cancellation failure.
    pub async fn record_observation(
        &self,
        authority: &str,
        observation: crate::core::ProxyObservation,
    ) {
        let updated = {
            let mut inner = self.inner.write().await;
            let Some(record) = inner.records.get_mut(authority) else {
                return;
            };
            record.record_observation(observation);
            record.clone()
        };
        #[cfg(feature = "persistence")]
        self.persist_records(std::slice::from_ref(&updated)).await;
        #[cfg(not(feature = "persistence"))]
        drop(updated);
    }

    /// Records a destination-scoped failure without globally degrading or evicting the proxy.
    ///
    /// # Arguments
    /// - `authority`: Endpoint actually used by the attempt.
    /// - `destination`: Exact destination of the failed exchange.
    /// - `reason`: Contextual failure category.
    ///
    /// # Returns
    /// Always `false`; retained for compatibility with the former promotion API.
    pub async fn record_destination_failure(
        &self,
        authority: &str,
        destination: &Destination,
        reason: crate::core::ProxyDestinationFailureReason,
    ) -> bool {
        self.record_destination_failure_with_cooldown(authority, destination, reason, None)
            .await
    }

    /// Records a destination failure without shortening an explicit origin rejection.
    /// Ambiguous failures use at most 30 seconds, including legacy stored exclusions
    /// when determining eligibility. Shorter configured limits remain effective.
    ///
    /// # Arguments
    /// - `authority`: Endpoint actually used.
    /// - `destination`: Exact observed destination.
    /// - `reason`: Contextual failure category.
    /// - `cooldown`: Optional override of the inventory default.
    pub async fn record_destination_failure_with_cooldown(
        &self,
        authority: &str,
        destination: &Destination,
        reason: crate::core::ProxyDestinationFailureReason,
        cooldown: Option<Duration>,
    ) -> bool {
        let cooldown = cooldown.unwrap_or(self.config.destination_failure_cooldown);
        let cooldown = if reason == crate::core::ProxyDestinationFailureReason::Other {
            cooldown.min(PROXY_TRANSIENT_FAILURE_COOLDOWN)
        } else {
            cooldown
        };
        let updated = {
            let mut inner = self.inner.write().await;
            let Some(record) = inner.records.get_mut(authority) else {
                return false;
            };
            let now = SystemTime::now();
            let host = destination.host_for_protocol();
            let scheme = destination_scheme(destination);
            if let Some(failure) = record.destination_failures.iter_mut().find(|failure| {
                failure.scheme == scheme && failure.host == host && failure.port == destination.port
            }) {
                failure.failure_count = failure.failure_count.saturating_add(1);
                failure.last_failed = now;
                if failure.reason != crate::core::ProxyDestinationFailureReason::BlockedByOrigin
                    || reason == crate::core::ProxyDestinationFailureReason::BlockedByOrigin
                {
                    failure.reason = reason;
                }
                failure.cooldown_until = failure.cooldown_until.max(Some(
                    now.checked_add(cooldown)
                        .unwrap_or(now + self.config.destination_failure_cooldown),
                ));
            } else {
                record
                    .destination_failures
                    .push(crate::core::ProxyDestinationFailure {
                        scheme,
                        host,
                        port: destination.port,
                        reason,
                        failure_count: 1,
                        last_failed: now,
                        cooldown_until: Some(
                            now.checked_add(cooldown)
                                .unwrap_or(now + self.config.destination_failure_cooldown),
                        ),
                    });
            }
            record.clone()
        };
        #[cfg(feature = "persistence")]
        self.persist_records(std::slice::from_ref(&updated)).await;
        #[cfg(not(feature = "persistence"))]
        drop(updated);
        false
    }

    /// Records a proven endpoint connection failure, with cooldown or age-based eviction.
    ///
    /// # Arguments
    /// * `authority` - Endpoint whose own connection failed, not a remote tunnel or TLS failure.
    pub async fn record_global_failure(&self, authority: &str) {
        if self.remove_if_stale(authority).await {
            return;
        }
        let updated = {
            let mut inner = self.inner.write().await;
            if let Some(record) = inner.records.get_mut(authority) {
                let now = SystemTime::now();
                record.status = ProxyRuntimeStatus::Ko;
                record.failure_count = record.failure_count.saturating_add(1);
                record.tcp_reachable = Some(false);
                record.cooldown_until = Some(now + self.config.ko_cooldown);
                Some(record.clone())
            } else {
                None
            }
        };
        if let Some(record) = updated {
            #[cfg(feature = "persistence")]
            self.persist_records(std::slice::from_ref(&record)).await;
            #[cfg(not(feature = "persistence"))]
            drop(record);
        }
    }

    /// Records that a proxy requires authentication.
    pub async fn record_auth_required(&self, authority: &str) {
        let updated = {
            let mut inner = self.inner.write().await;
            if let Some(record) = inner.records.get_mut(authority) {
                record.status = ProxyRuntimeStatus::AuthenticationRequired;
                record.authentication_required = Some(true);
                record.failure_count += 1;
                Some(record.clone())
            } else {
                None
            }
        };
        if let Some(record) = updated {
            #[cfg(feature = "persistence")]
            self.persist_records(std::slice::from_ref(&record)).await;
            #[cfg(not(feature = "persistence"))]
            drop(record);
        }
    }

    /// Records a successful proxy use and renews its 24-hour validation age.
    pub async fn record_ok(&self, authority: &str) {
        let updated = {
            let mut inner = self.inner.write().await;
            if let Some(record) = inner.records.get_mut(authority) {
                record.status = ProxyRuntimeStatus::Ok;
                record.failure_count = 0;
                record.cooldown_until = None;
                record.last_validated_at = Some(SystemTime::now());
                record.health_evidence.last_positive_at = record
                    .health_evidence
                    .last_positive_at
                    .max(record.last_validated_at);
                Some(record.clone())
            } else {
                None
            }
        };
        if let Some(record) = updated {
            #[cfg(feature = "persistence")]
            self.persist_records(std::slice::from_ref(&record)).await;
            #[cfg(not(feature = "persistence"))]
            drop(record);
        }
    }

    /// Renews the validation age after an accepted origin response and records
    /// the proxy as the preferred candidate for that exact destination. Clears
    /// older ambiguous failures for this association, preserving explicit origin
    /// rejections, newer failures and unrelated proxy or destination failure state.
    ///
    /// # Arguments
    /// - `authority`: Endpoint actually used by the accepted response.
    /// - `destination`: Exact origin that accepted the response.
    pub async fn record_validated_response(&self, authority: &str, destination: &Destination) {
        let accepted_at = SystemTime::now();
        let key = crate::core::ProxyDestinationPreference::destination_key(destination);
        let lock = self.destination_lock(&key).await;
        let _guard = lock.lock().await;
        #[cfg(feature = "persistence")]
        if !self
            .inner
            .read()
            .await
            .preferred_by_destination
            .contains_key(&key)
        {
            if let Some(repository) = &self.preferences {
                match repository.get(&key).await {
                    Ok(Some(preference)) => {
                        self.inner
                            .write()
                            .await
                            .preferred_by_destination
                            .insert(key.clone(), preference);
                    }
                    Err(error) => {
                        tracing::warn!(%error, destination = %key, "failed to load proxy history before accepted response")
                    }
                    _ => {}
                }
            }
        }
        let updated = {
            let mut inner = self.inner.write().await;
            let updated = inner.records.get_mut(authority).map(|record| {
                clear_superseded_destination_failures(record, destination, accepted_at);
                record.last_validated_at = record.last_validated_at.max(Some(accepted_at));
                record.health_evidence.last_positive_at = record
                    .health_evidence
                    .last_positive_at
                    .max(record.last_validated_at);
                record.clone()
            });
            if let Some(record) = &updated {
                inner
                    .preferred_by_destination
                    .entry(key.clone())
                    .or_insert_with(|| crate::core::ProxyDestinationPreference {
                        destination: key.clone(),
                        endpoints: Vec::new(),
                    })
                    .record_success(record.key(), accepted_at);
            }
            updated
        };
        if let Some(record) = updated {
            #[cfg(feature = "persistence")]
            {
                self.persist_accepted_record(&record, destination, accepted_at)
                    .await;
                let _commit = self.persistence_commits.lock().await;
                if let Some(repository) = &self.preferences {
                    let preference =
                        {
                            let inner = self.inner.read().await;
                            inner.preferred_by_destination.get(&key).cloned().map(
                                |mut preference| {
                                    preference.endpoints.retain(|item| {
                                        inner
                                            .records
                                            .values()
                                            .any(|record| record.key() == item.endpoint)
                                    });
                                    preference
                                },
                            )
                        };
                    if let Some(preference) = preference {
                        if let Err(error) = repository.save(&preference).await {
                            self.preference_persistence_failures
                                .fetch_add(1, Ordering::Relaxed);
                            tracing::error!(%error, destination = %key, endpoint = %authority,
                                "failed to persist accepted proxy preference; response and memory history preserved");
                        }
                    }
                }
            }
            #[cfg(not(feature = "persistence"))]
            drop(record);
            self.notify_destination_success();
        }
    }

    /// Evicts only on an observed endpoint connection failure after the retention threshold.
    async fn remove_if_stale(&self, authority: &str) -> bool {
        let _commit = self.persistence_commits.lock().await;
        let removed_record = {
            let mut inner = self.inner.write().await;
            let stale = inner
                .records
                .get(authority)
                .is_some_and(|record| proxy_validation_is_stale(record, SystemTime::now()));
            if !stale {
                return false;
            }
            let mut removed = inner.records.remove(authority);
            if let Some(record) = &mut removed {
                record.health_evidence.evicted_at = Some(SystemTime::now());
                record.cooldown_until = Some(SystemTime::now() + self.config.ko_cooldown);
                record.status = ProxyRuntimeStatus::Ko;
                record.tcp_reachable = Some(false);
                inner.evicted.insert(authority.to_string(), record.clone());
                if inner.evicted.len() > 1024 {
                    if let Some(oldest) = inner
                        .evicted
                        .iter()
                        .min_by_key(|(_, record)| record.health_evidence.evicted_at)
                        .map(|(key, _)| key.clone())
                    {
                        inner.evicted.remove(&oldest);
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
            }
            if let Some(record) = &removed {
                if let Some(country) = &record.country {
                    if let Some(keys) = inner.country_records.get_mut(country) {
                        keys.retain(|key| key != authority);
                    }
                }
            }
            removed
        };

        #[cfg(feature = "persistence")]
        if let Some(record) = removed_record {
            if let Some(repository) = &self.persistence {
                if let Err(error) = repository.evict(&record).await {
                    tracing::error!(%error, endpoint = %authority, "failed to persist proxy endpoint eviction");
                }
            }
            if let Some(repository) = &self.preferences {
                if let Err(error) = repository.remove_endpoint(&record.key()).await {
                    tracing::warn!(%error, endpoint = %authority, "failed to clean evicted proxy preferences");
                }
            }
        }
        #[cfg(not(feature = "persistence"))]
        let _ = removed_record;

        tracing::debug!(
            authority,
            "removed stale dynamic proxy after observed failure"
        );
        true
    }
}
