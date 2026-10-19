//! Typed repository for the dynamic proxy inventory.

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock, Weak},
    time::SystemTime,
};

use anyhow::{Context, Result};
use arachnea_core::persistence::{
    EntityKey, EntityQuery, EntityReader, EntitySchema, EntityWriter, Field, FieldType,
    MemoryEntityStore, PersistenceStoreConfig, PersistentEntity, SqlKey, TypedEntityStore,
};
use async_trait::async_trait;
use rusqlite::types::{Value as SqlValue, ValueRef};
use tokio::sync::Mutex;

use crate::core::{
    ProxyAvailabilityHint, ProxyCapabilityStatus, ProxyDeclaration, ProxyDestinationFailure,
    ProxyKey, ProxyProtocol, ProxyRecord, ProxyRuntimeStatus, PROXY_CACHE_TTL,
};

/// Physical SQLite store and available-record table name for dynamic proxies.
pub const PROXY_STORE_NAME: &str = "proxy-inventory";
/// Table name for bounded eviction records in the shared proxy SQLite store.
pub const PROXY_FAILURE_STORE_NAME: &str = "proxy-failures";
/// Repository used by [`crate::core::ProxyInventory`].
#[async_trait]
pub trait ProxyRepository: Send + Sync {
    /// Resolves an endpoint without relying on its historical country.
    async fn get(&self, key: &ProxyKey) -> Result<Option<ProxyRecord>>;
    /// Loads rediscovery protection retained outside the available inventory.
    async fn get_failure(&self, key: &ProxyKey) -> Result<Option<ProxyRecord>>;
    /// Removes an available endpoint after durably recording its failure state.
    async fn evict(&self, record: &ProxyRecord) -> Result<()>;
    /// Returns the proxies indexed for `country`.
    async fn find_by_country(&self, country: &str) -> Result<Vec<ProxyRecord>>;
    /// Persists a batch of records atomically.
    async fn save_many(&self, records: &[ProxyRecord]) -> Result<()>;
    /// Clears strictly older ambiguous exclusions for an accepted exact destination.
    /// Reads and updates the latest record under the repository commit lock.
    ///
    /// # Arguments
    /// * `key` - Endpoint whose accepted response was observed.
    /// * `destination` - Exact scheme, host and port that accepted the response.
    /// * `accepted_at` - Acceptance date; equal-date and newer failures remain intact.
    ///
    /// # Errors
    /// Returns a storage error if reading or updating the record fails.
    async fn clear_superseded_destination_failures(
        &self,
        key: &ProxyKey,
        destination: &crate::core::Destination,
        accepted_at: SystemTime,
    ) -> Result<()>;
    /// Removes a record by its persisted endpoint identity.
    async fn delete(&self, key: &ProxyKey) -> Result<()>;
}

/// Repository implementation over a typed entity store.
pub struct TypedProxyRepository {
    store: Arc<dyn TypedEntityStore<ProxyRecord>>,
    failures: Arc<dyn TypedEntityStore<ProxyRecord>>,
    commits: Arc<Mutex<()>>,
}
impl TypedProxyRepository {
    /// Creates a repository over `store`.
    pub fn new(store: Arc<dyn TypedEntityStore<ProxyRecord>>) -> Self {
        Self::with_failure_store(store, memory_proxy_store().expect("valid failure schema"))
    }

    /// Creates a repository sharing application-owned inventory and failure stores.
    ///
    /// # Arguments
    /// * `store` - Available inventory store.
    /// * `failures` - Durable rediscovery-protection table, distinct from available records.
    pub fn with_failure_store(
        store: Arc<dyn TypedEntityStore<ProxyRecord>>,
        failures: Arc<dyn TypedEntityStore<ProxyRecord>>,
    ) -> Self {
        static LOCKS: OnceLock<std::sync::Mutex<HashMap<usize, Weak<Mutex<()>>>>> = OnceLock::new();
        let identity = Arc::as_ptr(&store) as *const () as usize;
        let mut locks = LOCKS
            .get_or_init(Default::default)
            .lock()
            .expect("inventory commit registry poisoned");
        locks.retain(|_, lock| lock.strong_count() > 0);
        let commits = locks
            .get(&identity)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(identity, Arc::downgrade(&lock));
                lock
            });
        Self {
            store,
            failures,
            commits,
        }
    }
}

/// Creates an in-memory typed proxy store used by compatibility constructors.
pub fn memory_proxy_store() -> Result<Arc<dyn TypedEntityStore<ProxyRecord>>> {
    let config = PersistenceStoreConfig::new(PROXY_STORE_NAME, ProxyRecord::schema())?;
    Ok(Arc::new(MemoryEntityStore::<ProxyRecord>::new(config)?))
}
#[async_trait]
impl ProxyRepository for TypedProxyRepository {
    async fn get(&self, key: &ProxyKey) -> Result<Option<ProxyRecord>> {
        let record = self.store.get(key).await?;
        if let Some(failure) = self.get_failure(key).await? {
            if record.as_ref().is_some_and(|record| {
                record.health_evidence.last_positive_at <= failure.health_evidence.evicted_at
                    && record.health_evidence.first_discovered_at
                        <= failure.health_evidence.evicted_at
                    && record.last_checked <= failure.health_evidence.evicted_at
            }) {
                return Ok(None);
            }
        }
        Ok(record.filter(|record| record.health_evidence.evicted_at.is_none()))
    }
    async fn get_failure(&self, key: &ProxyKey) -> Result<Option<ProxyRecord>> {
        Ok(self.failures.get(key).await?.filter(|record| {
            record.health_evidence.evicted_at.is_some_and(|date| {
                SystemTime::now().duration_since(date).unwrap_or_default()
                    < std::time::Duration::from_secs(48 * 3600)
            })
        }))
    }
    async fn evict(&self, record: &ProxyRecord) -> Result<()> {
        let _guard = self.commits.lock().await;
        let mut failures = self.failures.find(&EntityQuery::new()).await?;
        failures.sort_by_key(|record| record.health_evidence.evicted_at);
        let excess = failures.len().saturating_sub(1023);
        for (index, failure) in failures.iter().enumerate() {
            if index < excess
                || failure.health_evidence.evicted_at.is_some_and(|date| {
                    SystemTime::now().duration_since(date).unwrap_or_default()
                        >= std::time::Duration::from_secs(48 * 3600)
                })
            {
                self.failures.delete(&failure.key()).await?;
            }
        }
        self.failures.put(record).await?;
        self.store.delete(&record.key()).await
    }
    async fn find_by_country(&self, country: &str) -> Result<Vec<ProxyRecord>> {
        let records = self
            .store
            .find(&EntityQuery::new().where_string(&ProxyRecord::country_field(), country))
            .await?;
        let mut available = Vec::new();
        for record in records {
            if let Some(record) = self.get(&record.key()).await? {
                available.push(record);
            }
        }
        Ok(available)
    }
    async fn save_many(&self, records: &[ProxyRecord]) -> Result<()> {
        let _guard = self.commits.lock().await;
        let mut merged = Vec::with_capacity(records.len());
        for record in records {
            let mut record = record.clone();
            if let Some(current) = self.store.get(&record.key()).await? {
                record.health_evidence.last_positive_at = record
                    .health_evidence
                    .last_positive_at
                    .max(current.health_evidence.last_positive_at);
                record.health_evidence.first_discovered_at = match (
                    record.health_evidence.first_discovered_at,
                    current.health_evidence.first_discovered_at,
                ) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                for failure in current.destination_failures {
                    if let Some(existing) = record.destination_failures.iter_mut().find(|item| {
                        item.scheme == failure.scheme
                            && item.host == failure.host
                            && item.port == failure.port
                    }) {
                        let until = existing.cooldown_until.max(failure.cooldown_until);
                        if failure.last_failed > existing.last_failed {
                            *existing = failure;
                        }
                        existing.cooldown_until = until;
                    } else {
                        record.destination_failures.push(failure);
                    }
                }
            }
            merged.push(record);
        }
        Ok(self.store.put_all(&merged).await?)
    }
    async fn clear_superseded_destination_failures(
        &self,
        key: &ProxyKey,
        destination: &crate::core::Destination,
        accepted_at: SystemTime,
    ) -> Result<()> {
        let _guard = self.commits.lock().await;
        if let Some(mut record) = self.store.get(key).await? {
            if super::proxy_inventory::clear_superseded_destination_failures(
                &mut record,
                destination,
                accepted_at,
            ) {
                self.store.put(&record).await?;
            }
        }
        Ok(())
    }
    async fn delete(&self, key: &ProxyKey) -> Result<()> {
        let _guard = self.commits.lock().await;
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
            .field(Field::json("declarations").nullable())
            .field(Field::json("probe_attempts").nullable())
            .field(Field::json("health_evidence").nullable())
            .field(Field::json("observations").nullable())
            .field(Field::boolean("tcp_reachable").nullable())
            .field(Field::string("status"))
            .field(Field::string("http_forwarding").nullable())
            .field(Field::string("https_tunnel").nullable())
            .field(Field::string("destination_tls").nullable())
            .field(Field::string("proxy_tls_certificate").nullable())
            .field(Field::integer("latency_ms").nullable())
            .field(Field::integer("failure_count"))
            .field(Field::boolean("authentication_required").nullable())
            .field(Field::string("availability"))
            .field(Field::json("destination_failures"))
            .field(Field::date_time("last_checked").nullable())
            .field(Field::date_time("last_validated_at").nullable())
            .field(Field::date_time("cooldown_until").nullable())
            // Kept for compatibility with existing stores. This field is no
            // longer declared as an expiration and is therefore never purged
            // automatically by the typed persistence layer.
            .field(Field::date_time("expires_at").indexed())
    }
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()> {
        writer.string("host", &self.host)?;
        writer.json(
            "health_evidence",
            serde_json::to_value(&self.health_evidence)?,
        )?;
        writer.json("observations", serde_json::to_value(&self.observations)?)?;
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
        writer.json("declarations", serde_json::to_value(&self.declarations)?)?;
        writer.json(
            "probe_attempts",
            serde_json::to_value(&self.probe_attempts)?,
        )?;
        if let Some(reachable) = self.tcp_reachable {
            writer.boolean("tcp_reachable", reachable)?;
        }
        writer.string("status", status_name(&self.status))?;
        writer.string(
            "http_forwarding",
            capability_status_name(self.http_forwarding),
        )?;
        writer.string("https_tunnel", capability_status_name(self.https_tunnel))?;
        writer.string(
            "destination_tls",
            capability_status_name(self.destination_tls),
        )?;
        writer.string(
            "proxy_tls_certificate",
            capability_status_name(self.proxy_tls_certificate),
        )?;
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
        if let Some(last_validated_at) = self.last_validated_at {
            writer.date_time("last_validated_at", last_validated_at)?;
        }
        if let Some(cooldown_until) = self.cooldown_until {
            writer.date_time("cooldown_until", cooldown_until)?;
        }
        let validation_reference = self
            .last_validated_at
            .or(self.last_checked)
            .unwrap_or_else(SystemTime::now);
        writer.date_time("expires_at", validation_reference + PROXY_CACHE_TTL)?;
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
            health_evidence: reader
                .optional_json("health_evidence")?
                .map(|value| serde_json::from_value(value.clone()))
                .transpose()?
                .unwrap_or_default(),
            observations: reader
                .optional_json("observations")?
                .map(|value| serde_json::from_value(value.clone()))
                .transpose()?
                .unwrap_or_default(),
            probe_attempts: reader
                .optional_json("probe_attempts")?
                .map(|value| serde_json::from_value(value.clone()))
                .transpose()?
                .unwrap_or_default(),
            tcp_reachable: reader.optional_boolean("tcp_reachable")?,
            protocol: reader
                .optional_string("protocol")?
                .map(parse_protocol)
                .transpose()?,
            host: reader.string("host")?.to_string(),
            port,
            country: reader.optional_string("country")?.map(str::to_string),
            supports_https: reader.optional_boolean("supports_https")?,
            declarations: reader
                .optional_json("declarations")?
                .map(|value| {
                    serde_json::from_value::<Vec<ProxyDeclaration>>(value.clone())
                        .context("failed to deserialize proxy declarations")
                })
                .transpose()?
                .unwrap_or_default(),
            status: parse_status(reader.string("status")?)?,
            http_forwarding: parse_optional_capability_status(
                reader.optional_string("http_forwarding")?,
            )?,
            https_tunnel: parse_optional_capability_status(
                reader.optional_string("https_tunnel")?,
            )?,
            destination_tls: parse_optional_capability_status(
                reader.optional_string("destination_tls")?,
            )?,
            proxy_tls_certificate: parse_optional_capability_status(
                reader.optional_string("proxy_tls_certificate")?,
            )?,
            latency_ms,
            failure_count,
            authentication_required: reader.optional_boolean("authentication_required")?,
            availability: parse_availability(reader.string("availability")?)?,
            destination_failures: serde_json::from_value::<Vec<ProxyDestinationFailure>>(
                reader.json("destination_failures")?.clone(),
            )
            .context("failed to deserialize proxy destination failures")?,
            last_checked: reader.optional_date_time("last_checked")?,
            last_validated_at: reader.optional_date_time("last_validated_at")?,
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
fn capability_status_name(status: ProxyCapabilityStatus) -> &'static str {
    match status {
        ProxyCapabilityStatus::Unknown => "unknown",
        ProxyCapabilityStatus::Available => "available",
        ProxyCapabilityStatus::Unavailable => "unavailable",
    }
}
fn parse_optional_capability_status(value: Option<&str>) -> Result<ProxyCapabilityStatus> {
    match value {
        None | Some("unknown") => Ok(ProxyCapabilityStatus::Unknown),
        Some("available") => Ok(ProxyCapabilityStatus::Available),
        Some("unavailable") => Ok(ProxyCapabilityStatus::Unavailable),
        Some(value) => anyhow::bail!("unknown persisted proxy capability status '{value}'"),
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
#[cfg(test)]
mod tests {
    use super::*;
    use arachnea_core::persistence::{
        FileEntityStore, SqliteDatabase, SqliteEntityStore, SqliteStoreConfig,
    };
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, SystemTime},
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
            health_evidence: Default::default(),
            observations: Vec::new(),
            probe_attempts: std::collections::HashMap::from([("https".to_string(), 1791540000000)]),
            tcp_reachable: Some(true),
            protocol: Some(ProxyProtocol::Socks5),
            host: host.to_string(),
            port,
            country: country.map(str::to_string),
            supports_https: Some(true),
            declarations: vec![ProxyDeclaration {
                source: "test-provider".to_string(),
                protocol: Some(ProxyProtocol::Socks5),
                supports_https: Some(true),
            }],
            status: ProxyRuntimeStatus::Ok,
            http_forwarding: ProxyCapabilityStatus::Available,
            https_tunnel: ProxyCapabilityStatus::Available,
            destination_tls: ProxyCapabilityStatus::Unknown,
            proxy_tls_certificate: ProxyCapabilityStatus::Unknown,
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
            last_validated_at: Some(SystemTime::now()),
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

        let preference_config = PersistenceStoreConfig::new(
            crate::core::PROXY_PREFERENCE_STORE_NAME,
            crate::core::ProxyDestinationPreference::schema(),
        )?;
        let destination = crate::core::Destination::host_port("ORIGIN.example", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        let mut preference = crate::core::ProxyDestinationPreference {
            destination: crate::core::ProxyDestinationPreference::destination_key(&destination),
            endpoints: Vec::new(),
        };
        let now = SystemTime::now();
        preference.record_success(key.clone(), now);
        for index in 0..4 {
            preference.record_success(
                ProxyKey {
                    host: format!("accepted-{index}.example"),
                    port: 8080,
                },
                now + Duration::from_secs(index),
            );
        }
        assert_eq!(preference.endpoints.len(), 3);
        let preference_memory = MemoryEntityStore::new(preference_config.clone())?;
        preference_memory.put(&preference).await?;
        assert_eq!(
            preference_memory.get(&preference.destination).await?,
            Some(preference.clone())
        );
        let preference_file = FileEntityStore::new(file_root.clone(), preference_config.clone())?;
        preference_file.put(&preference).await?;
        assert_eq!(
            preference_file.get(&preference.destination).await?,
            Some(preference.clone())
        );
        {
            let database =
                SqliteDatabase::open(SqliteStoreConfig::new(PROXY_STORE_NAME)?, &sqlite_root)?;
            let preferences = Arc::new(database.table(preference_config.clone())?);
            let repository = crate::core::TypedProxyPreferenceRepository::new(preferences);
            crate::core::ProxyPreferenceRepository::save(&repository, &preference).await?;
            let mut older = preference.clone();
            older.endpoints[0].accepted_at = now - Duration::from_secs(60);
            crate::core::ProxyPreferenceRepository::save(&repository, &older).await?;
        }
        let database =
            SqliteDatabase::open(SqliteStoreConfig::new(PROXY_STORE_NAME)?, &sqlite_root)?;
        let reopened_preferences =
            database.table::<crate::core::ProxyDestinationPreference>(preference_config)?;
        assert_eq!(
            reopened_preferences.get(&preference.destination).await?,
            Some(preference)
        );

        let _ = std::fs::remove_dir_all(&file_root);
        let _ = std::fs::remove_dir_all(&sqlite_root);
        Ok(())
    }

    #[tokio::test]
    async fn composite_sqlite_batch_is_atomic_and_stale_records_remain_available() -> Result<()> {
        let root = temp_dir("atomic-stale");
        let database = SqliteDatabase::open(SqliteStoreConfig::new(PROXY_STORE_NAME)?, &root)?;
        let store = database.table::<ProxyRecord>(config())?;
        let kept = record("kept.example", 8080, Some("BE"));
        store.put(&kept).await?;

        let mut invalid = record("invalid.example", 8081, Some("BE"));
        invalid.latency_ms = Some(u64::MAX);
        assert!(store
            .put_all(&[record("new.example", 8082, Some("BE")), invalid])
            .await
            .is_err());
        assert_eq!(store.get(&kept.key()).await?, Some(kept));

        let mut stale = record("stale.example", 8083, Some("BE"));
        stale.last_validated_at =
            Some(SystemTime::now() - PROXY_CACHE_TTL - Duration::from_secs(1));
        store.put(&stale).await?;
        assert_eq!(store.get(&stale.key()).await?, Some(stale.clone()));
        stale.health_evidence.last_positive_at = stale.last_validated_at;
        stale.health_evidence.first_discovered_at = stale.last_validated_at;
        let store = Arc::new(store);
        let failures = Arc::new(database.table(PersistenceStoreConfig::new(
            PROXY_FAILURE_STORE_NAME,
            ProxyRecord::schema(),
        )?)?);
        let repository = Arc::new(TypedProxyRepository::with_failure_store(
            store.clone(),
            failures.clone(),
        ));
        let inventory =
            crate::core::ProxyInventory::new(crate::core::InventoryConfig::default(), None, None)
                .with_proxy_repository(repository.clone());
        inventory.add_or_update(vec![stale.clone()]).await;
        inventory.record_global_failure(&stale.authority()).await;
        assert!(store.get(&stale.key()).await?.is_none());
        assert!(repository.get_failure(&stale.key()).await?.is_some());
        assert!(inventory.is_empty().await);
        let restored =
            crate::core::ProxyInventory::new(crate::core::InventoryConfig::default(), None, None)
                .with_proxy_repository(Arc::new(TypedProxyRepository::with_failure_store(
                    store, failures,
                )));
        restored
            .add_or_update(vec![record("stale.example", 8083, Some("BE"))])
            .await;
        assert!(restored.is_empty().await);

        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn legacy_authority_key_sqlite_cache_is_rejected() -> Result<()> {
        let root = temp_dir("legacy-cache");
        std::fs::create_dir_all(&root)?;
        let database_path = root.join(format!("{PROXY_STORE_NAME}.sqlite3"));
        let connection = rusqlite::Connection::open(&database_path)?;
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

        std::fs::remove_file(&database_path)?;
        assert!(SqliteEntityStore::<ProxyRecord>::new(config(), &root).is_ok());

        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }

    #[tokio::test]
    async fn interrupted_eviction_hides_available_record_and_orphan_preference() -> Result<()> {
        let root = temp_dir("interrupted-eviction");
        let database = SqliteDatabase::open(SqliteStoreConfig::new(PROXY_STORE_NAME)?, &root)?;
        let available = Arc::new(database.table::<ProxyRecord>(config())?);
        let failures = Arc::new(database.table::<ProxyRecord>(PersistenceStoreConfig::new(
            PROXY_FAILURE_STORE_NAME,
            ProxyRecord::schema(),
        )?)?);
        let preferences = Arc::new(database.table::<crate::core::ProxyDestinationPreference>(
            PersistenceStoreConfig::new(
                crate::core::PROXY_PREFERENCE_STORE_NAME,
                crate::core::ProxyDestinationPreference::schema(),
            )?,
        )?);
        let mut proxy = record("interrupted.example", 8080, Some("BE"));
        proxy.health_evidence.first_discovered_at =
            Some(SystemTime::now() - PROXY_CACHE_TTL - Duration::from_secs(60));
        proxy.health_evidence.last_positive_at = proxy.health_evidence.first_discovered_at;
        proxy.last_checked = proxy.health_evidence.last_positive_at;
        available.put(&proxy).await?;
        let destination = crate::core::Destination::host_port("origin.example", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        let mut preference = crate::core::ProxyDestinationPreference {
            destination: crate::core::ProxyDestinationPreference::destination_key(&destination),
            endpoints: Vec::new(),
        };
        preference.record_success(proxy.key(), proxy.health_evidence.last_positive_at.unwrap());
        preferences.put(&preference).await?;
        let mut tombstone = proxy.clone();
        tombstone.health_evidence.evicted_at = Some(SystemTime::now());
        tombstone.cooldown_until = Some(SystemTime::now() + Duration::from_secs(900));
        tombstone.status = ProxyRuntimeStatus::Ko;
        // Crash-equivalent state after tombstone commit, before available/history deletion.
        failures.put(&tombstone).await?;
        drop((available, failures, preferences, database));
        let database = SqliteDatabase::open(SqliteStoreConfig::new(PROXY_STORE_NAME)?, &root)?;
        let available = Arc::new(database.table::<ProxyRecord>(config())?);
        let failures = Arc::new(database.table::<ProxyRecord>(PersistenceStoreConfig::new(
            PROXY_FAILURE_STORE_NAME,
            ProxyRecord::schema(),
        )?)?);
        let preferences = Arc::new(crate::core::TypedProxyPreferenceRepository::new(Arc::new(
            database.table::<crate::core::ProxyDestinationPreference>(
                PersistenceStoreConfig::new(
                    crate::core::PROXY_PREFERENCE_STORE_NAME,
                    crate::core::ProxyDestinationPreference::schema(),
                )?,
            )?,
        )));
        let repository = Arc::new(TypedProxyRepository::with_failure_store(
            available.clone(),
            failures,
        ));
        assert!(available.get(&proxy.key()).await?.is_some());
        assert!(repository.get(&proxy.key()).await?.is_none());
        let inventory =
            crate::core::ProxyInventory::new(crate::core::InventoryConfig::default(), None, None)
                .with_proxy_repository(repository.clone())
                .with_preference_repository(preferences);
        assert!(inventory
            .select_for_destination("BE", true, Some(&destination))
            .await
            .is_err());
        inventory.add_or_update(vec![proxy.clone()]).await;
        assert!(inventory.is_empty().await);
        repository.evict(&tombstone).await?;
        assert!(available.get(&proxy.key()).await?.is_none());
        drop((inventory, repository, available, database));
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[tokio::test]
    async fn failure_retention_expires_after_48_hours_and_prunes_on_demand() -> Result<()> {
        let available = memory_proxy_store()?;
        let failures = memory_proxy_store()?;
        let repository = TypedProxyRepository::with_failure_store(available, failures.clone());
        let mut expired = record("expired.example", 8080, Some("BE"));
        expired.health_evidence.evicted_at =
            Some(SystemTime::now() - Duration::from_secs(49 * 3600));
        failures.put(&expired).await?;
        assert!(repository.get_failure(&expired.key()).await?.is_none());
        assert!(failures.get(&expired.key()).await?.is_some());
        let mut current = record("current.example", 8081, Some("BE"));
        current.health_evidence.evicted_at = Some(SystemTime::now());
        repository.evict(&current).await?;
        assert!(failures.get(&expired.key()).await?.is_none());
        assert!(repository.get_failure(&current.key()).await?.is_some());
        Ok(())
    }

    #[tokio::test]
    async fn eviction_recovery_handles_remaining_inter_table_commit_boundaries() -> Result<()> {
        use crate::core::ProxyPreferenceRepository;
        for stage in 0..4 {
            let root = temp_dir(&format!("eviction-boundary-{stage}"));
            let database = SqliteDatabase::open(SqliteStoreConfig::new(PROXY_STORE_NAME)?, &root)?;
            let available = database.table::<ProxyRecord>(config())?;
            let failure_config =
                PersistenceStoreConfig::new(PROXY_FAILURE_STORE_NAME, ProxyRecord::schema())?;
            let preference_config = PersistenceStoreConfig::new(
                crate::core::PROXY_PREFERENCE_STORE_NAME,
                crate::core::ProxyDestinationPreference::schema(),
            )?;
            let failures = database.table::<ProxyRecord>(failure_config.clone())?;
            let preferences = database
                .table::<crate::core::ProxyDestinationPreference>(preference_config.clone())?;
            assert!(root.join("proxy-inventory.sqlite3").is_file());
            assert!(!root.join("proxy-failures.sqlite3").exists());
            assert!(!root.join("proxy-preferences.sqlite3").exists());
            let mut proxy = record("boundary.example", 8080, Some("BE"));
            proxy.health_evidence.first_discovered_at =
                Some(SystemTime::now() - Duration::from_secs(25 * 3600));
            proxy.health_evidence.last_positive_at = proxy.health_evidence.first_discovered_at;
            proxy.last_checked = proxy.health_evidence.last_positive_at;
            let mut preference = crate::core::ProxyDestinationPreference {
                destination: "https://origin.example:443".into(),
                endpoints: Vec::new(),
            };
            preference.record_success(proxy.key(), proxy.health_evidence.last_positive_at.unwrap());
            available.put(&proxy).await?;
            preferences.put(&preference).await?;
            let mut tombstone = proxy.clone();
            tombstone.health_evidence.evicted_at = Some(SystemTime::now());
            tombstone.cooldown_until = Some(SystemTime::now() + Duration::from_secs(900));
            tombstone.status = ProxyRuntimeStatus::Ko;
            if stage >= 1 {
                failures.put(&tombstone).await?;
            }
            if stage >= 2 {
                available.delete(&proxy.key()).await?;
            }
            if stage >= 3 {
                preferences.delete(&preference.destination).await?;
            }
            drop((available, failures, preferences, database));
            let database = SqliteDatabase::open(SqliteStoreConfig::new(PROXY_STORE_NAME)?, &root)?;
            let available = Arc::new(database.table::<ProxyRecord>(config())?);
            let failures = Arc::new(database.table::<ProxyRecord>(failure_config)?);
            let preference_store = Arc::new(
                database.table::<crate::core::ProxyDestinationPreference>(preference_config)?,
            );
            let repository = TypedProxyRepository::with_failure_store(available.clone(), failures);
            let histories = crate::core::TypedProxyPreferenceRepository::new(preference_store);
            assert_eq!(repository.get(&proxy.key()).await?.is_some(), stage == 0);
            assert_eq!(
                histories.get(&preference.destination).await?.is_some(),
                stage < 3
            );
            // An interruption before the first durable write cannot preserve eviction.
            // Replay the observed failure/cleanup at demand and verify idempotence.
            repository.evict(&tombstone).await?;
            histories.remove_endpoint(&proxy.key()).await?;
            repository.evict(&tombstone).await?;
            histories.remove_endpoint(&proxy.key()).await?;
            assert!(repository.get(&proxy.key()).await?.is_none());
            assert!(histories.get(&preference.destination).await?.is_none());
            drop((repository, histories, available, database));
            std::fs::remove_dir_all(root)?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_file_history_commits_survive_reopen_across_destinations() -> Result<()> {
        use crate::core::ProxyPreferenceRepository;
        let root = temp_dir("concurrent-preferences");
        let config = PersistenceStoreConfig::new(
            crate::core::PROXY_PREFERENCE_STORE_NAME,
            crate::core::ProxyDestinationPreference::schema(),
        )?;
        let store = Arc::new(
            FileEntityStore::<crate::core::ProxyDestinationPreference>::new(
                root.clone(),
                config.clone(),
            )?,
        );
        let first = Arc::new(crate::core::TypedProxyPreferenceRepository::new(
            store.clone(),
        ));
        let second = Arc::new(crate::core::TypedProxyPreferenceRepository::new(
            store.clone(),
        ));
        let now = std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let writes_started = std::time::Instant::now();
        let mut tasks = Vec::new();
        for index in 0..20 {
            let repository = if index % 2 == 0 {
                first.clone()
            } else {
                second.clone()
            };
            tasks.push(tokio::spawn(async move {
                let mut history = crate::core::ProxyDestinationPreference {
                    destination: format!("https://origin-{}.example:443", index % 4),
                    endpoints: Vec::new(),
                };
                history.record_success(
                    crate::core::ProxyKey {
                        host: format!("proxy-{index}.example"),
                        port: 8080,
                    },
                    now + Duration::from_secs(index),
                );
                repository.save(&history).await
            }));
        }
        for task in tasks {
            task.await??;
        }
        let writes_us = writes_started.elapsed().as_micros();
        drop((first, second, store));
        let reopen_started = std::time::Instant::now();
        let reopened =
            FileEntityStore::<crate::core::ProxyDestinationPreference>::new(root.clone(), config)?;
        let histories = reopened.find(&EntityQuery::new()).await?;
        eprintln!("lot7 durable histories: commits=20, destinations={}, writes_us={writes_us}, reopen_and_read_us={}", histories.len(), reopen_started.elapsed().as_micros());
        assert_eq!(histories.len(), 4);
        for history in histories {
            assert_eq!(history.endpoints.len(), 3);
            assert!(history
                .endpoints
                .windows(2)
                .all(|pair| pair[0].accepted_at > pair[1].accepted_at));
            let group: u64 = history
                .destination
                .strip_prefix("https://origin-")
                .unwrap()
                .split('.')
                .next()
                .unwrap()
                .parse()?;
            assert_eq!(
                history.endpoints[0].accepted_at,
                now + Duration::from_secs(group + 16)
            );
        }
        std::fs::remove_dir_all(root)?;
        Ok(())
    }
}
