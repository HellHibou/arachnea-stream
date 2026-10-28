//! Schema-declared, entity-typed persistence contracts and backends.

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    fs,
    hash::Hash,
    marker::PhantomData,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::{JsonPersistenceFileCodec, PersistenceFileCodec};

/// Logical type accepted by a field in an [`EntitySchema`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldType {
    /// UTF-8 string.
    String,
    /// Signed 64-bit integer.
    Integer,
    /// Boolean value.
    Boolean,
    /// UTC timestamp represented internally with nanosecond precision.
    DateTime,
    /// Deliberately unstructured JSON payload.
    Json,
}

/// Extra semantics assigned to a schema field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldRole {
    /// A component of the entity primary key.
    PrimaryKey,
    /// Timestamp after which the entity is no longer returned.
    Expiration,
}

/// A named, schema-declared entity field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    name: String,
    field_type: FieldType,
    nullable: bool,
    indexed: bool,
    role: Option<FieldRole>,
}

impl Field {
    /// Creates a required string field.
    ///
    /// # Arguments
    /// * `name` - Stable field name used by the schema, documents and queries.
    pub fn string(name: impl Into<String>) -> Self {
        Self::new(name, FieldType::String)
    }
    /// Creates a required integer field.
    ///
    /// # Arguments
    /// * `name` - Stable field name used by the schema, documents and queries.
    pub fn integer(name: impl Into<String>) -> Self {
        Self::new(name, FieldType::Integer)
    }
    /// Creates a required boolean field.
    ///
    /// # Arguments
    /// * `name` - Stable field name used by the schema, documents and queries.
    pub fn boolean(name: impl Into<String>) -> Self {
        Self::new(name, FieldType::Boolean)
    }
    /// Creates a required date-time field.
    ///
    /// # Arguments
    /// * `name` - Stable field name used by the schema, documents and queries.
    pub fn date_time(name: impl Into<String>) -> Self {
        Self::new(name, FieldType::DateTime)
    }
    /// Creates a required JSON field.
    ///
    /// # Arguments
    /// * `name` - Stable field name used by the schema, documents and queries.
    pub fn json(name: impl Into<String>) -> Self {
        Self::new(name, FieldType::Json)
    }

    /// Creates a required field with the given logical type.
    ///
    /// # Arguments
    /// * `name` - Stable field name used by the schema, documents and queries.
    /// * `field_type` - Logical type expected for the field.
    fn new(name: impl Into<String>, field_type: FieldType) -> Self {
        Self {
            name: name.into(),
            field_type,
            nullable: false,
            indexed: false,
            role: None,
        }
    }
    /// Allows this field to be absent from an entity document.
    pub fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }
    /// Declares an index for this field.
    pub fn indexed(mut self) -> Self {
        self.indexed = true;
        self
    }
    /// Declares this field as the entity expiration timestamp.
    pub fn expiration(mut self) -> Self {
        self.role = Some(FieldRole::Expiration);
        self
    }
    /// Returns the stable field name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Returns the field's logical type.
    pub fn field_type(&self) -> FieldType {
        self.field_type
    }
    /// Returns whether an index is declared for this field.
    pub fn is_indexed(&self) -> bool {
        self.indexed
    }
    /// Returns whether this field may be absent.
    pub fn is_nullable(&self) -> bool {
        self.nullable
    }
    /// Returns the extra semantics assigned to this field, if any.
    pub fn role(&self) -> Option<FieldRole> {
        self.role
    }
}

/// Explicit schema for the sole entity contained by a typed store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntitySchema {
    fields: Vec<Field>,
}

impl EntitySchema {
    /// Creates an empty schema. A primary key must be added before use.
    pub fn new() -> Self {
        Self { fields: Vec::new() }
    }
    /// Adds one primary-key component in declaration order.
    ///
    /// The field is forced to be non-nullable and its role is replaced with
    /// [`FieldRole::PrimaryKey`], including when it was previously marked as an
    /// expiration field.
    ///
    /// # Arguments
    /// * `field` - Field declaration to append to the primary key.
    pub fn primary_key(mut self, mut field: Field) -> Self {
        field.nullable = false;
        field.role = Some(FieldRole::PrimaryKey);
        self.fields.push(field);
        self
    }
    /// Adds a regular field.
    ///
    /// Fields keep declaration order. Names must be non-empty and unique when
    /// the schema is validated.
    ///
    /// # Arguments
    /// * `field` - Field declaration to append.
    pub fn field(mut self, field: Field) -> Self {
        self.fields.push(field);
        self
    }
    /// Returns all fields in declaration order.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }
    /// Finds one field by its stable name.
    ///
    /// # Arguments
    /// * `name` - Stable field name to look up.
    pub fn field_named(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|field| field.name == name)
    }
    /// Returns the primary-key declarations in component order.
    pub fn primary_key_fields(&self) -> Vec<&Field> {
        self.fields
            .iter()
            .filter(|field| field.role == Some(FieldRole::PrimaryKey))
            .collect()
    }
    /// Returns the expiration declaration, when the entity has a TTL.
    pub fn expiration_field(&self) -> Option<&Field> {
        self.fields
            .iter()
            .find(|field| field.role == Some(FieldRole::Expiration))
    }

    /// Validates that the schema has unique names, a primary key and valid roles.
    fn validate(&self) -> anyhow::Result<()> {
        let mut names = std::collections::HashSet::new();
        let primary_keys = self
            .fields
            .iter()
            .filter(|field| field.role == Some(FieldRole::PrimaryKey))
            .count();
        if primary_keys == 0 {
            anyhow::bail!("an entity schema must declare at least one primary key field");
        }
        for field in &self.fields {
            if field.name.is_empty() {
                anyhow::bail!("entity schema fields must have a name");
            }
            if !names.insert(&field.name) {
                anyhow::bail!(
                    "entity schema declares field '{}' more than once",
                    field.name
                );
            }
            if field.role == Some(FieldRole::Expiration) && field.field_type != FieldType::DateTime
            {
                anyhow::bail!("expiration field '{}' must be a date-time", field.name);
            }
        }
        Ok(())
    }
}

impl Default for EntitySchema {
    fn default() -> Self {
        Self::new()
    }
}

/// Explicit table identity and entity schema, independent of a physical database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistenceTableConfig {
    /// Stable table name (also used for standalone memory/file stores).
    pub name: String,
    /// The one entity schema accepted by this table.
    pub schema: EntitySchema,
}

impl PersistenceTableConfig {
    /// Creates validated configuration for one named entity table.
    ///
    /// # Arguments
    /// * `name` - Non-empty table name, independent of the physical SQLite store name.
    /// * `schema` - Complete schema for the entity persisted in this table.
    ///
    /// # Errors
    /// Returns an error when the name is empty or the entity schema is invalid.
    pub fn new(name: impl Into<String>, schema: EntitySchema) -> anyhow::Result<Self> {
        let name = name.into();
        if name.is_empty() {
            anyhow::bail!("persistence table name must not be empty");
        }
        schema.validate()?;
        Ok(Self { name, schema })
    }
}

/// Compatibility name for an entity table configuration, not a physical SQLite store.
pub type PersistenceStoreConfig = PersistenceTableConfig;

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

/// JSON-compatible entity value used only inside persistence adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum StoredValue {
    String(String),
    Integer(i64),
    Boolean(bool),
    DateTime(i128),
    Json(Value),
}

impl StoredValue {
    /// Returns the logical schema type represented by this value.
    pub(crate) fn field_type(&self) -> FieldType {
        match self {
            Self::String(_) => FieldType::String,
            Self::Integer(_) => FieldType::Integer,
            Self::Boolean(_) => FieldType::Boolean,
            Self::DateTime(_) => FieldType::DateTime,
            Self::Json(_) => FieldType::Json,
        }
    }
}

/// Internal, serialized form of one entity. It is never exposed to domains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EntityDocument {
    pub(crate) fields: BTreeMap<String, StoredValue>,
}

/// Schema-validating sink used by [`PersistentEntity::write_to`].
pub struct EntityWriter {
    schema: EntitySchema,
    fields: BTreeMap<String, StoredValue>,
}

impl EntityWriter {
    /// Creates a writer bound to `schema`.
    ///
    /// # Arguments
    /// * `schema` - Entity schema used to validate field names and types.
    pub fn new(schema: EntitySchema) -> Self {
        Self {
            schema,
            fields: BTreeMap::new(),
        }
    }
    /// Writes a string field.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    /// * `value` - String value to store.
    ///
    /// # Errors
    /// Returns an error when the field is undeclared, expects another type, or
    /// has already been written.
    pub fn string(&mut self, field: &str, value: impl Into<String>) -> anyhow::Result<()> {
        self.write(field, StoredValue::String(value.into()))
    }
    /// Writes an integer field.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    /// * `value` - Signed 64-bit integer to store.
    ///
    /// # Errors
    /// Returns an error when the field is undeclared, expects another type, or
    /// has already been written.
    pub fn integer(&mut self, field: &str, value: i64) -> anyhow::Result<()> {
        self.write(field, StoredValue::Integer(value))
    }
    /// Writes a boolean field.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    /// * `value` - Boolean value to store.
    ///
    /// # Errors
    /// Returns an error when the field is undeclared, expects another type, or
    /// has already been written.
    pub fn boolean(&mut self, field: &str, value: bool) -> anyhow::Result<()> {
        self.write(field, StoredValue::Boolean(value))
    }
    /// Writes a date-time field.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    /// * `value` - Timestamp to store with nanosecond precision.
    ///
    /// # Errors
    /// Returns an error when the field is undeclared, expects another type, has
    /// already been written, or the timestamp predates [`UNIX_EPOCH`].
    pub fn date_time(&mut self, field: &str, value: SystemTime) -> anyhow::Result<()> {
        self.write(field, StoredValue::DateTime(to_nanos(value)?))
    }
    /// Writes an explicitly declared JSON field.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    /// * `value` - JSON value to store.
    ///
    /// # Errors
    /// Returns an error when the field is undeclared, expects another type, or
    /// has already been written.
    pub fn json(&mut self, field: &str, value: Value) -> anyhow::Result<()> {
        self.write(field, StoredValue::Json(value))
    }
    /// Writes one value after validating its declaration and logical type.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    /// * `value` - Stored value whose logical type must match the declaration.
    ///
    /// # Errors
    /// Returns an error when the field is undeclared, has another type, or was already written.
    fn write(&mut self, field: &str, value: StoredValue) -> anyhow::Result<()> {
        let declaration = self.schema.field_named(field).ok_or_else(|| {
            anyhow::anyhow!("field '{field}' is not declared in the entity schema")
        })?;
        if declaration.field_type != value.field_type() {
            anyhow::bail!(
                "field '{field}' expects {:?}, got {:?}",
                declaration.field_type,
                value.field_type()
            );
        }
        if self.fields.insert(field.to_string(), value).is_some() {
            anyhow::bail!("field '{field}' was written more than once");
        }
        Ok(())
    }
    /// Completes the document after checking that all required fields exist.
    fn finish(self) -> anyhow::Result<EntityDocument> {
        for field in self.schema.fields() {
            if !field.nullable && !self.fields.contains_key(&field.name) {
                anyhow::bail!("required field '{}' was not written", field.name);
            }
        }
        Ok(EntityDocument {
            fields: self.fields,
        })
    }
}

/// Schema-validating source used by [`PersistentEntity::read_from`].
pub struct EntityReader<'a> {
    document: &'a EntityDocument,
}

impl<'a> EntityReader<'a> {
    /// Creates a reader over a validated entity document.
    ///
    /// # Arguments
    /// * `document` - Entity document previously validated against the schema.
    pub(crate) fn new(document: &'a EntityDocument) -> Self {
        Self { document }
    }
    /// Reads a required string field.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    ///
    /// # Errors
    /// Returns an error when the field is absent. The store validates declared
    /// types before invoking the entity reader.
    pub fn string(&self, field: &str) -> anyhow::Result<&str> {
        match self.get(field)? {
            StoredValue::String(value) => Ok(value),
            _ => unreachable!(),
        }
    }
    /// Reads a required integer field.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    ///
    /// # Errors
    /// Returns an error when the field is absent. The store validates declared
    /// types before invoking the entity reader.
    pub fn integer(&self, field: &str) -> anyhow::Result<i64> {
        match self.get(field)? {
            StoredValue::Integer(value) => Ok(*value),
            _ => unreachable!(),
        }
    }
    /// Reads a required boolean field.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    ///
    /// # Errors
    /// Returns an error when the field is absent. The store validates declared
    /// types before invoking the entity reader.
    pub fn boolean(&self, field: &str) -> anyhow::Result<bool> {
        match self.get(field)? {
            StoredValue::Boolean(value) => Ok(*value),
            _ => unreachable!(),
        }
    }
    /// Reads a required date-time field.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    ///
    /// # Errors
    /// Returns an error when the field is absent or stores a timestamp outside
    /// the supported range. The store validates declared types before invoking
    /// the entity reader.
    pub fn date_time(&self, field: &str) -> anyhow::Result<SystemTime> {
        match self.get(field)? {
            StoredValue::DateTime(value) => from_nanos(*value),
            _ => unreachable!(),
        }
    }
    /// Reads a required JSON field.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    ///
    /// # Errors
    /// Returns an error when the field is absent. The store validates declared
    /// types before invoking the entity reader.
    pub fn json(&self, field: &str) -> anyhow::Result<&Value> {
        match self.get(field)? {
            StoredValue::Json(value) => Ok(value),
            _ => unreachable!(),
        }
    }
    /// Returns a required stored value or an error when it is absent.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    ///
    /// # Errors
    /// Returns an error when the field is absent from the document.
    fn get(&self, field: &str) -> anyhow::Result<&StoredValue> {
        self.document
            .fields
            .get(field)
            .ok_or_else(|| anyhow::anyhow!("required field '{field}' is absent"))
    }
    /// Reads an optional string field.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    ///
    /// # Errors
    /// Returns an error when the field is present but does not contain a string.
    pub fn optional_string(&self, field: &str) -> anyhow::Result<Option<&str>> {
        match self.document.fields.get(field) {
            None => Ok(None),
            Some(StoredValue::String(value)) => Ok(Some(value)),
            Some(_) => anyhow::bail!("field '{field}' must be a string"),
        }
    }
    /// Reads an optional integer field.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    ///
    /// # Errors
    /// Returns an error when the field is present but does not contain an integer.
    pub fn optional_integer(&self, field: &str) -> anyhow::Result<Option<i64>> {
        match self.document.fields.get(field) {
            None => Ok(None),
            Some(StoredValue::Integer(value)) => Ok(Some(*value)),
            Some(_) => anyhow::bail!("field '{field}' must be an integer"),
        }
    }
    /// Reads an optional boolean field.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    ///
    /// # Errors
    /// Returns an error when the field is present but does not contain a boolean.
    pub fn optional_boolean(&self, field: &str) -> anyhow::Result<Option<bool>> {
        match self.document.fields.get(field) {
            None => Ok(None),
            Some(StoredValue::Boolean(value)) => Ok(Some(*value)),
            Some(_) => anyhow::bail!("field '{field}' must be a boolean"),
        }
    }
    /// Reads an optional date-time field.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    ///
    /// # Errors
    /// Returns an error when the field is present but is not a date-time or
    /// stores a timestamp outside the supported range.
    pub fn optional_date_time(&self, field: &str) -> anyhow::Result<Option<SystemTime>> {
        match self.document.fields.get(field) {
            None => Ok(None),
            Some(StoredValue::DateTime(value)) => Ok(Some(from_nanos(*value)?)),
            Some(_) => anyhow::bail!("field '{field}' must be a date-time"),
        }
    }
    /// Reads an optional JSON field.
    ///
    /// # Arguments
    /// * `field` - Declared field name.
    ///
    /// # Errors
    /// Returns an error when the field is present but does not contain JSON.
    pub fn optional_json(&self, field: &str) -> anyhow::Result<Option<&Value>> {
        match self.document.fields.get(field) {
            None => Ok(None),
            Some(StoredValue::Json(value)) => Ok(Some(value)),
            Some(_) => anyhow::bail!("field '{field}' must be a json value"),
        }
    }
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
/// Validates `entity` against `config` and encodes it as an entity document.
///
/// # Arguments
/// * `config` - Store configuration whose schema is validated.
/// * `entity` - Entity written into the resulting document.
///
/// # Errors
/// Returns an error when the configuration or entity violates the schema, or when writing a field fails.
pub(crate) fn encode<E: PersistentEntity>(
    config: &PersistenceStoreConfig,
    entity: &E,
) -> anyhow::Result<EntityDocument> {
    checked_schema::<E>(config)?;
    let mut writer = EntityWriter::new(config.schema.clone());
    entity.write_to(&mut writer)?;
    writer.finish()
}
/// Returns whether `document` has an expiration field at or before now.
///
/// # Arguments
/// * `schema` - Entity schema declaring the optional expiration field.
/// * `document` - Entity document whose expiration value is checked.
///
/// # Errors
/// Returns an error when the expiration value is not a valid date-time.
pub(crate) fn expired(schema: &EntitySchema, document: &EntityDocument) -> anyhow::Result<bool> {
    let Some(field) = schema.expiration_field() else {
        return Ok(false);
    };
    match document.fields.get(&field.name) {
        None => Ok(false),
        Some(StoredValue::DateTime(value)) => Ok(from_nanos(*value)? <= SystemTime::now()),
        Some(_) => anyhow::bail!("expiration field '{}' has an invalid value", field.name),
    }
}
/// Evaluates all equality predicates in `query` against `document`.
///
/// # Arguments
/// * `config` - Store configuration providing the schema.
/// * `document` - Entity document tested against the predicates.
/// * `query` - Conjunction of equality predicates.
///
/// # Errors
/// Returns an error when a predicate references an undeclared field or uses the wrong value type.
fn matches<E: PersistentEntity>(
    config: &PersistenceStoreConfig,
    document: &EntityDocument,
    query: &EntityQuery<E>,
) -> anyhow::Result<bool> {
    for (name, expected) in &query.predicates {
        let field = config
            .schema
            .field_named(name)
            .ok_or_else(|| anyhow::anyhow!("query field '{name}' is not declared"))?;
        if field.field_type != expected.field_type() {
            anyhow::bail!("query value for '{name}' has the wrong type");
        }
        if document.fields.get(name) != Some(expected) {
            return Ok(false);
        }
    }
    Ok(true)
}
/// Checks document fields against the schema's names, types and required fields.
///
/// # Arguments
/// * `schema` - Entity schema expected for the document.
/// * `document` - Entity document to validate.
///
/// # Errors
/// Returns an error for undeclared fields, type mismatches, or missing required fields.
fn validate_document(schema: &EntitySchema, document: &EntityDocument) -> anyhow::Result<()> {
    for (name, value) in &document.fields {
        let field = schema
            .field_named(name)
            .ok_or_else(|| anyhow::anyhow!("entity document contains undeclared field '{name}'"))?;
        if field.field_type != value.field_type() {
            anyhow::bail!(
                "entity document field '{name}' expects {:?}, got {:?}",
                field.field_type,
                value.field_type()
            );
        }
    }
    for field in schema.fields() {
        if !field.nullable && !document.fields.contains_key(&field.name) {
            anyhow::bail!("entity document is missing required field '{}'", field.name);
        }
    }
    Ok(())
}
/// Validates an entity document and rebuilds the entity through `E::read_from`.
///
/// # Arguments
/// * `schema` - Entity schema expected for the document.
/// * `document` - Entity document decoded into `E`.
///
/// # Errors
/// Returns an error when validation fails or the entity reader rejects a field.
pub(crate) fn decode<E: PersistentEntity>(
    schema: &EntitySchema,
    document: &EntityDocument,
) -> anyhow::Result<E> {
    validate_document(schema, document)?;
    E::read_from(&EntityReader::new(document))
}
/// Converts a timestamp to nanoseconds since [`UNIX_EPOCH`].
///
/// # Arguments
/// * `value` - Timestamp to convert.
///
/// # Errors
/// Returns an error when the timestamp predates [`UNIX_EPOCH`].
fn to_nanos(value: SystemTime) -> anyhow::Result<i128> {
    Ok(value
        .duration_since(UNIX_EPOCH)
        .map_err(|_| anyhow::anyhow!("date-time must not predate UNIX_EPOCH"))?
        .as_nanos() as i128)
}
/// Converts nanoseconds since [`UNIX_EPOCH`] to a timestamp.
///
/// # Arguments
/// * `value` - Nanoseconds elapsed since [`UNIX_EPOCH`].
///
/// # Errors
/// Returns an error when the value is negative or does not fit the supported duration range.
fn from_nanos(value: i128) -> anyhow::Result<SystemTime> {
    if value < 0 {
        anyhow::bail!("date-time must not predate UNIX_EPOCH");
    }
    Ok(UNIX_EPOCH
        + Duration::from_nanos(
            u64::try_from(value).map_err(|_| anyhow::anyhow!("date-time is out of range"))?,
        ))
}

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

/// Serialized file document holding entity records by storage key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TypedFileDocument {
    records: HashMap<String, EntityDocument>,
}

/// File-backed typed store. Each write is atomically persisted as one entity document.
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
#[path = "../../tests/persistence/typed_store.rs"]
mod tests;
