//! Entity field declarations, schemas and table configuration.

use serde::{Deserialize, Serialize};

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
    /// Stable field name shared by schemas, entity documents and queries.
    pub(super) name: String,
    /// Logical type accepted by this field's readers and writers.
    pub(super) field_type: FieldType,
    /// Whether an entity document may omit this field.
    pub(super) nullable: bool,
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
    pub(super) fn validate(&self) -> anyhow::Result<()> {
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
