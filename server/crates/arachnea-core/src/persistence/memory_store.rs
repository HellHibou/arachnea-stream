//! In-memory persistence store used as a default and for tests.

use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;

use super::{PersistedRecord, PersistenceKey, PersistenceStore};

/// In-memory [`PersistenceStore`] implementation.
///
/// Preserves the behavior of a system without external persistence. Useful
/// as a default value and for deterministic tests.
#[derive(Debug, Default)]
pub struct MemoryPersistenceStore {
    records: RwLock<HashMap<PersistenceKey, PersistedRecord>>,
}

impl MemoryPersistenceStore {
    /// Creates an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl PersistenceStore for MemoryPersistenceStore {
    async fn get(&self, key: &PersistenceKey) -> anyhow::Result<Option<PersistedRecord>> {
        let records = self
            .records
            .read()
            .map_err(|_| anyhow::anyhow!("memory persistence store lock was poisoned"))?;
        Ok(records.get(key).cloned())
    }

    async fn put(&self, key: PersistenceKey, record: PersistedRecord) -> anyhow::Result<()> {
        let mut records = self
            .records
            .write()
            .map_err(|_| anyhow::anyhow!("memory persistence store lock was poisoned"))?;
        records.insert(key, record);
        Ok(())
    }

    async fn delete(&self, key: &PersistenceKey) -> anyhow::Result<()> {
        let mut records = self
            .records
            .write()
            .map_err(|_| anyhow::anyhow!("memory persistence store lock was poisoned"))?;
        records.remove(key);
        Ok(())
    }
}
