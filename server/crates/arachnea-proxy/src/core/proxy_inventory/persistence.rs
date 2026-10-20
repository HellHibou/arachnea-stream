//! Persistent inventory cache loading and serialized record commits.

use std::sync::Arc;
use std::time::SystemTime;

use super::ProxyInventory;
use crate::core::{Destination, ProxyRecord, ProxyRepository};

/// Writes `records` through the typed proxy repository.
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
    #[cfg(feature = "persistence")]
    pub(super) async fn persist_records(&self, records: &[ProxyRecord]) {
        let Some(persistence) = &self.persistence else {
            return;
        };
        let queued = std::time::Instant::now();
        let _guard = self.persistence_commits.lock().await;
        let waiting_ms = queued.elapsed().as_millis();
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
        tracing::debug!(
            waiting_ms,
            duration_ms = started.elapsed().as_millis(),
            count = current.len(),
            "committed proxy inventory records"
        );
    }

    /// Persists acceptance without letting the repository's conservative failure
    /// merge restore the superseded exclusion. Both writes use the latest state.
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
    /// # Returns
    ///
    /// `true` when at least one valid cached record was imported.
    #[cfg(feature = "persistence")]
    pub(super) async fn load_cached_country(&self, country: &str) -> bool {
        let Some(persistence) = &self.persistence else {
            return false;
        };
        let result = persistence.find_by_country(country).await;
        let converted = match result {
            Ok(converted) => converted,
            Err(error) => {
                tracing::warn!(country = %country, %error, "failed to read cached proxy records");
                return false;
            }
        };
        if converted.is_empty() {
            return false;
        }
        let count = converted.len();
        self.add_or_update(converted).await;
        tracing::debug!(country = %country, count, "loaded cached proxy records from persistence");
        true
    }
}
