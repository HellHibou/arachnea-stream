//! Progressive provider loading and country refresh coordination.

use std::sync::Arc;
use std::time::{Instant, SystemTime};

use futures::future::WeakShared;
use futures::FutureExt;
use tokio::sync::Mutex;

use super::ProxyInventory;
use crate::core::{Destination, ProxyRecord, ProxySearchContext, Result};

use super::eligibility::is_selection_candidate;
#[cfg(feature = "performance-diagnostics")]
use super::performance_diagnostics::{ProviderLoadObservation, ProviderLoadProgress};
use super::records::prepare_provider_records;
/// Provider callback that incrementally merges arriving records into the shared inventory.
struct InventoryBatchSink {
    inventory: ProxyInventory,
    #[cfg(feature = "performance-diagnostics")]
    country: String,
    #[cfg(feature = "performance-diagnostics")]
    progress: Arc<std::sync::Mutex<ProviderLoadProgress>>,
}

#[async_trait::async_trait]
impl crate::core::proxy_record::ProxyBatchSink for InventoryBatchSink {
    async fn publish(&self, records: Vec<ProxyRecord>) -> Result<()> {
        #[cfg(feature = "performance-diagnostics")]
        if let Ok(mut progress) = self.progress.lock() {
            progress.received_batches += 1;
            progress.received_records += records.len();
        }
        self.inventory
            .merge_provider_batch(
                #[cfg(feature = "performance-diagnostics")]
                &self.country,
                records,
                #[cfg(feature = "performance-diagnostics")]
                &self.progress,
            )
            .await;
        Ok(())
    }
}

impl ProxyInventory {
    /// Ranks countries by cached candidate availability before provider discovery.
    ///
    /// # Arguments
    /// * `countries` - Allowed ISO alpha-2 country codes.
    /// * `require_https` - Whether selection requires an HTTPS tunnel and destination TLS capability.
    /// * `destination` - Final origin used for capability checks and destination-specific exclusions.
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
    ///
    /// # Arguments
    /// * `country` - ISO alpha-2 country code associated with the requested inventory partition.
    ///
    pub(super) async fn load_if_needed(&self, country: &str) {
        let search = ProxySearchContext::current();
        let search_id = search.as_ref().map(ProxySearchContext::id);
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
                    tracing::debug!(
                        search_id,
                        country,
                        joined = true,
                        "subscribed to shared proxy provider load"
                    );
                    existing
                } else {
                    tracing::debug!(
                        search_id,
                        country,
                        joined = false,
                        "subscribed to shared proxy provider load"
                    );
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

    /// Refreshes one country through its provider under a single-flight lock and refresh cooldown.
    ///
    /// # Arguments
    /// * `country` - ISO alpha-2 country code associated with the requested inventory partition.
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

        tracing::debug!(
            country = %country,
            cooldown_seconds = self.config.provider_refresh_cooldown.as_secs(),
            "refreshing dynamic proxy provider during candidate exploration"
        );

        #[cfg(feature = "performance-diagnostics")]
        let started = std::time::Instant::now();
        #[cfg(feature = "performance-diagnostics")]
        let progress = Arc::new(std::sync::Mutex::new(ProviderLoadProgress::default()));
        #[cfg(feature = "performance-diagnostics")]
        let mut observation = ProviderLoadObservation {
            country: country.to_string(),
            started,
            outcome: "cancelled",
            progress: progress.clone(),
        };
        let result = provider
            .load_proxies_progressively(
                crate::core::ProxyLoadRequest {
                    countries: vec![country.to_string()],
                },
                Arc::new(InventoryBatchSink {
                    inventory: self.clone(),
                    #[cfg(feature = "performance-diagnostics")]
                    country: country.to_string(),
                    #[cfg(feature = "performance-diagnostics")]
                    progress,
                }),
            )
            .await;
        #[cfg(feature = "performance-diagnostics")]
        {
            observation.outcome = if result.is_ok() {
                "completed"
            } else {
                "failed"
            };
        }

        if let Err(error) = result {
            tracing::warn!(country, %error, "progressive proxy load stopped; published batches retained");
        }
    }

    /// Deduplicates a provider batch and merges it while retaining valid cached runtime evidence.
    ///
    /// # Arguments
    /// * `country` - ISO alpha-2 country code associated with the requested inventory partition.
    /// * `records` - Proxy records to merge or persist; endpoint identity is host and port.
    /// * `progress` - Diagnostics-only counters updated as provider records arrive.
    async fn merge_provider_batch(
        &self,
        #[cfg(feature = "performance-diagnostics")] country: &str,
        records: Vec<ProxyRecord>,
        #[cfg(feature = "performance-diagnostics")] progress: &std::sync::Mutex<
            ProviderLoadProgress,
        >,
    ) {
        #[cfg(feature = "performance-diagnostics")]
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
        let (records, _probe_indices, _duplicate_count, _cached_count) =
            prepare_provider_records(records, &existing, SystemTime::now(), self.config.probe_ttl);
        #[cfg(feature = "performance-diagnostics")]
        self.log_prepared_provider_batch(
            country,
            &records,
            &existing,
            loaded_count,
            _probe_indices.len(),
            _duplicate_count,
            _cached_count,
        );
        let records_to_persist = self.add_or_update(records).await;
        #[cfg(feature = "performance-diagnostics")]
        if let Ok(mut progress) = progress.lock() {
            progress.merged_batches += 1;
            progress.merged_records += records_to_persist.len();
            progress.first_merge.get_or_insert_with(Instant::now);
        }

        #[cfg(feature = "performance-diagnostics")]
        self.log_updated_inventory(country).await;
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
