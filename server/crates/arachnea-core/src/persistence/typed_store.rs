//! Schema-declared, entity-typed persistence contracts and backends.

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    fs,
    hash::Hash,
    io::Write,
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
    /// The single scalar primary key of the entity.
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
    pub fn string(name: impl Into<String>) -> Self {
        Self::new(name, FieldType::String)
    }
    /// Creates a required integer field.
    pub fn integer(name: impl Into<String>) -> Self {
        Self::new(name, FieldType::Integer)
    }
    /// Creates a required boolean field.
    pub fn boolean(name: impl Into<String>) -> Self {
        Self::new(name, FieldType::Boolean)
    }
    /// Creates a required date-time field.
    pub fn date_time(name: impl Into<String>) -> Self {
        Self::new(name, FieldType::DateTime)
    }
    /// Creates a required JSON field.
    pub fn json(name: impl Into<String>) -> Self {
        Self::new(name, FieldType::Json)
    }

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
    /// Adds the schema primary key.
    pub fn primary_key(mut self, mut field: Field) -> Self {
        field.nullable = false;
        field.role = Some(FieldRole::PrimaryKey);
        self.fields.push(field);
        self
    }
    /// Adds a regular field.
    pub fn field(mut self, field: Field) -> Self {
        self.fields.push(field);
        self
    }
    /// Returns all fields in declaration order.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }
    /// Finds one field by its stable name.
    pub fn field_named(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|field| field.name == name)
    }
    /// Returns the primary key declaration.
    pub fn primary_key_field(&self) -> Option<&Field> {
        self.fields
            .iter()
            .find(|field| field.role == Some(FieldRole::PrimaryKey))
    }
    /// Returns the expiration declaration, when the entity has a TTL.
    pub fn expiration_field(&self) -> Option<&Field> {
        self.fields
            .iter()
            .find(|field| field.role == Some(FieldRole::Expiration))
    }

    fn validate(&self) -> anyhow::Result<()> {
        let mut names = std::collections::HashSet::new();
        let primary_keys = self
            .fields
            .iter()
            .filter(|field| field.role == Some(FieldRole::PrimaryKey))
            .count();
        if primary_keys != 1 {
            anyhow::bail!("an entity schema must declare exactly one primary key");
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

/// Explicit identity and schema used to construct one typed persistence store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistenceStoreConfig {
    /// Stable name used for diagnostics and file placement.
    pub name: String,
    /// The one entity schema accepted by this store.
    pub schema: EntitySchema,
}

impl PersistenceStoreConfig {
    /// Creates validated configuration for one named store.
    pub fn new(name: impl Into<String>, schema: EntitySchema) -> anyhow::Result<Self> {
        let name = name.into();
        if name.is_empty() {
            anyhow::bail!("persistence store name must not be empty");
        }
        schema.validate()?;
        Ok(Self { name, schema })
    }
}

/// Scalar key supported by the first typed persistence implementation.
pub trait EntityKey: Clone + Eq + Hash + Debug + Send + Sync + 'static {
    /// Logical schema type of this key.
    fn field_type() -> FieldType;
    /// Stable representation used in file documents.
    fn storage_key(&self) -> String;
}
impl EntityKey for String {
    fn field_type() -> FieldType {
        FieldType::String
    }
    fn storage_key(&self) -> String {
        self.clone()
    }
}
impl EntityKey for i64 {
    fn field_type() -> FieldType {
        FieldType::Integer
    }
    fn storage_key(&self) -> String {
        self.to_string()
    }
}
impl EntityKey for Uuid {
    fn field_type() -> FieldType {
        FieldType::String
    }
    fn storage_key(&self) -> String {
        self.to_string()
    }
}

/// Typed entity stored by a [`TypedEntityStore`].
pub trait PersistentEntity: Clone + Send + Sync + Sized + 'static {
    /// Scalar type used by the entity primary key.
    type Key: EntityKey;
    /// Returns the entity primary key.
    fn key(&self) -> Self::Key;
    /// Returns the complete, stable schema for this entity.
    fn schema() -> EntitySchema;
    /// Writes every declared field to `writer`.
    fn write_to(&self, writer: &mut EntityWriter) -> anyhow::Result<()>;
    /// Rebuilds the entity from a validated entity document.
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
    pub fn new(schema: EntitySchema) -> Self {
        Self {
            schema,
            fields: BTreeMap::new(),
        }
    }
    /// Writes a string field.
    pub fn string(&mut self, field: &str, value: impl Into<String>) -> anyhow::Result<()> {
        self.write(field, StoredValue::String(value.into()))
    }
    /// Writes an integer field.
    pub fn integer(&mut self, field: &str, value: i64) -> anyhow::Result<()> {
        self.write(field, StoredValue::Integer(value))
    }
    /// Writes a boolean field.
    pub fn boolean(&mut self, field: &str, value: bool) -> anyhow::Result<()> {
        self.write(field, StoredValue::Boolean(value))
    }
    /// Writes a date-time field.
    pub fn date_time(&mut self, field: &str, value: SystemTime) -> anyhow::Result<()> {
        self.write(field, StoredValue::DateTime(to_nanos(value)?))
    }
    /// Writes an explicitly declared JSON field.
    pub fn json(&mut self, field: &str, value: Value) -> anyhow::Result<()> {
        self.write(field, StoredValue::Json(value))
    }
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
    pub(crate) fn new(document: &'a EntityDocument) -> Self {
        Self { document }
    }
    /// Reads a required string field.
    pub fn string(&self, field: &str) -> anyhow::Result<&str> {
        match self.get(field)? {
            StoredValue::String(value) => Ok(value),
            _ => unreachable!(),
        }
    }
    /// Reads a required integer field.
    pub fn integer(&self, field: &str) -> anyhow::Result<i64> {
        match self.get(field)? {
            StoredValue::Integer(value) => Ok(*value),
            _ => unreachable!(),
        }
    }
    /// Reads a required boolean field.
    pub fn boolean(&self, field: &str) -> anyhow::Result<bool> {
        match self.get(field)? {
            StoredValue::Boolean(value) => Ok(*value),
            _ => unreachable!(),
        }
    }
    /// Reads a required date-time field.
    pub fn date_time(&self, field: &str) -> anyhow::Result<SystemTime> {
        match self.get(field)? {
            StoredValue::DateTime(value) => from_nanos(*value),
            _ => unreachable!(),
        }
    }
    /// Reads a required JSON field.
    pub fn json(&self, field: &str) -> anyhow::Result<&Value> {
        match self.get(field)? {
            StoredValue::Json(value) => Ok(value),
            _ => unreachable!(),
        }
    }
    fn get(&self, field: &str) -> anyhow::Result<&StoredValue> {
        self.document
            .fields
            .get(field)
            .ok_or_else(|| anyhow::anyhow!("required field '{field}' is absent"))
    }
    /// Reads an optional string field.
    pub fn optional_string(&self, field: &str) -> anyhow::Result<Option<&str>> {
        match self.document.fields.get(field) {
            None => Ok(None),
            Some(StoredValue::String(value)) => Ok(Some(value)),
            Some(_) => anyhow::bail!("field '{field}' must be a string"),
        }
    }
    /// Reads an optional integer field.
    pub fn optional_integer(&self, field: &str) -> anyhow::Result<Option<i64>> {
        match self.document.fields.get(field) {
            None => Ok(None),
            Some(StoredValue::Integer(value)) => Ok(Some(*value)),
            Some(_) => anyhow::bail!("field '{field}' must be an integer"),
        }
    }
    /// Reads an optional boolean field.
    pub fn optional_boolean(&self, field: &str) -> anyhow::Result<Option<bool>> {
        match self.document.fields.get(field) {
            None => Ok(None),
            Some(StoredValue::Boolean(value)) => Ok(Some(*value)),
            Some(_) => anyhow::bail!("field '{field}' must be a boolean"),
        }
    }
    /// Reads an optional date-time field.
    pub fn optional_date_time(&self, field: &str) -> anyhow::Result<Option<SystemTime>> {
        match self.document.fields.get(field) {
            None => Ok(None),
            Some(StoredValue::DateTime(value)) => Ok(Some(from_nanos(*value)?)),
            Some(_) => anyhow::bail!("field '{field}' must be a date-time"),
        }
    }
    /// Reads an optional JSON field.
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
    pub fn where_string(mut self, field: &Field, value: impl Into<String>) -> Self {
        self.predicates
            .push((field.name.clone(), StoredValue::String(value.into())));
        self
    }
    /// Adds an integer equality predicate.
    pub fn where_integer(mut self, field: &Field, value: i64) -> Self {
        self.predicates
            .push((field.name.clone(), StoredValue::Integer(value)));
        self
    }
    /// Adds a boolean equality predicate.
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
    async fn get(&self, key: &E::Key) -> anyhow::Result<Option<E>>;
    /// Validates and writes one entity.
    async fn put(&self, entity: &E) -> anyhow::Result<()>;
    /// Validates all entities before committing the complete batch atomically.
    async fn put_all(&self, entities: &[E]) -> anyhow::Result<()>;
    /// Removes an entity by its primary key.
    async fn delete(&self, key: &E::Key) -> anyhow::Result<()>;
    /// Returns the non-expired entities matching every query predicate.
    async fn find(&self, query: &EntityQuery<E>) -> anyhow::Result<Vec<E>>;
}

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
    if schema
        .primary_key_field()
        .expect("validated schema")
        .field_type
        != E::Key::field_type()
    {
        anyhow::bail!(
            "store '{}' primary key type does not match the entity key",
            config.name
        );
    }
    Ok(())
}
pub(crate) fn encode<E: PersistentEntity>(
    config: &PersistenceStoreConfig,
    entity: &E,
) -> anyhow::Result<EntityDocument> {
    checked_schema::<E>(config)?;
    let mut writer = EntityWriter::new(config.schema.clone());
    entity.write_to(&mut writer)?;
    writer.finish()
}
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
pub(crate) fn decode<E: PersistentEntity>(
    schema: &EntitySchema,
    document: &EntityDocument,
) -> anyhow::Result<E> {
    validate_document(schema, document)?;
    E::read_from(&EntityReader::new(document))
}
fn to_nanos(value: SystemTime) -> anyhow::Result<i128> {
    Ok(value
        .duration_since(UNIX_EPOCH)
        .map_err(|_| anyhow::anyhow!("date-time must not predate UNIX_EPOCH"))?
        .as_nanos() as i128)
}
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
    pub fn new(
        directory: impl Into<PathBuf>,
        config: PersistenceStoreConfig,
    ) -> anyhow::Result<Self> {
        Self::new_with_codec(directory, config, JsonPersistenceFileCodec)
    }
    /// Creates a JSON store under the default application data directory.
    pub fn default_data_dir(config: PersistenceStoreConfig) -> anyhow::Result<Self> {
        Self::new(
            crate::application::get_application_data_path("data"),
            config,
        )
    }
}
impl<E: PersistentEntity, C: PersistenceFileCodec> FileEntityStore<E, C> {
    /// Creates a store using `codec` and explicit store configuration.
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
    async fn document<'a>(
        &self,
        state: &'a mut Option<TypedFileDocument>,
    ) -> anyhow::Result<&'a mut TypedFileDocument> {
        if state.is_none() {
            let path = self.path();
            let codec = Arc::clone(&self.codec);
            *state = Some(
                tokio::task::spawn_blocking(move || match fs::read(&path) {
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
                .map_err(|error| {
                    anyhow::anyhow!("typed persistence read task failed: {error}")
                })??,
            );
        }
        Ok(state.as_mut().expect("typed file document was loaded"))
    }
    fn path(&self) -> PathBuf {
        self.directory.join(format!(
            "store-{}.{}",
            safe_name(&self.config.name),
            self.codec.file_extension().trim_start_matches('.')
        ))
    }
    async fn persist(&self, document: TypedFileDocument) -> anyhow::Result<()> {
        let path = self.path();
        let codec = Arc::clone(&self.codec);
        tokio::task::spawn_blocking(move || persist_file(path, codec, document))
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
fn persist_file<C: PersistenceFileCodec>(
    path: PathBuf,
    codec: Arc<C>,
    document: TypedFileDocument,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            anyhow::anyhow!(
                "failed to create typed persistence directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let payload = codec.serialize(&document)?;
    let temp_path = path.with_extension(format!(
        "{}.{}.tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("data"),
        std::process::id()
    ));
    let mut file = fs::File::create(&temp_path).map_err(|error| {
        anyhow::anyhow!(
            "failed to create temporary typed persistence document {}: {error}",
            temp_path.display()
        )
    })?;
    file.write_all(&payload)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp_path, &path).map_err(|error| {
        anyhow::anyhow!(
            "failed to replace typed persistence document {}: {error}",
            path.display()
        )
    })
}
