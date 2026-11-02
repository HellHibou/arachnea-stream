//! Normalized parameter rows and atomic per-source replacement.

use anyhow::Result;
use arachnea_core::persistence::{
    EntityKey, EntityQuery, EntityReader, EntitySchema, EntityWriter, Field, FieldType,
    MemoryEntityStore, PersistenceStoreConfig, PersistentEntity, TypedEntityStore,
};
use std::{collections::HashMap, sync::Arc};

use super::SourceServiceKey;

#[cfg(feature = "sqlite-persistence")]
use arachnea_core::persistence::{PersistenceTableConfig, SqlKey, SqliteDatabase};
#[cfg(feature = "sqlite-persistence")]
use rusqlite::types::{Value as SqlValue, ValueRef};

/// Unique identity of one administrator parameter within a source namespace.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ServiceParameterKey {
    /// Group and source owning the parameter.
    pub source: SourceServiceKey,
    /// Exact parameter name declared by the YAML.
    pub parameter_name: String,
}

impl EntityKey for ServiceParameterKey {
    fn field_types() -> &'static [FieldType] {
        &[FieldType::String, FieldType::String, FieldType::String]
    }
    fn storage_key(&self) -> String {
        let source = self.source.storage_key();
        format!("{}:{}:{}", source.len(), source, self.parameter_name)
    }
}

/// One raw administrator override; defaults and definitions remain in YAML.
#[derive(Clone, Debug)]
pub struct ServiceParametersRecord {
    /// Composite group/source/parameter identity.
    pub key: ServiceParameterKey,
    /// Raw string value before parameter actions or template resolution.
    pub value: String,
}

impl PersistentEntity for ServiceParametersRecord {
    type Key = ServiceParameterKey;

    fn key(&self) -> Self::Key {
        self.key.clone()
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("service_store_id"))
            .primary_key(Field::string("source_id"))
            .primary_key(Field::string("parameter_name"))
            .field(Field::string("value"))
    }
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()> {
        writer.string("service_store_id", &self.key.source.service_store_id)?;
        writer.string("source_id", &self.key.source.source_id)?;
        writer.string("parameter_name", &self.key.parameter_name)?;
        writer.string("value", &self.value)?;
        Ok(())
    }
    fn read_from(reader: &EntityReader<'_>) -> Result<Self> {
        Ok(Self {
            key: ServiceParameterKey {
                source: SourceServiceKey::new(
                    reader.string("service_store_id")?,
                    reader.string("source_id")?,
                ),
                parameter_name: reader.string("parameter_name")?.to_string(),
            },
            value: reader.string("value")?.to_string(),
        })
    }
}

/// Builds the primary-key scope of one source, including all its parameters.
///
/// # Arguments
/// * `source` - Group-qualified source identity.
pub fn parameter_scope(source: &SourceServiceKey) -> EntityQuery<ServiceParametersRecord> {
    EntityQuery::new()
        .where_string(&Field::string("service_store_id"), &source.service_store_id)
        .where_string(&Field::string("source_id"), &source.source_id)
}

/// Reads all overrides for one source as a runtime dictionary.
///
/// # Arguments
/// * `store` - Normalized parameter table.
/// * `source` - Group-qualified source identity.
///
/// # Errors
/// Returns a contextual read/decoding error from the backing store.
pub async fn load_parameter_values(
    store: &dyn TypedEntityStore<ServiceParametersRecord>,
    source: &SourceServiceKey,
) -> Result<HashMap<String, String>> {
    Ok(store
        .find(&parameter_scope(source))
        .await?
        .into_iter()
        .map(|record| (record.key.parameter_name, record.value))
        .collect())
}

/// Atomically replaces a source's overrides; an empty dictionary resets its options.
///
/// # Arguments
/// * `store` - Normalized parameter table with atomic replacement support.
/// * `source` - Group-qualified source identity.
/// * `values` - Complete raw overrides, already validated against current YAML definitions.
///
/// # Errors
/// Returns a backend error without a partial replacement of the source's rows.
pub async fn replace_parameter_values(
    store: &dyn TypedEntityStore<ServiceParametersRecord>,
    source: &SourceServiceKey,
    values: HashMap<String, String>,
) -> Result<()> {
    let records = values
        .into_iter()
        .map(|(parameter_name, value)| ServiceParametersRecord {
            key: ServiceParameterKey {
                source: source.clone(),
                parameter_name,
            },
            value,
        })
        .collect::<Vec<_>>();
    store
        .replace_where(&parameter_scope(source), &records)
        .await
}

/// Creates an isolated in-memory parameter table for compatibility constructors.
///
/// # Returns
/// A new empty table; cloned handles share its records, separate calls do not.
///
/// # Panics
/// Panics if the crate-owned table configuration or entity schema is invalid.
pub fn memory_parameters_store() -> Arc<dyn TypedEntityStore<ServiceParametersRecord>> {
    Arc::new(
        MemoryEntityStore::new(
            PersistenceStoreConfig::new(
                crate::SERVICE_PARAMETERS_TABLE_NAME,
                ServiceParametersRecord::schema(),
            )
            .expect("valid parameter store schema"),
        )
        .expect("valid parameter store"),
    )
}

#[cfg(feature = "sqlite-persistence")]
impl SqlKey for ServiceParameterKey {
    fn physical_types() -> &'static [&'static str] {
        &["TEXT", "TEXT", "TEXT"]
    }
    fn to_sql_values(&self) -> Vec<SqlValue> {
        vec![
            SqlValue::Text(self.source.service_store_id.clone()),
            SqlValue::Text(self.source.source_id.clone()),
            SqlValue::Text(self.parameter_name.clone()),
        ]
    }
    fn from_sql_value(_index: usize, cell: ValueRef<'_>) -> Result<SqlValue> {
        match cell {
            ValueRef::Text(value) => Ok(SqlValue::Text(std::str::from_utf8(value)?.to_string())),
            _ => anyhow::bail!("service parameter key component must be text"),
        }
    }
}

/// Opens the normalized parameter table without converting historical parameter data.
///
/// # Arguments
/// * `database` - Existing services database owning the parameter table.
///
/// # Errors
/// Returns an error if the table cannot be opened or its schema is incompatible.
/// Historical two-column keys and JSON parameter dictionaries are not migrated.
#[cfg(feature = "sqlite-persistence")]
pub fn sqlite_parameters_store(
    database: &SqliteDatabase,
) -> Result<Arc<dyn TypedEntityStore<ServiceParametersRecord>>> {
    Ok(Arc::new(database.table(PersistenceTableConfig::new(
        crate::SERVICE_PARAMETERS_TABLE_NAME,
        ServiceParametersRecord::schema(),
    )?)?))
}
