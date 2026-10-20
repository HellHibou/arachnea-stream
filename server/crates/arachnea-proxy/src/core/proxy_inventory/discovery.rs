//! Progressive provider loading and country refresh coordination.

use std::sync::Arc;
use std::time::{Instant, SystemTime};

use futures::future::WeakShared;
use futures::FutureExt;
use tokio::sync::Mutex;

use super::ProxyInventory;
use crate::core::{Destination, ProxyRecord, ProxySearchContext, Result};

use super::eligibility::{is_selection_candidate, selection_stats};
use super::records::{
    declared_protocol_count, prepare_provider_records, runtime_protocol_is_undeclared,
};

struct InventoryBatchSink {
    inventory: ProxyInventory,
    country: String,
}

#[async_trait::async_trait]
impl crate::core::proxy_record::ProxyBatchSink for InventoryBatchSink {
    async fn publish(&self, records: Vec<ProxyRecord>) -> Result<()> {
        self.inventory
            .merge_provider_batch(&self.country, records)
            .await;
        Ok(())
    }
}

impl ProxyInventory {
    pub(super) async fn discovery_order(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Vec<String> {
        let inner = self.inner.read().await;
        let now = SystemTime::now();
        let mut ranked: Vec<_> = countries
            .iter()
            .map(|country| {
                let usable = inner
                    .country_records
                    .get(country)
                    .into_iter()
                    .flatten()
                    .filter_map(|key| inner.records.get(key))
                    .filter(|record| {
                        is_selection_candidate(
                            record,
                            country,
                            require_https,
                            destination,
                            now,
                            self.probe_mode(),
                        )
                    })
                    .count();
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                use std::hash::{Hash, Hasher};
                country.hash(&mut hash);
                (
                    country.clone(),
                    usable,
                    inner.country_last_refresh_attempts.get(country).copied(),
                    hash.finish(),
                )
            })
            .collect();
        ranked.sort_by_key(|(_, usable, refreshed, tie)| (*usable, *refreshed, *tie));
        ranked
            .into_iter()
            .map(|(country, _, _, _)| country)
            .collect()
    }

    /// Loads proxies from the provider after selection found no eligible cached
    /// candidate.
    ///
    /// Calls for the same country share a per-country asynchronous lock and a
    /// refresh-attempt cooldown. One call performs the network work while
    /// concurrent or subsequent calls reuse inventory state until the cooldown
    /// expires, including when the provider failed or returned no candidates.
    /// Provider records are deduplicated and stored without eager batch probing.
    /// Selection runs prechecks and full validations progressively within the shared deadline.
    /// The shared refresh owns a child task to avoid extending the caller's HTTP
    /// polling stack; dropping its last subscriber aborts that task.
    pub(super) async fn load_if_needed(&self, country: &str) {
        let search = ProxySearchContext::current();
        if search.as_ref().is_some_and(|search| !search.can_discover()) {
            return;
        }
        let wait = async {
            let _local = if let Some(search) = &search {
                Some(search.provider_slots.acquire().await.ok()?)
            } else {
                None
            };
            let key = format!("provider:{country}");
            let shared = {
                let mut registry = self.shared_work.lock().await;
                registry.retain(|_, weak| weak.upgrade().is_some());
                if let Some(existing) = registry.get(&key).and_then(WeakShared::upgrade) {
                    existing
                } else {
                    let inventory = self.clone();
                    let country = country.to_string();
                    let future = async move {
                        // The owner breaks the nested HTTP polling stack and aborts
                        // the child when the last shared subscriber disappears.
                        let mut tasks = tokio::task::JoinSet::new();
                        let load_country = country.clone();
                        tasks.spawn(ProxySearchContext::unscoped(async move {
                            let Ok(_slot) = inventory.provider_slots.acquire().await else {
                                return;
                            };
                            inventory.load_country(&load_country).await;
                        }));
                        if let Some(Err(error)) = tasks.join_next().await {
                            tracing::warn!(
                                country,
                                panicked = error.is_panic(),
                                "dynamic proxy provider task failed"
                            );
                        }
                        None
                    }
                    .boxed()
                    .shared();
                    registry.insert(key, future.downgrade()?);
                    future
                }
            };
            shared.await
        };
        if let Some(search) = &search {
            let _ = search.technical(wait).await;
        } else {
            wait.await;
        }
    }

    async fn load_country(&self, country: &str) {
        let provider = match &self.provider {
            Some(p) => p,
            None => return,
        };

        let load_lock = {
            let mut inner = self.inner.write().await;
            Arc::clone(
                inner
                    .country_load_locks
                    .entry(country.to_string())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let _load_guard = load_lock.lock().await;

        {
            let mut inner = self.inner.write().await;
            let now = Instant::now();
            if let Some(last_attempt) = inner.country_last_refresh_attempts.get(country) {
                let elapsed = now.duration_since(*last_attempt);
                let cooldown = if inner.country_early_refresh.contains(country) {
                    self.config.provider_min_refresh_interval
                } else {
                    self.config
                        .provider_refresh_cooldown
                        .max(self.config.provider_min_refresh_interval)
                };
                if elapsed < cooldown {
                    tracing::debug!(
                        country = %country,
                        elapsed_ms = elapsed.as_millis(),
                        cooldown_ms = cooldown.as_millis(),
                        "dynamic proxy provider refresh suppressed by inventory cooldown"
                    );
                    return;
                }
            }
            inner.country_early_refresh.remove(country);
            inner
                .country_last_refresh_attempts
                .insert(country.to_string(), now);
        }

        tracing::info!(
            country = %country,
            cooldown_seconds = self.config.provider_refresh_cooldown.as_secs(),
            "refreshing dynamic proxy provider during candidate exploration"
        );

        if let Err(error) = provider
            .load_proxies_progressively(
                crate::core::ProxyLoadRequest {
                    countries: vec![country.to_string()],
                },
                Arc::new(InventoryBatchSink {
                    inventory: self.clone(),
                    country: country.to_string(),
                }),
            )
            .await
        {
            tracing::warn!(country, %error, "progressive proxy load stopped; published batches retained");
        }
    }

    async fn merge_provider_batch(&self, country: &str, records: Vec<ProxyRecord>) {
        let loaded_count = records.len();
        let existing = {
            let inner = self.inner.read().await;
            records
                .iter()
                .filter_map(|record| {
                    let key = record.authority();
                    inner.records.get(&key).map(|cached| (key, cached.clone()))
                })
                .collect()
        };
        let (records, probe_indices, duplicate_count, cached_count) =
            prepare_provider_records(records, &existing, SystemTime::now(), self.config.probe_ttl);
        let pending_probe_count = probe_indices.len();
        let probe_stats = selection_stats(
            records.iter(),
            country,
            false,
            None,
            SystemTime::now(),
            self.config.probe_ttl,
            self.probe_mode(),
        );
        let protocol_conflict_count = records
            .iter()
            .filter(|record| declared_protocol_count(record) > 1)
            .count();
        let undeclared_protocol_count = records
            .iter()
            .filter(|record| runtime_protocol_is_undeclared(record))
            .count();
        tracing::info!(
            country = %country,
            loaded_count,
            deduplicated_count = records.len(),
            duplicate_count,
            protocol_conflict_count,
            undeclared_protocol_count,
            cached_count,
            pending_probe_count,
            ok = probe_stats.ok,
            unknown = probe_stats.unknown,
            ko = probe_stats.ko,
            auth_required = probe_stats.authentication_required,
            auth_flag = probe_stats.authentication_required_flag,
            http_forward_available = probe_stats.http_forward_available,
            http_forward_unavailable = probe_stats.http_forward_unavailable,
            http_forward_unknown = probe_stats.http_forward_unknown,
            https_tunnel_available = probe_stats.https_tunnel_available,
            https_tunnel_unavailable = probe_stats.https_tunnel_unavailable,
            https_tunnel_unknown = probe_stats.https_tunnel_unknown,
            destination_tls_available = probe_stats.destination_tls_available,
            destination_tls_unavailable = probe_stats.destination_tls_unavailable,
            destination_tls_unknown = probe_stats.destination_tls_unknown,
            proxy_tls_valid = probe_stats.proxy_tls_valid,
            proxy_tls_invalid = probe_stats.proxy_tls_invalid,
            proxy_tls_unknown = probe_stats.proxy_tls_unknown,
            "loaded dynamic proxies prepared for on-demand probing"
        );
        let records_to_persist = self.add_or_update(records).await;

        let http_stats = self
            .selection_stats_for_country(country, false)
            .await
            .unwrap_or_default();
        let https_stats = self
            .selection_stats_for_country(country, true)
            .await
            .unwrap_or_default();
        tracing::info!(
            country = %country,
            total = http_stats.total,
            ok = http_stats.ok,
            eligible_http = http_stats.eligible,
            eligible_https = https_stats.eligible,
            http_forward_available = http_stats.http_forward_available,
            http_forward_unavailable = http_stats.http_forward_unavailable,
            http_forward_unknown = http_stats.http_forward_unknown,
            https_tunnel_available = https_stats.https_tunnel_available,
            https_tunnel_unavailable = https_stats.https_tunnel_unavailable,
            https_tunnel_unknown = https_stats.https_tunnel_unknown,
            destination_tls_available = https_stats.destination_tls_available,
            destination_tls_unavailable = https_stats.destination_tls_unavailable,
            destination_tls_unknown = https_stats.destination_tls_unknown,
            https_rejected = https_stats.https_rejected,
            ko = http_stats.ko,
            auth_required = http_stats.authentication_required,
            cooldown = http_stats.cooldown,
            "dynamic proxy inventory updated"
        );

        #[cfg(feature = "persistence")]
        self.persist_records(&records_to_persist).await;
        #[cfg(not(feature = "persistence"))]
        let _ = records_to_persist;
        self.discovery_revision
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }

    /// Allows the provider to be consulted again after an observed proxy
    /// connection failure.
    ///
    /// This does not load any data itself. It shortens the per-country
    /// refresh-attempt cooldown to the configured minimum so a later selection may refresh the provider
    /// if all cached and persisted candidates have become ineligible. The
    /// failed proxy keeps its runtime exclusion and is therefore not revived by
    /// a provider response while its cached probe remains fresh.
    ///
    /// # Arguments
    ///
    /// - `countries`: Country codes associated with the failed dynamic pool.
    pub(crate) async fn allow_provider_refresh_after_observed_failure(&self, countries: &[String]) {
        let mut inner = self.inner.write().await;
        for country in countries {
            let country = country.trim();
            if country.is_empty() {
                continue;
            }
            inner.country_early_refresh.insert(country.to_string());
        }
    }
}
