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
/// Table name for bounded eviction records in the shared proxy SQLite store.
///
/// Both store names are defined in the crate root and re-exported here for
/// path compatibility.
pub use crate::{PROXY_FAILURE_STORE_NAME, PROXY_STORE_NAME};
/// Repository used by [`crate::core::ProxyInventory`].
#[async_trait]
pub trait ProxyRepository: Send + Sync {
    /// Resolves an endpoint without relying on its historical country.
    ///
    /// # Arguments
    /// * `key` - Endpoint identity used to locate the stored proxy record.
    ///
    /// # Errors
    ///
    /// Returns an error if available records or retained failure evidence cannot be read.
    async fn get(&self, key: &ProxyKey) -> Result<Option<ProxyRecord>>;
    /// Loads rediscovery protection retained outside the available inventory.
    ///
    /// # Arguments
    /// * `key` - Endpoint identity used to locate the stored proxy record.
    ///
    /// # Errors
    ///
    /// Returns an error if retained failure evidence cannot be read.
    async fn get_failure(&self, key: &ProxyKey) -> Result<Option<ProxyRecord>>;
    /// Removes an available endpoint after durably recording its failure state.
    ///
    /// # Arguments
    /// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
    ///
    /// # Errors
    ///
    /// Returns an error if failure retention or available-record deletion fails; earlier writes may remain.
    async fn evict(&self, record: &ProxyRecord) -> Result<()>;
    /// Returns the proxies indexed for `country`.
    ///
    /// # Arguments
    /// * `country` - ISO alpha-2 country code associated with the requested inventory partition.
    ///
    /// # Errors
    ///
    /// Returns an error if country records or their failure evidence cannot be read consistently.
    async fn find_by_country(&self, country: &str) -> Result<Vec<ProxyRecord>>;
    /// Merges and persists a batch, preserving newer positive evidence.
    ///
    /// The typed store determines transactional guarantees; this boundary does not
    /// promise batch rollback on a backend write failure.
    ///
    /// # Arguments
    /// * `records` - Proxy records to merge or persist; endpoint identity is host and port.
    ///
    /// # Errors
    ///
    /// Returns an error if existing evidence cannot be read or updated records cannot be saved; store-level rollback depends on the backend.
    async fn save_many(&self, records: &[ProxyRecord]) -> Result<()>;
    /// Clears strictly older `Other`/`Timeout` exclusions for an accepted exact destination.
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
    ///
    /// # Arguments
    /// * `key` - Endpoint identity used to locate the stored proxy record.
    ///
    /// # Errors
    ///
    /// Returns an error if the available-record deletion fails.
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
    ///
    /// # Arguments
    /// * `store` - Typed store containing available proxy records; failure protection defaults to memory.
    ///
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
        /// Weak registry sharing a commit lock between repositories backed by the same inventory store.
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

    /// Filters an already-read record against current durable eviction evidence.
    ///
    /// # Arguments
    /// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
    ///
    async fn filter_available(&self, record: Option<ProxyRecord>) -> Result<Option<ProxyRecord>> {
        let Some(record) = record else {
            return Ok(None);
        };
        let failure = self.get_failure(&record.key()).await?;
        Ok(Self::available_with_failure(record, failure))
    }

    /// Applies the same durable eviction rules to scalar and grouped reads.
    ///
    /// # Arguments
    /// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
    /// * `failure` - Durable eviction evidence associated with the same endpoint.
    ///
    fn available_with_failure(
        record: ProxyRecord,
        failure: Option<ProxyRecord>,
    ) -> Option<ProxyRecord> {
        if let Some(failure) = failure.filter(Self::failure_is_retained) {
            if record.health_evidence.last_positive_at <= failure.health_evidence.evicted_at
                && record.health_evidence.first_discovered_at <= failure.health_evidence.evicted_at
                && record.last_checked <= failure.health_evidence.evicted_at
            {
                return None;
            }
        }
        record
            .health_evidence
            .evicted_at
            .is_none()
            .then_some(record)
    }

    /// Keeps eviction protection for 48 hours after the recorded eviction time.
    ///
    /// # Arguments
    /// * `record` - Proxy record whose endpoint and runtime evidence are inspected or updated.
    fn failure_is_retained(record: &ProxyRecord) -> bool {
        record.health_evidence.evicted_at.is_some_and(|date| {
            SystemTime::now().duration_since(date).unwrap_or_default()
                < std::time::Duration::from_secs(48 * 3600)
        })
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
        self.filter_available(record).await
    }
    async fn get_failure(&self, key: &ProxyKey) -> Result<Option<ProxyRecord>> {
        Ok(self
            .failures
            .get(key)
            .await?
            .filter(Self::failure_is_retained))
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
        let _guard = self.commits.lock().await;
        let records = self
            .store
            .find(&EntityQuery::new().where_string(&ProxyRecord::country_field(), country))
            .await?;
        let keys = records.iter().map(ProxyRecord::key).collect::<Vec<_>>();
        let failures = self.failures.get_many(&keys).await?;
        anyhow::ensure!(
            failures.len() == records.len(),
            "proxy failure store returned an invalid batch length"
        );
        Ok(records
            .into_iter()
            .zip(failures)
            .filter_map(|(record, failure)| Self::available_with_failure(record, failure))
            .collect())
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
/// Returns the persistent identifier of a runtime proxy status.
///
/// # Arguments
/// * `status` - Runtime status converted to its stable persistence identifier.
fn status_name(status: &ProxyRuntimeStatus) -> &'static str {
    match status {
        ProxyRuntimeStatus::Unknown => "unknown",
        ProxyRuntimeStatus::Ok => "ok",
        ProxyRuntimeStatus::Ko => "ko",
        ProxyRuntimeStatus::AuthenticationRequired => "authentication_required",
    }
}
/// Parses a persisted runtime status, rejecting unknown identifiers.
///
/// # Arguments
/// * `value` - Persisted identifier to decode; unknown values return a contextual storage error.
fn parse_status(value: &str) -> Result<ProxyRuntimeStatus> {
    match value {
        "unknown" => Ok(ProxyRuntimeStatus::Unknown),
        "ok" => Ok(ProxyRuntimeStatus::Ok),
        "ko" => Ok(ProxyRuntimeStatus::Ko),
        "authentication_required" => Ok(ProxyRuntimeStatus::AuthenticationRequired),
        _ => anyhow::bail!("unknown persisted proxy status '{value}'"),
    }
}
/// Returns the persistent identifier of a measured capability status.
///
/// # Arguments
/// * `status` - Runtime status converted to its stable persistence identifier.
fn capability_status_name(status: ProxyCapabilityStatus) -> &'static str {
    match status {
        ProxyCapabilityStatus::Unknown => "unknown",
        ProxyCapabilityStatus::Available => "available",
        ProxyCapabilityStatus::Unavailable => "unavailable",
    }
}
/// Parses a persisted capability status, treating absent data as unknown.
///
/// # Arguments
/// * `value` - Persisted identifier to decode; unknown values return a contextual storage error.
fn parse_optional_capability_status(value: Option<&str>) -> Result<ProxyCapabilityStatus> {
    match value {
        None | Some("unknown") => Ok(ProxyCapabilityStatus::Unknown),
        Some("available") => Ok(ProxyCapabilityStatus::Available),
        Some("unavailable") => Ok(ProxyCapabilityStatus::Unavailable),
        Some(value) => anyhow::bail!("unknown persisted proxy capability status '{value}'"),
    }
}
/// Returns the persistent identifier of a provider availability hint.
///
/// # Arguments
/// * `availability` - Provider availability hint converted into its persistent name.
fn availability_name(availability: &ProxyAvailabilityHint) -> &'static str {
    match availability {
        ProxyAvailabilityHint::Unknown => "unknown",
        ProxyAvailabilityHint::Low => "low",
        ProxyAvailabilityHint::Medium => "medium",
        ProxyAvailabilityHint::High => "high",
    }
}
/// Parses a persisted availability hint, rejecting unknown identifiers.
///
/// # Arguments
/// * `value` - Persisted identifier to decode; unknown values return a contextual storage error.
fn parse_availability(value: &str) -> Result<ProxyAvailabilityHint> {
    match value {
        "unknown" => Ok(ProxyAvailabilityHint::Unknown),
        "low" => Ok(ProxyAvailabilityHint::Low),
        "medium" => Ok(ProxyAvailabilityHint::Medium),
        "high" => Ok(ProxyAvailabilityHint::High),
        _ => anyhow::bail!("unknown persisted proxy availability '{value}'"),
    }
}
/// Parses a persisted proxy protocol, rejecting unknown identifiers.
///
/// # Arguments
/// * `value` - Persisted identifier to decode; unknown values return a contextual storage error.
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
#[path = "../../tests/core/proxy_repository.rs"]
mod tests;
