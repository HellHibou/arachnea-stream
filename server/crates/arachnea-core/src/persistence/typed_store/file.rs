//! File-backed typed entity snapshots and normalization of loaded values.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, marker::PhantomData, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

use super::{
    checked_schema, decode, encode, expired, matches, safe_name, EntityDocument, EntityKey,
    EntityQuery, EntitySchema, FieldType, PersistenceStoreConfig, PersistentEntity, StoredValue,
    TypedEntityStore,
};
use crate::persistence::{JsonPersistenceFileCodec, PersistenceFileCodec};

/// Serialized file document holding entity records by storage key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TypedFileDocument {
    records: HashMap<String, EntityDocument>,
}

/// File-backed typed store. Each write atomically persists a complete store snapshot.
pub struct FileEntityStore<E: PersistentEntity, C = JsonPersistenceFileCodec> {
    config: PersistenceStoreConfig,
    directory: PathBuf,
    codec: Arc<C>,
    state: Mutex<Option<TypedFileDocument>>,
    marker: PhantomData<E>,
}

impl<E: PersistentEntity> FileEntityStore<E, JsonPersistenceFileCodec> {
    /// Creates a JSON store rooted at `directory`.
    ///
    /// # Arguments
    /// * `directory` - Directory containing the store file.
    /// * `config` - Table identity and entity schema accepted by the store.
    ///
    /// # Errors
    /// Returns an error when the schema is invalid or does not match `E`.
    pub fn new(
        directory: impl Into<PathBuf>,
        config: PersistenceStoreConfig,
    ) -> anyhow::Result<Self> {
        Self::new_with_codec(directory, config, JsonPersistenceFileCodec)
    }
    /// Creates a JSON store under the default application data directory.
    ///
    /// The file is stored in the application's `data` directory.
    ///
    /// # Arguments
    /// * `config` - Table identity and entity schema accepted by the store.
    ///
    /// # Errors
    /// Returns an error when the schema is invalid or does not match `E`.
    pub fn default_data_dir(config: PersistenceStoreConfig) -> anyhow::Result<Self> {
        Self::new(
            crate::application::get_application_data_path("data"),
            config,
        )
    }
}
impl<E: PersistentEntity, C: PersistenceFileCodec> FileEntityStore<E, C> {
    /// Creates a store using `codec` and explicit store configuration.
    ///
    /// # Arguments
    /// * `directory` - Directory containing the store file.
    /// * `config` - Table identity and entity schema accepted by the store.
    /// * `codec` - File codec used to load and save the store document.
    ///
    /// # Errors
    /// Returns an error when the schema is invalid or does not match `E`.
    pub fn new_with_codec(
        directory: impl Into<PathBuf>,
        config: PersistenceStoreConfig,
        codec: C,
    ) -> anyhow::Result<Self> {
        checked_schema::<E>(&config)?;
        Ok(Self {
            config,
            directory: directory.into(),
            codec: Arc::new(codec),
            state: Mutex::new(None),
            marker: PhantomData,
        })
    }
    /// Loads the store document on first use and returns the mutable state.
    ///
    /// # Arguments
    /// * `state` - Cached document state filled from disk when empty.
    ///
    /// # Errors
    /// Returns an error when the file cannot be read or decoded, or when the blocking read task fails.
    async fn document<'a>(
        &self,
        state: &'a mut Option<TypedFileDocument>,
    ) -> anyhow::Result<&'a mut TypedFileDocument> {
        if state.is_none() {
            let path = self.path();
            let codec = Arc::clone(&self.codec);
            let mut loaded = tokio::task::spawn_blocking(move || match fs::read(&path) {
                Ok(payload) => codec.deserialize(&payload),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(TypedFileDocument::default())
                }
                Err(error) => Err(anyhow::anyhow!(
                    "failed to read typed persistence document {}: {error}",
                    path.display()
                )),
            })
            .await
            .map_err(|error| anyhow::anyhow!("typed persistence read task failed: {error}"))??;
            normalize_loaded_documents(&mut loaded, &self.config.schema);
            *state = Some(loaded);
        }
        Ok(state.as_mut().expect("typed file document was loaded"))
    }
    /// Returns the file path of the store document.
    fn path(&self) -> PathBuf {
        self.directory.join(format!(
            "store-{}.{}",
            safe_name(&self.config.name),
            self.codec.file_extension().trim_start_matches('.')
        ))
    }
    /// Persists the store document through the configured codec.
    ///
    /// # Arguments
    /// * `document` - Document snapshot to write.
    ///
    /// # Errors
    /// Returns an error when the blocking write task or the codec save fails.
    async fn persist(&self, document: TypedFileDocument) -> anyhow::Result<()> {
        let path = self.path();
        let codec = Arc::clone(&self.codec);
        tokio::task::spawn_blocking(move || codec.save(path, &document))
            .await
            .map_err(|error| anyhow::anyhow!("typed persistence write task failed: {error}"))?
    }
}
#[async_trait]
impl<E: PersistentEntity, C: PersistenceFileCodec> TypedEntityStore<E> for FileEntityStore<E, C> {
    async fn get(&self, key: &E::Key) -> anyhow::Result<Option<E>> {
        let storage_key = key.storage_key();
        let mut state = self.state.lock().await;
        let document = self.document(&mut state).await?;
        let Some(entity) = document.records.get(&storage_key) else {
            return Ok(None);
        };
        if expired(&self.config.schema, entity)? {
            document.records.remove(&storage_key);
            let snapshot = document.clone();
            drop(state);
            self.persist(snapshot).await?;
            return Ok(None);
        }
        Ok(Some(decode(&self.config.schema, entity)?))
    }
    async fn put(&self, entity: &E) -> anyhow::Result<()> {
        let encoded = encode(&self.config, entity)?;
        let mut state = self.state.lock().await;
        let document = self.document(&mut state).await?;
        document.records.insert(entity.key().storage_key(), encoded);
        let snapshot = document.clone();
        drop(state);
        self.persist(snapshot).await
    }
    async fn put_all(&self, entities: &[E]) -> anyhow::Result<()> {
        let encoded: anyhow::Result<Vec<_>> = entities
            .iter()
            .map(|entity| Ok((entity.key().storage_key(), encode(&self.config, entity)?)))
            .collect();
        let encoded = encoded?;
        let mut state = self.state.lock().await;
        let document = self.document(&mut state).await?;
        for (key, entity) in encoded {
            document.records.insert(key, entity);
        }
        let snapshot = document.clone();
        drop(state);
        self.persist(snapshot).await
    }
    async fn delete(&self, key: &E::Key) -> anyhow::Result<()> {
        let mut state = self.state.lock().await;
        let document = self.document(&mut state).await?;
        document.records.remove(&key.storage_key());
        let snapshot = document.clone();
        drop(state);
        self.persist(snapshot).await
    }
    async fn find(&self, query: &EntityQuery<E>) -> anyhow::Result<Vec<E>> {
        let mut state = self.state.lock().await;
        let document = self.document(&mut state).await?;
        let mut result = Vec::new();
        document
            .records
            .retain(|_, entity| !expired(&self.config.schema, entity).unwrap_or(false));
        for entity in document.records.values() {
            if matches(&self.config, entity, query)? {
                result.push(decode(&self.config.schema, entity)?);
            }
        }
        Ok(result)
    }
}
/// Restores the declared logical types lost by the untagged JSON encoding.
///
/// The codec serializes `StoredValue` without type tags, so `DateTime` values
/// read back as integers and scalar `Json` values read back as their inner
/// scalar. Each file belongs to one store schema, which provides the declared
/// type of every field.
///
/// # Arguments
/// * `document` - Loaded document whose field values are normalized in place.
/// * `schema` - Store schema providing the declared field types.
fn normalize_loaded_documents(document: &mut TypedFileDocument, schema: &EntitySchema) {
    for record in document.records.values_mut() {
        for (name, value) in record.fields.iter_mut() {
            let Some(field) = schema.field_named(name) else {
                continue;
            };
            let normalized = match (&field.field_type, &*value) {
                (FieldType::DateTime, StoredValue::Integer(nanos)) => {
                    Some(StoredValue::DateTime(i128::from(*nanos)))
                }
                (FieldType::Json, StoredValue::Integer(number)) => {
                    Some(StoredValue::Json(serde_json::Value::from(*number)))
                }
                (FieldType::Json, StoredValue::Boolean(flag)) => {
                    Some(StoredValue::Json(serde_json::Value::from(*flag)))
                }
                (FieldType::Json, StoredValue::String(text)) => {
                    Some(StoredValue::Json(serde_json::Value::from(text.clone())))
                }
                _ => None,
            };
            if let Some(normalized) = normalized {
                *value = normalized;
            }
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/persistence/typed_store/file.rs"]
mod tests;
