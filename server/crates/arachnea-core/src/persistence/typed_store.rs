//! Schema-declared, entity-typed persistence contracts and backends.

mod document;
mod file;
mod memory;
mod schema;

use document::matches;
pub(crate) use document::{decode, encode, expired, EntityDocument, StoredValue};
pub use document::{EntityReader, EntityWriter};
pub use file::FileEntityStore;
pub use memory::MemoryEntityStore;
pub use schema::{
    EntitySchema, Field, FieldRole, FieldType, PersistenceStoreConfig, PersistenceTableConfig,
};
use std::collections::HashSet;

use async_trait::async_trait;
use std::{fmt::Debug, hash::Hash, marker::PhantomData};
use uuid::Uuid;

/// Stable typed identity used by an entity store.
pub trait EntityKey: Clone + Eq + Hash + Debug + Send + Sync + 'static {
    /// Logical types of the primary-key components, in schema declaration order.
    fn field_types() -> &'static [FieldType];
    /// Canonical, unambiguous representation used in file documents.
    fn storage_key(&self) -> String;
}
impl EntityKey for String {
    fn field_types() -> &'static [FieldType] {
        &[FieldType::String]
    }
    fn storage_key(&self) -> String {
        self.clone()
    }
}
impl EntityKey for i64 {
    fn field_types() -> &'static [FieldType] {
        &[FieldType::Integer]
    }
    fn storage_key(&self) -> String {
        self.to_string()
    }
}
impl EntityKey for Uuid {
    fn field_types() -> &'static [FieldType] {
        &[FieldType::String]
    }
    fn storage_key(&self) -> String {
        self.to_string()
    }
}

/// Typed entity stored by a [`TypedEntityStore`].
pub trait PersistentEntity: Clone + Send + Sync + Sized + 'static {
    /// Type used by the entity primary key.
    type Key: EntityKey;
    /// Returns the entity primary key value.
    fn key(&self) -> Self::Key;
    /// Returns the complete, stable schema for this entity.
    fn schema() -> EntitySchema;
    /// Writes every declared field to `writer`.
    ///
    /// Implementations must write each schema field at most once and must not
    /// write undeclared fields. The store validates the completed document
    /// before persisting it.
    ///
    /// # Arguments
    /// * `writer` - Schema-bound writer that validates field names and types.
    ///
    /// # Errors
    /// Returns an error for an undeclared field, a type mismatch, a duplicate
    /// write, or a value that cannot be represented by the storage format.
    fn write_to(&self, writer: &mut EntityWriter) -> anyhow::Result<()>;
    /// Rebuilds the entity from a validated entity document.
    ///
    /// # Arguments
    /// * `reader` - Schema-bound reader for the stored entity document.
    ///
    /// # Errors
    /// Returns an error when a required field is absent, a field has the wrong
    /// logical type, or a stored value cannot be converted to the expected type.
    fn read_from(reader: &EntityReader<'_>) -> anyhow::Result<Self>;
}

/// A conjunction of equality predicates evaluated by a typed store.
pub struct EntityQuery<E: PersistentEntity> {
    predicates: Vec<(String, StoredValue)>,
    marker: PhantomData<E>,
}
impl<E: PersistentEntity> EntityQuery<E> {
    /// Creates an empty query that matches every non-expired entity.
    pub fn new() -> Self {
        Self {
            predicates: Vec::new(),
            marker: PhantomData,
        }
    }
    /// Adds a string equality predicate.
    ///
    /// The predicate is combined with the other predicates using logical AND.
    /// Field declaration and value type are checked when the query is executed.
    ///
    /// # Arguments
    /// * `field` - Declared field to compare.
    /// * `value` - Expected string value.
    pub fn where_string(mut self, field: &Field, value: impl Into<String>) -> Self {
        self.predicates
            .push((field.name.clone(), StoredValue::String(value.into())));
        self
    }
    /// Adds an integer equality predicate.
    ///
    /// The predicate is combined with the other predicates using logical AND.
    /// Field declaration and value type are checked when the query is executed.
    ///
    /// # Arguments
    /// * `field` - Declared field to compare.
    /// * `value` - Expected integer value.
    pub fn where_integer(mut self, field: &Field, value: i64) -> Self {
        self.predicates
            .push((field.name.clone(), StoredValue::Integer(value)));
        self
    }
    /// Adds a boolean equality predicate.
    ///
    /// The predicate is combined with the other predicates using logical AND.
    /// Field declaration and value type are checked when the query is executed.
    ///
    /// # Arguments
    /// * `field` - Declared field to compare.
    /// * `value` - Expected boolean value.
    pub fn where_boolean(mut self, field: &Field, value: bool) -> Self {
        self.predicates
            .push((field.name.clone(), StoredValue::Boolean(value)));
        self
    }
}
impl<E: PersistentEntity> Default for EntityQuery<E> {
    fn default() -> Self {
        Self::new()
    }
}
#[cfg(feature = "sqlite-persistence")]
impl<E: PersistentEntity> EntityQuery<E> {
    /// Returns the conjunction of equality predicates as `(field, value)` pairs.
    pub(crate) fn predicates(&self) -> &[(String, StoredValue)] {
        &self.predicates
    }
}

/// Object-safe-per-entity contract used by domain repositories.
#[async_trait]
pub trait TypedEntityStore<E>: Send + Sync
where
    E: PersistentEntity,
{
    /// Reads one non-expired entity by its primary key.
    ///
    /// # Arguments
    /// * `key` - Primary key of the entity to read.
    ///
    /// # Errors
    /// Returns a backend read or decoding error. Expired entities are treated as
    /// absent; implementations may purge them as a side effect.
    async fn get(&self, key: &E::Key) -> anyhow::Result<Option<E>>;
    /// Reads non-expired entities in input order, preserving duplicates and absences.
    ///
    /// Reads are not an atomic snapshot across keys. Implementations may purge
    /// expired entries as `get` does, including before a later read fails.
    /// Dropping the future stops further batches; an already started blocking
    /// batch may finish. No results are cached between calls.
    ///
    /// # Arguments
    /// * `keys` - Primary keys to read; an empty slice returns an empty vector.
    ///
    /// # Errors
    /// Returns a backend read/decoding error without a partial result vector.
    async fn get_many(&self, keys: &[E::Key]) -> anyhow::Result<Vec<Option<E>>> {
        let mut results = Vec::with_capacity(keys.len());
        for key in keys {
            results.push(self.get(key).await?);
        }
        Ok(results)
    }
    /// Validates and writes one entity.
    ///
    /// # Arguments
    /// * `entity` - Entity to validate and persist.
    ///
    /// # Errors
    /// Returns an error when the entity does not match the store schema or the
    /// backend cannot persist it.
    async fn put(&self, entity: &E) -> anyhow::Result<()>;
    /// Validates every entity before writing any of them.
    ///
    /// A validation error leaves the store unchanged. Backend write failures are
    /// reported after the batch has been accepted for writing; implementations
    /// are not required to roll back an already-started write.
    ///
    /// # Arguments
    /// * `entities` - Entities to validate and persist.
    ///
    /// # Errors
    /// Returns an error when any entity is invalid or the backend cannot write
    /// the batch.
    async fn put_all(&self, entities: &[E]) -> anyhow::Result<()>;
    /// Atomically replaces all rows in a primary-key scope, including expired rows.
    ///
    /// An empty query replaces the entire table; an empty replacement deletes the
    /// scope. Every predicate must compare a primary-key field. Replacement rows
    /// must match the scope and have unique keys. Concurrent operations observe
    /// either the previous set or the complete replacement, never a partial set.
    /// Dropping the future may leave an already-started backend commit running.
    ///
    /// # Arguments
    /// * `query` - Equality predicates on primary-key components defining the scope.
    /// * `entities` - Complete replacement for that scope.
    ///
    /// # Errors
    /// Returns an error for invalid predicates, invalid or out-of-scope entities,
    /// duplicate keys or a backend failure. Failed validation/writes leave the
    /// prior scope intact; an interrupted commit may have an indeterminate outcome.
    async fn replace_where(&self, query: &EntityQuery<E>, entities: &[E]) -> anyhow::Result<()>;

    /// Removes an entity by its primary key.
    ///
    /// # Arguments
    /// * `key` - Primary key of the entity to remove.
    ///
    /// # Errors
    /// Returns a backend write error. Removing an absent key is not an error.
    async fn delete(&self, key: &E::Key) -> anyhow::Result<()>;
    /// Returns the non-expired entities matching every query predicate.
    ///
    /// Result order is backend-dependent and unspecified. Implementations may
    /// purge expired entries while scanning.
    ///
    /// # Arguments
    /// * `query` - Conjunction of equality predicates to apply.
    ///
    /// # Errors
    /// Returns an error when a predicate references an undeclared field, uses the
    /// wrong value type, or the backend cannot read or decode an entity.
    async fn find(&self, query: &EntityQuery<E>) -> anyhow::Result<Vec<E>>;
}

/// Checks that `config` matches the schema and primary key declared by `E`.
///
/// # Arguments
/// * `config` - Store configuration validated against the entity schema.
///
/// # Errors
/// Returns an error when the schema is invalid, differs from the entity schema, or its primary key does not match `E::Key`.
pub(crate) fn checked_schema<E: PersistentEntity>(
    config: &PersistenceStoreConfig,
) -> anyhow::Result<()> {
    let schema = E::schema();
    schema.validate()?;
    if config.schema != schema {
        anyhow::bail!(
            "store '{}' schema does not match the entity schema",
            config.name
        );
    }
    let primary_key_fields = schema.primary_key_fields();
    if primary_key_fields.len() != E::Key::field_types().len()
        || primary_key_fields
            .iter()
            .zip(E::Key::field_types())
            .any(|(field, field_type)| field.field_type != *field_type)
    {
        anyhow::bail!(
            "store '{}' primary key fields do not match the entity key",
            config.name
        );
    }
    Ok(())
}
/// Percent-encodes characters that are unsafe in a store file name.
///
/// # Arguments
/// * `name` - Store name to encode.
pub(crate) fn safe_name(name: &str) -> String {
    let mut safe = String::with_capacity(name.len());
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' {
            safe.push(byte as char);
        } else {
            safe.push_str(&format!("%{byte:02X}"));
        }
    }
    safe
}

#[cfg(test)]
#[path = "../../tests/support/typed_store.rs"]
mod test_support;

/// Validates an atomic replacement and encodes its rows before any mutation.
///
/// # Arguments
/// * `config` - Target table schema.
/// * `query` - Primary-key scope, validated even when the table/replacement is empty.
/// * `entities` - Complete replacement rows.
///
/// # Errors
/// Returns an error for invalid/non-key predicates, duplicate keys or invalid/out-of-scope rows.
pub(crate) fn replacement_documents<E: PersistentEntity>(
    config: &PersistenceStoreConfig,
    query: &EntityQuery<E>,
    entities: &[E],
) -> anyhow::Result<Vec<(E::Key, EntityDocument)>> {
    for (name, value) in &query.predicates {
        let field = config
            .schema
            .primary_key_fields()
            .iter()
            .find(|field| field.name() == name)
            .copied()
            .ok_or_else(|| {
                anyhow::anyhow!("replacement predicate '{name}' must reference a primary-key field")
            })?;
        anyhow::ensure!(
            field.field_type() == value.field_type(),
            "replacement predicate '{name}' has the wrong type"
        );
    }
    let mut keys = HashSet::new();
    entities
        .iter()
        .map(|entity| {
            let key = entity.key();
            anyhow::ensure!(
                keys.insert(key.clone()),
                "duplicate replacement entity key: {key:?}"
            );
            let document = encode(config, entity)?;
            anyhow::ensure!(
                matches(config, &document, query)?,
                "replacement entity is outside its scope: {key:?}"
            );
            Ok((key, document))
        })
        .collect()
}
