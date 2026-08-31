//! In-memory persistence store used as a default and for tests.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde_json::{Map, Value};

use super::{
    PersistedRecord, PersistenceBackend, PersistenceKey, PersistenceStore, PersistenceTransaction,
    record_matches_filters,
};

/// Shared state of the in-memory backend.
#[derive(Debug, Default)]
struct MemoryRecords {
    records: RwLock<HashMap<PersistenceKey, PersistedRecord>>,
}

impl MemoryRecords {
    /// Locks the record map for reading.
    ///
    /// # Errors
    ///
    /// Returns an error when the lock was poisoned.
    fn read(&self) -> anyhow::Result<std::sync::RwLockReadGuard<'_, HashMap<PersistenceKey, PersistedRecord>>> {
        self.records
            .read()
            .map_err(|_| anyhow::anyhow!("memory persistence store lock was poisoned"))
    }

    /// Locks the record map for writing.
    ///
    /// # Errors
    ///
    /// Returns an error when the lock was poisoned.
    fn write(
        &self,
    ) -> anyhow::Result<std::sync::RwLockWriteGuard<'_, HashMap<PersistenceKey, PersistedRecord>>>
    {
        self.records
            .write()
            .map_err(|_| anyhow::anyhow!("memory persistence store lock was poisoned"))
    }
}

/// Legacy in-memory [`PersistenceStore`] implementation.
///
/// Preserves the behavior of a system without external persistence. Useful
/// as a default value and for deterministic tests. `commit` is a no-op:
/// writes are visible immediately.
#[derive(Debug, Default)]
pub struct LegacyMemoryPersistenceStore {
    records: Arc<MemoryRecords>,
}

impl LegacyMemoryPersistenceStore {
    /// Creates an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl PersistenceStore for LegacyMemoryPersistenceStore {
    async fn transaction(&self, namespace: &str) -> anyhow::Result<PersistenceTransaction> {
        Ok(PersistenceTransaction::new(
            namespace.to_string(),
            self.records.clone(),
        ))
    }
}

#[async_trait]
impl PersistenceBackend for MemoryRecords {
    async fn get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<PersistedRecord>> {
        let records = self.read()?;
        Ok(records
            .get(&PersistenceKey {
                namespace: namespace.to_string(),
                key: key.to_string(),
            })
            .cloned())
    }

    async fn put(
        &self,
        namespace: &str,
        key: String,
        record: PersistedRecord,
    ) -> anyhow::Result<()> {
        let mut records = self.write()?;
        records.insert(
            PersistenceKey {
                namespace: namespace.to_string(),
                key,
            },
            record,
        );
        Ok(())
    }

    async fn delete(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
        let mut records = self.write()?;
        records.remove(&PersistenceKey {
            namespace: namespace.to_string(),
            key: key.to_string(),
        });
        Ok(())
    }

    async fn find_by_fields(
        &self,
        namespace: &str,
        filters: &Map<String, Value>,
    ) -> anyhow::Result<Vec<(String, PersistedRecord)>> {
        let records = self.read()?;
        Ok(records
            .iter()
            .filter(|(key, record)| {
                key.namespace == namespace && record_matches_filters(record.fields.as_ref(), filters)
            })
            .map(|(key, record)| (key.key.clone(), record.clone()))
            .collect())
    }

    async fn commit(&self, _namespace: &str) -> anyhow::Result<()> {
        Ok(())
    }
}
