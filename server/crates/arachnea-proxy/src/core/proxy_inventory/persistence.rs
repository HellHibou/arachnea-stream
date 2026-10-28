//! Persistent inventory cache loading and serialized record commits.

use futures::FutureExt;
use std::sync::Arc;
use std::time::SystemTime;

use super::ProxyInventory;
use crate::core::{Destination, ProxyRecord, ProxyRepository};

/// Writes `records` through the typed proxy repository.
///
/// # Arguments
/// * `persistence` - Repository used to load and persist dynamic proxy records.
/// * `records` - Proxy records to merge or persist; endpoint identity is host and port.
///
#[cfg(feature = "persistence")]
async fn persist_proxy_records(
    persistence: &Arc<dyn ProxyRepository>,
    records: &[ProxyRecord],
) -> anyhow::Result<()> {
    persistence.save_many(records).await
}

impl ProxyInventory {
    /// Persists records through one transaction committed after the whole
    /// batch has been written into the namespace cache.
    ///
    /// # Arguments
    /// * `records` - Proxy records to merge or persist; endpoint identity is host and port.
    ///
    #[cfg(feature = "persistence")]
    pub(super) async fn persist_records(&self, records: &[ProxyRecord]) {
        let Some(persistence) = &self.persistence else {
            return;
        };
        #[cfg(feature = "performance-diagnostics")]
        let queued = std::time::Instant::now();
        let _guard = self.persistence_commits.lock().await;
        #[cfg(feature = "performance-diagnostics")]
        let waiting_ms = queued.elapsed().as_millis();
        #[cfg(feature = "performance-diagnostics")]
        let waiting_us = queued.elapsed().as_micros();
        #[cfg(feature = "performance-diagnostics")]
        let started = std::time::Instant::now();
        let current = {
            let inner = self.inner.read().await;
            records
                .iter()
                .filter_map(|record| inner.records.get(&record.authority()).cloned())
                .collect::<Vec<_>>()
        };
        if let Err(error) = persist_proxy_records(persistence, &current).await {
            tracing::warn!(%error, count = records.len(), "failed to persist proxy records");
        }
        #[cfg(feature = "performance-diagnostics")]
        tracing::debug!(
            search_id = crate::core::ProxySearchContext::current().map(|search| search.id()),
            waiting_ms,
            waiting_us,
            duration_ms = started.elapsed().as_millis(),
            duration_us = started.elapsed().as_micros(),
            count = current.len(),
            "committed proxy inventory records"
        );
    }

    /// Persists acceptance without letting the repository's conservative failure
    /// merge restore the superseded exclusion. Both writes use the latest state.
    ///
    /// # Arguments
    /// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
    /// * `destination` - Final origin used for capability checks and destination-specific exclusions.
    /// * `accepted_at` - Time of same-origin acceptance; only strictly older eligible failures are cleared.
    ///
    #[cfg(feature = "persistence")]
    pub(super) async fn persist_accepted_record(
        &self,
        record: &ProxyRecord,
        destination: &Destination,
        accepted_at: SystemTime,
    ) {
        let Some(persistence) = &self.persistence else {
            return;
        };
        let _guard = self.persistence_commits.lock().await;
        let current = self
            .inner
            .read()
            .await
            .records
            .get(&record.authority())
            .cloned();
        let Some(current) = current else {
            return;
        };
        if let Err(error) = persistence.save_many(std::slice::from_ref(&current)).await {
            tracing::warn!(%error, "failed to persist accepted proxy record");
            return;
        }
        if let Err(error) = persistence
            .clear_superseded_destination_failures(&current.key(), destination, accepted_at)
            .await
        {
            tracing::warn!(%error, "failed to persist superseded proxy exclusion repair");
        }
    }

    /// Deletes the persisted record of an endpoint key, if persistence is
    /// configured.
    ///
    /// # Arguments
    /// * `key` - Endpoint identity used to locate the stored proxy record.
    ///
    #[cfg(feature = "persistence")]
    pub(super) async fn delete_persisted_record(&self, key: &crate::core::ProxyKey) {
        let Some(persistence) = &self.persistence else {
            return;
        };
        let result = persistence.delete(key).await;
        if let Err(error) = result {
            tracing::warn!(host = %key.host, port = key.port, %error, "failed to delete persisted proxy record");
        }
    }

    /// Imports cached records for `country` from the persistence store into
    /// the in-memory inventory.
    ///
    /// # Arguments
    /// * `country` - ISO alpha-2 country code associated with the requested inventory partition.
    ///
    /// # Returns
    ///
    /// `true` when at least one valid cached record was imported.
    #[cfg(feature = "persistence")]
    pub(super) async fn load_cached_country(&self, country: &str) -> bool {
        if self.persistence.is_none() {
            return false;
        }
        let (operation, _joined) = {
            let mut imports = self.country_imports.lock().await;
            imports.retain(|_, pending| pending.upgrade().is_some());
            if let Some(pending) = imports.get(country).and_then(|pending| pending.upgrade()) {
                (pending, true)
            } else {
                let inventory = self.clone();
                let country_key = country.to_string();
                let pending = crate::core::ProxySearchContext::unscoped(async move {
                    let imported = inventory.import_cached_country(&country_key).await;
                    inventory.country_imports.lock().await.remove(&country_key);
                    imported
                })
                .boxed()
                .shared();
                imports.insert(
                    country.to_string(),
                    pending.downgrade().expect("live country import"),
                );
                (pending, false)
            }
        };
        #[cfg(feature = "performance-diagnostics")]
        let started = std::time::Instant::now();
        let imported = operation.await;
        #[cfg(feature = "performance-diagnostics")]
        tracing::debug!(
            search_id = crate::core::ProxySearchContext::current().map(|search| search.id()),
            country,
            joined = _joined,
            imported,
            duration_us = started.elapsed().as_micros(),
            "finished shared persistent proxy country import"
        );
        imported
    }

    /// Reads and merges one country once for the currently subscribed selections.
    ///
    /// # Arguments
    /// - `country`: Country shared by callers on this repository instance.
    async fn import_cached_country(&self, country: &str) -> bool {
        let Some(persistence) = &self.persistence else {
            return false;
        };
        #[cfg(feature = "performance-diagnostics")]
        let started = std::time::Instant::now();
        #[cfg(feature = "performance-diagnostics")]
        let search_id = crate::core::ProxySearchContext::current().map(|search| search.id());
        let result = persistence.find_by_country(country).await;
        #[cfg(feature = "performance-diagnostics")]
        let read_us = started.elapsed().as_micros();
        let converted = match result {
            Ok(converted) => converted,
            Err(error) => {
                #[cfg(feature = "performance-diagnostics")]
                tracing::debug!(
                    search_id,
                    country,
                    read_us,
                    outcome = "error",
                    "finished persistent proxy country read"
                );
                tracing::warn!(country = %country, %error, "failed to read cached proxy records");
                return false;
            }
        };
        #[cfg(feature = "performance-diagnostics")]
        tracing::debug!(
            search_id,
            country,
            read_us,
            count = converted.len(),
            outcome = "success",
            "finished persistent proxy country read"
        );
        if converted.is_empty() {
            return false;
        }
        #[cfg(feature = "performance-diagnostics")]
        let count = converted.len();
        #[cfg(feature = "performance-diagnostics")]
        let merge_started = std::time::Instant::now();
        self.add_or_update(converted).await;
        #[cfg(feature = "performance-diagnostics")]
        tracing::debug!(search_id, country = %country, count, read_us,
            merge_us = merge_started.elapsed().as_micros(), duration_us = started.elapsed().as_micros(),
            "loaded cached proxy records from persistence");
        true
    }
}
