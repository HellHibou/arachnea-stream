//! Typed repository for the dynamic proxy inventory.

use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::Result;
use arachnea_core::persistence::{
    EntityQuery, EntityReader, EntitySchema, EntityWriter, Field, MemoryEntityStore,
    PersistenceStoreConfig, PersistentEntity, TypedEntityStore,
};
use async_trait::async_trait;

use crate::core::ProxyRecord;

/// Stable store name for dynamic proxy records.
pub const PROXY_STORE_NAME: &str = "proxy-inventory";
/// Default TTL for cached proxy records without probe freshness.
pub const PROXY_CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Repository used by [`crate::core::ProxyInventory`].
#[async_trait]
pub trait ProxyRepository: Send + Sync {
    /// Returns the non-expired proxies indexed for `country`.
    async fn find_by_country(&self, country: &str) -> Result<Vec<ProxyRecord>>;
    /// Persists a batch of records atomically.
    async fn save_many(&self, records: &[ProxyRecord]) -> Result<()>;
    /// Removes a record by authority.
    async fn delete(&self, authority: &str) -> Result<()>;
}

/// Repository implementation over a typed entity store.
pub struct TypedProxyRepository {
    store: Arc<dyn TypedEntityStore<ProxyRecord>>,
}
impl TypedProxyRepository {
    /// Creates a repository over `store`.
    pub fn new(store: Arc<dyn TypedEntityStore<ProxyRecord>>) -> Self {
        Self { store }
    }
}

/// Creates an in-memory typed proxy store used by compatibility constructors.
pub fn memory_proxy_store() -> Result<Arc<dyn TypedEntityStore<ProxyRecord>>> {
    let config = PersistenceStoreConfig::new(PROXY_STORE_NAME, ProxyRecord::schema())?;
    Ok(Arc::new(MemoryEntityStore::<ProxyRecord>::new(config)?))
}
#[async_trait]
impl ProxyRepository for TypedProxyRepository {
    async fn find_by_country(&self, country: &str) -> Result<Vec<ProxyRecord>> {
        Ok(self
            .store
            .find(&EntityQuery::new().where_string(&ProxyRecord::country_field(), country))
            .await?)
    }
    async fn save_many(&self, records: &[ProxyRecord]) -> Result<()> {
        Ok(self.store.put_all(records).await?)
    }
    async fn delete(&self, authority: &str) -> Result<()> {
        Ok(self.store.delete(&authority.to_string()).await?)
    }
}

impl PersistentEntity for ProxyRecord {
    type Key = String;
    fn key(&self) -> Self::Key {
        self.authority()
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("authority"))
            .field(Field::string("country").nullable().indexed())
            .field(Field::date_time("expires_at").expiration())
            .field(Field::json("record"))
    }
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()> {
        writer.string("authority", self.authority())?;
        if let Some(country) = &self.country {
            writer.string("country", country)?;
        }
        writer.date_time(
            "expires_at",
            proxy_cache_expires_at(self, SystemTime::now()),
        )?;
        writer.json("record", serde_json::to_value(self)?)
    }
    fn read_from(reader: &EntityReader<'_>) -> Result<Self> {
        let authority = reader.string("authority")?;
        let record: ProxyRecord = serde_json::from_value(reader.json("record")?.clone())?;
        if record.authority() != authority {
            anyhow::bail!("persisted proxy authority does not match its record");
        }
        Ok(record)
    }
}
impl ProxyRecord {
    /// Schema descriptor used by country repository queries.
    pub fn country_field() -> Field {
        Field::string("country").nullable().indexed()
    }
}
fn proxy_cache_expires_at(record: &ProxyRecord, now: SystemTime) -> SystemTime {
    let ttl_expiry = record
        .last_checked
        .map(|checked| checked + PROXY_CACHE_TTL)
        .unwrap_or(now + PROXY_CACHE_TTL);
    record
        .cooldown_until
        .map(|cooldown| cooldown.min(ttl_expiry))
        .unwrap_or(ttl_expiry)
}
