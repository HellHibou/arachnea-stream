//! Persistent source-activation policies.

use anyhow::{Context, Result};
use arachnea_core::persistence::{
    EntityReader, EntitySchema, EntityWriter, Field, PersistentEntity, TypedEntityStore,
};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

use super::ScraperQueryCollectionParameter;

/// Immutable source information provided to an activation policy.
#[derive(Clone, Debug)]
pub struct ScraperSourceDescriptor {
    /// Stable identifier declared by the YAML source.
    pub id: String,
    /// Fully resolved path of the YAML source.
    pub path: PathBuf,
    /// Default activation value declared by the manifest.
    pub default_enabled: bool,
    /// Manifest parameter overrides for this source.
    pub parameters: Vec<ScraperQueryCollectionParameter>,
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

/// Explicit persisted activation override for one source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceEnabledOverride {
    /// Stable source identifier and primary key.
    pub source_id: String,
    /// Administrator-selected activation value.
    pub enabled: bool,
}

impl PersistentEntity for SourceEnabledOverride {
    type Key = String;

    fn key(&self) -> Self::Key {
        self.source_id.clone()
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("source_id"))
            .field(Field::boolean("enabled"))
    }
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()> {
        writer.string("source_id", &self.source_id)?;
        writer.boolean("enabled", self.enabled)
    }
    fn read_from(reader: &EntityReader<'_>) -> Result<Self> {
        Ok(Self {
            source_id: reader.string("source_id")?.to_string(),
            enabled: reader.boolean("enabled")?,
        })
    }
}

/// Repository for source activation overrides.
#[async_trait]
pub trait SourceEnabledRepository: Send + Sync {
    /// Returns the stored override, if any.
    async fn get(&self, source_id: &str) -> Result<Option<SourceEnabledOverride>>;
    /// Writes an override.
    async fn save(&self, override_value: &SourceEnabledOverride) -> Result<()>;
    /// Writes a batch of overrides atomically.
    async fn save_all(&self, overrides: &[SourceEnabledOverride]) -> Result<()>;
    /// Removes an override.
    async fn delete(&self, source_id: &str) -> Result<()>;
}

/// Repository backed by any typed entity store.
pub struct TypedSourceEnabledRepository {
    store: Arc<dyn TypedEntityStore<SourceEnabledOverride>>,
}

impl TypedSourceEnabledRepository {
    /// Creates a repository over a typed entity store.
    pub fn new(store: Arc<dyn TypedEntityStore<SourceEnabledOverride>>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl SourceEnabledRepository for TypedSourceEnabledRepository {
    async fn get(&self, source_id: &str) -> Result<Option<SourceEnabledOverride>> {
        Ok(self.store.get(&source_id.to_string()).await?)
    }
    async fn save(&self, override_value: &SourceEnabledOverride) -> Result<()> {
        Ok(self.store.put(override_value).await?)
    }
    async fn save_all(&self, overrides: &[SourceEnabledOverride]) -> Result<()> {
        Ok(self.store.put_all(overrides).await?)
    }
    async fn delete(&self, source_id: &str) -> Result<()> {
        Ok(self.store.delete(&source_id.to_string()).await?)
    }
}

impl PersistenceSourceEnabled {
    /// Creates a persistent activation policy over a typed entity store.
    pub fn with_typed_store(store: Arc<dyn TypedEntityStore<SourceEnabledOverride>>) -> Self {
        Self::with_repository(Arc::new(TypedSourceEnabledRepository::new(store)))
    }

    /// Creates a policy backed by a source-activation repository.
    pub fn with_repository(repository: Arc<dyn SourceEnabledRepository>) -> Self {
        Self { repository }
    }

    /// Persists an explicit enabled override for a source identifier.
    pub async fn set_enabled(&self, source_id: &str, enabled: bool) -> Result<()> {
        self.repository
            .save(&SourceEnabledOverride {
                source_id: source_id.to_string(),
                enabled,
            })
            .await
    }

    /// Creates missing source records while preserving existing administrator choices.
    pub async fn register_defaults(&self, sources: &[ScraperSourceDescriptor]) -> Result<()> {
        let mut missing = Vec::new();
        for source in sources {
            if self.repository.get(&source.id).await?.is_none() {
                missing.push(SourceEnabledOverride {
                    source_id: source.id.clone(),
                    enabled: source.default_enabled,
                });
            }
        }
        self.repository.save_all(&missing).await
    }

    /// Removes the persisted override of one source identifier.
    ///
    /// The source falls back to its manifest default after this call.
    ///
    /// # Arguments
    /// * `source_id` - Stable source identifier to reset.
    ///
    /// # Errors
    /// Returns an error when the backing store cannot be updated.
    pub async fn clear_enabled(&self, source_id: &str) -> Result<()> {
        self.repository.delete(source_id).await
    }

    /// Returns the persisted override of one source identifier, if any.
    ///
    /// # Errors
    /// Returns an error when the backing store cannot be read.
    pub async fn override_for(&self, source_id: &str) -> Result<Option<SourceEnabledOverride>> {
        self.repository.get(source_id).await
    }
}

#[async_trait]
impl ScraperSourceEnabled for PersistenceSourceEnabled {
    async fn is_enabled(&self, source: &ScraperSourceDescriptor) -> Result<bool> {
        let record = self
            .repository
            .get(&source.id)
            .await
            .with_context(|| format!("Failed to read source activation for {}.", source.id))?;
        Ok(record
            .map(|record| record.enabled)
            .unwrap_or(source.default_enabled))
    }
}
