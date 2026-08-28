//! Persistent source-activation policies.

use anyhow::{Context, Result};
use arachnea_core::persistence::{PersistedRecord, PersistenceStore};
use async_trait::async_trait;
use serde_json::{Map, Value};
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

/// Activation policy using a named PersistenceStore namespace.
pub struct PersistenceSourceEnabled {
    store: Arc<dyn PersistenceStore>,
    namespace: String,
}

impl PersistenceSourceEnabled {
    /// Creates a persistent activation policy.
    pub fn new(store: Arc<dyn PersistenceStore>, namespace: impl Into<String>) -> Self {
        Self {
            store,
            namespace: namespace.into(),
        }
    }

    /// Persists an explicit enabled override for a source identifier.
    pub async fn set_enabled(&self, source_id: &str, enabled: bool) -> Result<()> {
        let transaction = self.store.transaction(&self.namespace).await?;
        transaction
            .put(
                source_id.to_string(),
                PersistedRecord {
                    format_version: 1,
                    fields: Some(Map::from_iter([(
                        "enabled".to_string(),
                        Value::Bool(enabled),
                    )])),
                    expires_at: None,
                    updated_at: std::time::SystemTime::now(),
                },
            )
            .await?;
        transaction.commit().await
    }

    /// Creates missing source records while preserving existing administrator choices.
    pub async fn register_defaults(&self, sources: &[ScraperSourceDescriptor]) -> Result<()> {
        let transaction = self.store.transaction(&self.namespace).await?;
        for source in sources {
            if transaction.get(&source.id).await?.is_none() {
                transaction
                    .put(
                        source.id.clone(),
                        PersistedRecord {
                            format_version: 1,
                            fields: Some(Map::from_iter([(
                                "enabled".to_string(),
                                Value::Bool(source.default_enabled),
                            )])),
                            expires_at: None,
                            updated_at: std::time::SystemTime::now(),
                        },
                    )
                    .await?;
            }
        }
        transaction.commit().await
    }
}

#[async_trait]
impl ScraperSourceEnabled for PersistenceSourceEnabled {
    async fn is_enabled(&self, source: &ScraperSourceDescriptor) -> Result<bool> {
        let transaction = self.store.transaction(&self.namespace).await?;
        let record = transaction.get(&source.id).await.with_context(|| {
            format!(
                "Failed to read source activation for {} from {}.",
                source.id, self.namespace
            )
        })?;
        Ok(record
            .and_then(|record| record.fields)
            .and_then(|fields| fields.get("enabled").and_then(Value::as_bool))
            .unwrap_or(source.default_enabled))
    }
}
