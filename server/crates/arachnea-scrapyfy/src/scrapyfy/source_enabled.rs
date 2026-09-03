//! Persistent source-activation policies.
//!
//! Administrable sources are identified by the composite pair
//! `(service_store_id, source_id)` so two service stores (groups) may declare
//! the same source identifier without colliding in persistence.

use anyhow::{Context, Result};
use arachnea_core::persistence::{
    EntityKey, EntityReader, EntitySchema, EntityWriter, Field, FieldType, PersistentEntity,
    TypedEntityStore,
};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(feature = "sqlite-persistence")]
use arachnea_core::persistence::SqlKey;
#[cfg(feature = "sqlite-persistence")]
use rusqlite::types::{Value as SqlValue, ValueRef};

use super::ScraperQueryCollectionParameter;

/// Immutable source information provided to an activation policy.
#[derive(Clone, Debug)]
pub struct ScraperSourceDescriptor {
    /// Service store (group) owning the source.
    pub service_store_id: String,
    /// Stable identifier declared by the YAML source.
    pub id: String,
    /// Fully resolved path of the YAML source.
    pub path: PathBuf,
    /// Default activation value declared by the manifest.
    pub default_enabled: bool,
    /// Manifest parameter overrides for this source.
    pub parameters: Vec<ScraperQueryCollectionParameter>,
}

/// Composite identity of one administrable source.
///
/// The pair is ordered like the entity schema primary key:
/// `service_store_id` first, then `source_id`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SourceServiceKey {
    /// Service store (group) identifier owning the source.
    pub service_store_id: String,
    /// Stable YAML identifier of the source.
    pub source_id: String,
}

impl SourceServiceKey {
    /// Creates a composite source identity.
    pub fn new(service_store_id: impl Into<String>, source_id: impl Into<String>) -> Self {
        Self {
            service_store_id: service_store_id.into(),
            source_id: source_id.into(),
        }
    }
}

impl EntityKey for SourceServiceKey {
    fn field_types() -> &'static [FieldType] {
        &[FieldType::String, FieldType::String]
    }

    fn storage_key(&self) -> String {
        // Length-prefixed first component keeps the encoding unambiguous.
        format!(
            "{}:{}:{}",
            self.service_store_id.len(),
            self.service_store_id,
            self.source_id
        )
    }
}

#[cfg(feature = "sqlite-persistence")]
impl SqlKey for SourceServiceKey {
    fn physical_types() -> &'static [&'static str] {
        &["TEXT", "TEXT"]
    }

    fn to_sql_values(&self) -> Vec<SqlValue> {
        vec![
            SqlValue::Text(self.service_store_id.clone()),
            SqlValue::Text(self.source_id.clone()),
        ]
    }

    fn from_sql_value(index: usize, cell: ValueRef<'_>) -> anyhow::Result<SqlValue> {
        match (index, cell) {
            (0 | 1, ValueRef::Text(value)) => {
                Ok(SqlValue::Text(std::str::from_utf8(value)?.to_string()))
            }
            _ => anyhow::bail!("source service primary key cell has an unexpected SQLite type"),
        }
    }
}

/// Determines whether a configured source must be loaded.
#[async_trait]
pub trait ScraperSourceEnabled: Send + Sync {
    /// Returns the effective enabled state for one source.
    async fn is_enabled(&self, source: &ScraperSourceDescriptor) -> Result<bool>;
}

/// Activation policy backed by a typed source-activation repository.
pub struct PersistenceSourceEnabled {
    repository: Arc<dyn SourceEnabledRepository>,
}

/// Persisted per-service record combining the activation override with the
/// encrypted service credentials.
///
/// The record is keyed by `(service_store_id, source_id)` so the same source
/// identifier may exist in several service stores.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceServiceRecord {
    /// Service store (group) identifier; first primary-key component.
    pub service_store_id: String,
    /// Stable source identifier; second primary-key component.
    pub source_id: String,
    /// Administrator-selected activation value.
    pub enabled: bool,
    /// AES-256-GCM encrypted login (`base64(nonce || ciphertext)`), when set.
    ///
    /// The field is opaque to this crate: encryption lives in the application
    /// adapter implementing [`arachnea_core::persistence::CredentialsStore`].
    pub login: Option<String>,
    /// AES-256-GCM encrypted password (`base64(nonce || ciphertext)`), when set.
    pub password: Option<String>,
}

impl PersistentEntity for SourceServiceRecord {
    type Key = SourceServiceKey;

    fn key(&self) -> Self::Key {
        SourceServiceKey {
            service_store_id: self.service_store_id.clone(),
            source_id: self.source_id.clone(),
        }
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("service_store_id"))
            .primary_key(Field::string("source_id"))
            .field(Field::boolean("enabled"))
            .field(Field::string("login").nullable())
            .field(Field::string("password").nullable())
    }
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()> {
        writer.string("service_store_id", &self.service_store_id)?;
        writer.string("source_id", &self.source_id)?;
        writer.boolean("enabled", self.enabled)?;
        if let Some(login) = &self.login {
            writer.string("login", login)?;
        }
        if let Some(password) = &self.password {
            writer.string("password", password)?;
        }
        Ok(())
    }
    fn read_from(reader: &EntityReader<'_>) -> Result<Self> {
        Ok(Self {
            service_store_id: reader.string("service_store_id")?.to_string(),
            source_id: reader.string("source_id")?.to_string(),
            enabled: reader.boolean("enabled")?,
            login: reader.optional_string("login")?.map(str::to_string),
            password: reader.optional_string("password")?.map(str::to_string),
        })
    }
}

/// Repository for source service records (activation and credentials).
#[async_trait]
pub trait SourceEnabledRepository: Send + Sync {
    /// Returns the stored record, if any.
    async fn get(&self, key: &SourceServiceKey) -> Result<Option<SourceServiceRecord>>;
    /// Writes a record.
    async fn save(&self, record: &SourceServiceRecord) -> Result<()>;
    /// Writes a batch of records atomically.
    async fn save_all(&self, records: &[SourceServiceRecord]) -> Result<()>;
    /// Removes a record.
    async fn delete(&self, key: &SourceServiceKey) -> Result<()>;
}

/// Repository backed by any typed entity store.
pub struct TypedSourceEnabledRepository {
    store: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
}

impl TypedSourceEnabledRepository {
    /// Creates a repository over a typed entity store.
    pub fn new(store: Arc<dyn TypedEntityStore<SourceServiceRecord>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl SourceEnabledRepository for TypedSourceEnabledRepository {
    async fn get(&self, key: &SourceServiceKey) -> Result<Option<SourceServiceRecord>> {
        Ok(self.store.get(key).await?)
    }
    async fn save(&self, record: &SourceServiceRecord) -> Result<()> {
        Ok(self.store.put(record).await?)
    }
    async fn save_all(&self, records: &[SourceServiceRecord]) -> Result<()> {
        Ok(self.store.put_all(records).await?)
    }
    async fn delete(&self, key: &SourceServiceKey) -> Result<()> {
        Ok(self.store.delete(key).await?)
    }
}

impl PersistenceSourceEnabled {
    /// Creates a persistent activation policy over a typed entity store.
    pub fn with_typed_store(store: Arc<dyn TypedEntityStore<SourceServiceRecord>>) -> Self {
        Self::with_repository(Arc::new(TypedSourceEnabledRepository::new(store)))
    }

    /// Creates a policy backed by a source-activation repository.
    pub fn with_repository(repository: Arc<dyn SourceEnabledRepository>) -> Self {
        Self { repository }
    }

    /// Persists an explicit enabled override for one namespaced source.
    ///
    /// Existing encrypted credentials recorded on the same service record are
    /// preserved; only the activation value is updated.
    pub async fn set_enabled(&self, key: &SourceServiceKey, enabled: bool) -> Result<()> {
        let mut record = match self.repository.get(key).await? {
            Some(record) => record,
            None => SourceServiceRecord {
                service_store_id: key.service_store_id.clone(),
                source_id: key.source_id.clone(),
                enabled,
                login: None,
                password: None,
            },
        };
        record.enabled = enabled;
        self.repository.save(&record).await
    }

    /// Creates missing source records while preserving existing administrator choices.
    pub async fn register_defaults(&self, sources: &[ScraperSourceDescriptor]) -> Result<()> {
        let mut missing = Vec::new();
        for source in sources {
            let key = SourceServiceKey::new(&source.service_store_id, &source.id);
            if self.repository.get(&key).await?.is_none() {
                missing.push(SourceServiceRecord {
                    service_store_id: source.service_store_id.clone(),
                    source_id: source.id.clone(),
                    enabled: source.default_enabled,
                    login: None,
                    password: None,
                });
            }
        }
        self.repository.save_all(&missing).await
    }

    /// Removes the persisted override of one namespaced source.
    ///
    /// The source falls back to its manifest default after this call.
    ///
    /// # Arguments
    /// * `key` - Composite identity of the source to reset.
    ///
    /// # Errors
    /// Returns an error when the backing store cannot be updated.
    pub async fn clear_enabled(&self, key: &SourceServiceKey) -> Result<()> {
        self.repository.delete(key).await
    }

    /// Returns the persisted record of one namespaced source, if any.
    ///
    /// # Errors
    /// Returns an error when the backing store cannot be read.
    pub async fn override_for(
        &self,
        key: &SourceServiceKey,
    ) -> Result<Option<SourceServiceRecord>> {
        self.repository.get(key).await
    }
}

#[async_trait]
impl ScraperSourceEnabled for PersistenceSourceEnabled {
    async fn is_enabled(&self, source: &ScraperSourceDescriptor) -> Result<bool> {
        let key = SourceServiceKey::new(&source.service_store_id, &source.id);
        let record = self.repository.get(&key).await.with_context(|| {
            format!(
                "Failed to read source activation for {}/{}.",
                source.service_store_id, source.id
            )
        })?;
        Ok(record
            .map(|record| record.enabled)
            .unwrap_or(source.default_enabled))
    }
}
