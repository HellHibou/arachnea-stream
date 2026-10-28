//! Typed entity document encoding, validation, reading and expiration.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::{
    checked_schema, EntityQuery, EntitySchema, FieldType, PersistenceStoreConfig, PersistentEntity,
};

/// JSON-compatible entity value used only inside persistence adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum StoredValue {
    /// UTF-8 field value.
    String(String),
    /// Signed 64-bit field value.
    Integer(i64),
    /// Boolean field value.
    Boolean(bool),
    /// Nanoseconds since the Unix epoch.
    DateTime(i128),
    /// Unstructured JSON field value.
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
    /// Present field values indexed by their declared schema names.
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
pub(super) fn matches<E: PersistentEntity>(
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
