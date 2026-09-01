//! Typed repository for the dynamic proxy inventory.

use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result};
use arachnea_core::persistence::{
    EntityKey, EntityQuery, EntityReader, EntitySchema, EntityWriter, Field, FieldType,
    MemoryEntityStore, PersistenceStoreConfig, PersistentEntity, SqlKey, TypedEntityStore,
};
use async_trait::async_trait;
use rusqlite::types::{Value as SqlValue, ValueRef};

use crate::core::{
    ProxyAvailabilityHint, ProxyDestinationFailure, ProxyKey, ProxyProtocol, ProxyRecord,
    ProxyRuntimeStatus,
};

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
    /// Removes a record by its persisted endpoint identity.
    async fn delete(&self, key: &ProxyKey) -> Result<()>;
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
    async fn delete(&self, key: &ProxyKey) -> Result<()> {
        Ok(self.store.delete(key).await?)
    }
}

impl EntityKey for ProxyKey {
    fn field_types() -> &'static [FieldType] {
        &[FieldType::String, FieldType::Integer]
    }

    fn storage_key(&self) -> String {
        format!("{}:{}:{}", self.host.len(), self.host, self.port)
    }
}

impl SqlKey for ProxyKey {
    fn physical_types() -> &'static [&'static str] {
        &["TEXT", "INTEGER"]
    }

    fn to_sql_values(&self) -> Vec<SqlValue> {
        vec![
            SqlValue::Text(self.host.clone()),
            SqlValue::Integer(i64::from(self.port)),
        ]
    }

    fn from_sql_value(index: usize, cell: ValueRef<'_>) -> Result<SqlValue> {
        match (index, cell) {
            (0, ValueRef::Text(value)) => {
                Ok(SqlValue::Text(std::str::from_utf8(value)?.to_string()))
            }
            (1, ValueRef::Integer(value)) => Ok(SqlValue::Integer(value)),
            (0, _) => anyhow::bail!("proxy host primary key has an unexpected SQLite type"),
            (1, _) => anyhow::bail!("proxy port primary key has an unexpected SQLite type"),
            _ => anyhow::bail!("proxy primary key has an unexpected component index {index}"),
        }
    }
}

impl PersistentEntity for ProxyRecord {
    type Key = ProxyKey;
    fn key(&self) -> Self::Key {
        ProxyRecord::key(self)
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("host"))
            .primary_key(Field::integer("port"))
            .field(Field::string("protocol").nullable())
            .field(Field::string("country").nullable().indexed())
            .field(Field::boolean("supports_https").nullable())
            .field(Field::string("status"))
            .field(Field::integer("latency_ms").nullable())
            .field(Field::integer("failure_count"))
            .field(Field::boolean("authentication_required").nullable())
            .field(Field::string("availability"))
            .field(Field::json("destination_failures"))
            .field(Field::date_time("last_checked").nullable())
            .field(Field::date_time("cooldown_until").nullable())
            .field(Field::date_time("expires_at").expiration().indexed())
    }
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()> {
        writer.string("host", &self.host)?;
        writer.integer("port", i64::from(self.port))?;
        if let Some(protocol) = &self.protocol {
            writer.string("protocol", protocol.to_string())?;
        }
        if let Some(country) = &self.country {
            writer.string("country", country)?;
        }
        if let Some(supports_https) = self.supports_https {
            writer.boolean("supports_https", supports_https)?;
        }
        writer.string("status", status_name(&self.status))?;
        if let Some(latency_ms) = self.latency_ms {
            writer.integer(
                "latency_ms",
                i64::try_from(latency_ms).context("proxy latency exceeds SQLite integer range")?,
            )?;
        }
        writer.integer("failure_count", i64::from(self.failure_count))?;
        if let Some(authentication_required) = self.authentication_required {
            writer.boolean("authentication_required", authentication_required)?;
        }
        writer.string("availability", availability_name(&self.availability))?;
        writer.json(
            "destination_failures",
            serde_json::to_value(&self.destination_failures)
                .context("failed to serialize proxy destination failures")?,
        )?;
        if let Some(last_checked) = self.last_checked {
            writer.date_time("last_checked", last_checked)?;
        }
        if let Some(cooldown_until) = self.cooldown_until {
            writer.date_time("cooldown_until", cooldown_until)?;
        }
        writer.date_time(
            "expires_at",
            proxy_cache_expires_at(self, SystemTime::now()),
        )?;
        Ok(())
    }
    fn read_from(reader: &EntityReader<'_>) -> Result<Self> {
        let port = u16::try_from(reader.integer("port")?)
            .context("persisted proxy port is outside the u16 range")?;
        let latency_ms = reader
            .optional_integer("latency_ms")?
            .map(|value| {
                u64::try_from(value).context("persisted proxy latency is outside the u64 range")
            })
            .transpose()?;
        let failure_count = u32::try_from(reader.integer("failure_count")?)
            .context("persisted proxy failure count is outside the u32 range")?;
        Ok(ProxyRecord {
            protocol: reader
                .optional_string("protocol")?
                .map(parse_protocol)
                .transpose()?,
            host: reader.string("host")?.to_string(),
            port,
            country: reader.optional_string("country")?.map(str::to_string),
            supports_https: reader.optional_boolean("supports_https")?,
            status: parse_status(reader.string("status")?)?,
            latency_ms,
            failure_count,
            authentication_required: reader.optional_boolean("authentication_required")?,
            availability: parse_availability(reader.string("availability")?)?,
            destination_failures: serde_json::from_value::<Vec<ProxyDestinationFailure>>(
                reader.json("destination_failures")?.clone(),
            )
            .context("failed to deserialize proxy destination failures")?,
            last_checked: reader.optional_date_time("last_checked")?,
            cooldown_until: reader.optional_date_time("cooldown_until")?,
        })
    }
}
fn status_name(status: &ProxyRuntimeStatus) -> &'static str {
    match status {
        ProxyRuntimeStatus::Unknown => "unknown",
        ProxyRuntimeStatus::Ok => "ok",
        ProxyRuntimeStatus::Ko => "ko",
        ProxyRuntimeStatus::AuthenticationRequired => "authentication_required",
    }
}
fn parse_status(value: &str) -> Result<ProxyRuntimeStatus> {
    match value {
        "unknown" => Ok(ProxyRuntimeStatus::Unknown),
        "ok" => Ok(ProxyRuntimeStatus::Ok),
        "ko" => Ok(ProxyRuntimeStatus::Ko),
        "authentication_required" => Ok(ProxyRuntimeStatus::AuthenticationRequired),
        _ => anyhow::bail!("unknown persisted proxy status '{value}'"),
    }
}
fn availability_name(availability: &ProxyAvailabilityHint) -> &'static str {
    match availability {
        ProxyAvailabilityHint::Unknown => "unknown",
        ProxyAvailabilityHint::Low => "low",
        ProxyAvailabilityHint::Medium => "medium",
        ProxyAvailabilityHint::High => "high",
    }
}
fn parse_availability(value: &str) -> Result<ProxyAvailabilityHint> {
    match value {
        "unknown" => Ok(ProxyAvailabilityHint::Unknown),
        "low" => Ok(ProxyAvailabilityHint::Low),
        "medium" => Ok(ProxyAvailabilityHint::Medium),
        "high" => Ok(ProxyAvailabilityHint::High),
        _ => anyhow::bail!("unknown persisted proxy availability '{value}'"),
    }
}
fn parse_protocol(value: &str) -> Result<ProxyProtocol> {
    match value {
        "http" => Ok(ProxyProtocol::Http),
        "https" => Ok(ProxyProtocol::Https),
        "socks4" => Ok(ProxyProtocol::Socks4),
        "socks4a" => Ok(ProxyProtocol::Socks4a),
        "socks5" => Ok(ProxyProtocol::Socks5),
        _ => anyhow::bail!("unknown persisted proxy protocol '{value}'"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use arachnea_core::persistence::{FileEntityStore, SqliteEntityStore};
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    fn temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "arachnea-proxy-persistence-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&directory);
        directory
    }

    fn record(host: &str, port: u16, country: Option<&str>) -> ProxyRecord {
        let destination_failure_time = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| {
                std::time::UNIX_EPOCH + Duration::from_millis(duration.as_millis() as u64)
            })
            .expect("system time after UNIX_EPOCH");
        ProxyRecord {
            protocol: Some(ProxyProtocol::Socks5),
            host: host.to_string(),
            port,
            country: country.map(str::to_string),
            supports_https: Some(true),
            status: ProxyRuntimeStatus::Ok,
            latency_ms: Some(42),
            failure_count: 0,
            authentication_required: Some(false),
            availability: ProxyAvailabilityHint::High,
            destination_failures: vec![ProxyDestinationFailure {
                scheme: "https".to_string(),
                host: "origin.example".to_string(),
                port: 443,
                reason: crate::core::ProxyDestinationFailureReason::Timeout,
                failure_count: 1,
                last_failed: destination_failure_time,
                cooldown_until: None,
            }],
            last_checked: Some(SystemTime::now()),
            cooldown_until: None,
        }
    }

    fn config() -> PersistenceStoreConfig {
        PersistenceStoreConfig::new(PROXY_STORE_NAME, ProxyRecord::schema()).expect("valid config")
    }

    #[tokio::test]
    async fn composite_proxy_key_roundtrips_on_all_backends() -> Result<()> {
        let proxy = record("2001:db8::1", 1080, Some("BE"));
        let key = proxy.key();

        let memory = MemoryEntityStore::<ProxyRecord>::new(config())?;
        memory.put(&proxy).await?;
        assert_eq!(memory.get(&key).await?, Some(proxy.clone()));

        let file_root = temp_dir("file-roundtrip");
        let file = FileEntityStore::<ProxyRecord>::new(file_root.clone(), config())?;
        file.put(&proxy).await?;
        assert_eq!(file.get(&key).await?, Some(proxy.clone()));

        let sqlite_root = temp_dir("sqlite-roundtrip");
        {
            let sqlite = SqliteEntityStore::<ProxyRecord>::new(config(), &sqlite_root)?;
            sqlite.put(&proxy).await?;
            assert_eq!(sqlite.get(&key).await?, Some(proxy.clone()));
            assert_eq!(
                sqlite
                    .find(&EntityQuery::new().where_string(&ProxyRecord::country_field(), "BE"))
                    .await?,
                vec![proxy.clone()]
            );
        }
        let reopened = SqliteEntityStore::<ProxyRecord>::new(config(), &sqlite_root)?;
        assert_eq!(reopened.get(&key).await?, Some(proxy));

        let _ = std::fs::remove_dir_all(&file_root);
        let _ = std::fs::remove_dir_all(&sqlite_root);
        Ok(())
    }

    #[tokio::test]
    async fn composite_sqlite_batch_is_atomic_and_expiration_is_filtered() -> Result<()> {
        let root = temp_dir("atomic-expiration");
        let store = SqliteEntityStore::<ProxyRecord>::new(config(), &root)?;
        let kept = record("kept.example", 8080, Some("BE"));
        store.put(&kept).await?;

        let mut invalid = record("invalid.example", 8081, Some("BE"));
        invalid.latency_ms = Some(u64::MAX);
        assert!(store
            .put_all(&[record("new.example", 8082, Some("BE")), invalid])
            .await
            .is_err());
        assert_eq!(store.get(&kept.key()).await?, Some(kept));

        let mut expired = record("expired.example", 8083, Some("BE"));
        expired.last_checked = Some(SystemTime::now() - PROXY_CACHE_TTL - Duration::from_secs(1));
        store.put(&expired).await?;
        assert!(store.get(&expired.key()).await?.is_none());

        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn legacy_authority_key_sqlite_cache_is_rejected() -> Result<()> {
        let root = temp_dir("legacy-cache");
        let store_directory = root.join(PROXY_STORE_NAME);
        std::fs::create_dir_all(&store_directory)?;
        let connection = rusqlite::Connection::open(store_directory.join("records.sqlite3"))?;
        connection.execute_batch(
            "CREATE TABLE \"proxy-inventory\" (\
                authority TEXT PRIMARY KEY, \
                country TEXT, \
                expires_at INTEGER NOT NULL, \
                record TEXT NOT NULL\
            ) STRICT;",
        )?;
        drop(connection);

        let error = SqliteEntityStore::<ProxyRecord>::new(config(), &root)
            .err()
            .expect("legacy authority key must be rejected");
        assert!(error.to_string().contains("primary key"));

        std::fs::remove_dir_all(&store_directory)?;
        assert!(SqliteEntityStore::<ProxyRecord>::new(config(), &root).is_ok());

        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }
}
