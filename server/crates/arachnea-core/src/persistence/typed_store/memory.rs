//! In-memory implementation of the typed entity store contract.

use async_trait::async_trait;
use std::collections::HashMap;

use super::{
    checked_schema, encode, expired, matches, EntityQuery, PersistenceStoreConfig,
    PersistentEntity, TypedEntityStore,
};

/// In-memory store holding entities directly, without JSON conversion.
pub struct MemoryEntityStore<E: PersistentEntity> {
    config: PersistenceStoreConfig,
    records: std::sync::RwLock<HashMap<E::Key, E>>,
}
impl<E: PersistentEntity> MemoryEntityStore<E> {
    /// Creates an empty store for the explicit configuration.
    ///
    /// # Arguments
    /// * `config` - Table identity and entity schema accepted by the store.
    ///
    /// # Errors
    /// Returns an error when the schema is invalid or does not match `E`.
    pub fn new(config: PersistenceStoreConfig) -> anyhow::Result<Self> {
        checked_schema::<E>(&config)?;
        Ok(Self {
            config,
            records: std::sync::RwLock::new(HashMap::new()),
        })
    }
}
#[async_trait]
impl<E: PersistentEntity> TypedEntityStore<E> for MemoryEntityStore<E> {
    async fn get(&self, key: &E::Key) -> anyhow::Result<Option<E>> {
        let mut records = self
            .records
            .write()
            .map_err(|_| anyhow::anyhow!("memory entity store lock was poisoned"))?;
        if let Some(entity) = records.get(key) {
            if expired(&self.config.schema, &encode(&self.config, entity)?)? {
                records.remove(key);
                return Ok(None);
            }
        }
        Ok(records.get(key).cloned())
    }
    async fn put(&self, entity: &E) -> anyhow::Result<()> {
        encode(&self.config, entity)?;
        self.records
            .write()
            .map_err(|_| anyhow::anyhow!("memory entity store lock was poisoned"))?
            .insert(entity.key(), entity.clone());
        Ok(())
    }
    async fn put_all(&self, entities: &[E]) -> anyhow::Result<()> {
        for entity in entities {
            encode(&self.config, entity)?;
        }
        let mut records = self
            .records
            .write()
            .map_err(|_| anyhow::anyhow!("memory entity store lock was poisoned"))?;
        for entity in entities {
            records.insert(entity.key(), entity.clone());
        }
        Ok(())
    }
    async fn delete(&self, key: &E::Key) -> anyhow::Result<()> {
        self.records
            .write()
            .map_err(|_| anyhow::anyhow!("memory entity store lock was poisoned"))?
            .remove(key);
        Ok(())
    }
    async fn find(&self, query: &EntityQuery<E>) -> anyhow::Result<Vec<E>> {
        let mut records = self
            .records
            .write()
            .map_err(|_| anyhow::anyhow!("memory entity store lock was poisoned"))?;
        let mut expired_keys = Vec::new();
        let mut result = Vec::new();
        for (key, entity) in records.iter() {
            let document = encode(&self.config, entity)?;
            if expired(&self.config.schema, &document)? {
                expired_keys.push(key.clone());
            } else if matches(&self.config, &document, query)? {
                result.push(entity.clone());
            }
        }
        for key in expired_keys {
            records.remove(&key);
        }
        Ok(result)
    }
}

#[cfg(test)]
#[path = "../../../tests/persistence/typed_store/memory.rs"]
mod tests;
