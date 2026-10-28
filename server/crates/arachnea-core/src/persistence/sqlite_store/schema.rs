//! Transactional reconciliation of SQLite tables, columns and managed indexes.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use super::{safe_name, EntitySchema, PersistenceStoreConfig, PersistentEntity, SqlKey};
use crate::persistence::typed_store::FieldType;
use crate::{
    SQLITE_COLUMNS_META_TABLE as COLUMNS_META_TABLE,
    SQLITE_INDEXES_META_TABLE as INDEXES_META_TABLE,
};

/// Returns the SQLite storage type used for a logical field type.
///
/// # Arguments
/// * `field_type` - Logical entity field type.
fn sql_type(field_type: FieldType) -> &'static str {
    match field_type {
        FieldType::String | FieldType::Json => "TEXT",
        FieldType::Integer | FieldType::Boolean | FieldType::DateTime => "INTEGER",
    }
}
/// Returns the metadata name recorded for a logical field type.
///
/// # Arguments
/// * `field_type` - Logical entity field type.
fn logical_type_name(field_type: FieldType) -> &'static str {
    match field_type {
        FieldType::String => "string",
        FieldType::Integer => "integer",
        FieldType::Boolean => "boolean",
        FieldType::DateTime => "date_time",
        FieldType::Json => "json",
    }
}
/// Builds a reserved index name with a length-delimited table component.
///
/// # Arguments
/// * `table` - Physical table name, whose length prevents underscore collisions.
/// * `column` - Column name encoded safely in the managed index name.
pub(super) fn index_name(table: &str, column: &str) -> String {
    format!("arachnea_idx_{}_{table}_{}", table.len(), safe_name(column))
}
/// Builds the primary-key clause for a schema.
///
/// # Arguments
/// * `schema` - Entity schema whose primary-key fields are listed.
fn primary_key_clause(schema: &EntitySchema) -> String {
    schema
        .primary_key_fields()
        .iter()
        .map(|field| format!("\"{}\"", field.name()))
        .collect::<Vec<_>>()
        .join(", ")
}
/// Creates or evolves the entity table and its metadata.
///
/// # Arguments
/// * `connection` - Open SQLite connection.
/// * `config` - Store name and schema used to build the table.
///
/// # Errors
/// Returns an error when schema inspection, table creation, or evolution fails.
pub(super) fn initialize_schema<E: PersistentEntity>(
    connection: &Connection,
    config: &PersistenceStoreConfig,
) -> anyhow::Result<()>
where
    E::Key: SqlKey,
{
    connection.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {COLUMNS_META_TABLE} \
             (table_name TEXT NOT NULL, name TEXT NOT NULL, logical_type TEXT NOT NULL, \
              PRIMARY KEY (table_name, name)) STRICT;\
         CREATE TABLE IF NOT EXISTS {INDEXES_META_TABLE} \
             (table_name TEXT NOT NULL, name TEXT NOT NULL, column_name TEXT NOT NULL, \
              PRIMARY KEY (table_name, name)) STRICT;"
    ))?;
    let table = safe_name(&config.name);
    let exists: bool = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [&table],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| anyhow::anyhow!("failed to inspect sqlite schema: {error}"))?
        > 0;
    if exists {
        evolve_table::<E>(connection, &table, config)
    } else {
        create_table::<E>(connection, &table, config)
    }
}
/// Records a managed column in the schema metadata table.
///
/// # Arguments
/// * `connection` - Open SQLite connection.
/// * `table` - Physical table name.
/// * `name` - Column name.
/// * `field_type` - Logical type recorded for the column.
///
/// # Errors
/// Returns an error when the metadata write fails.
fn record_column(
    connection: &Connection,
    table: &str,
    name: &str,
    field_type: FieldType,
) -> anyhow::Result<()> {
    connection.execute(
        &format!(
            "INSERT OR REPLACE INTO {COLUMNS_META_TABLE} (table_name, name, logical_type) VALUES (?1, ?2, ?3)"
        ),
        rusqlite::params![table, name, logical_type_name(field_type)],
    )?;
    Ok(())
}
/// Records a managed index in the schema metadata table.
///
/// # Arguments
/// * `connection` - Open SQLite connection.
/// * `table` - Physical table name.
/// * `name` - Index name.
/// * `column` - Indexed column name.
///
/// # Errors
/// Returns an error when the metadata write fails.
fn record_index(
    connection: &Connection,
    table: &str,
    name: &str,
    column: &str,
) -> anyhow::Result<()> {
    connection.execute(
        &format!("INSERT OR REPLACE INTO {INDEXES_META_TABLE} (table_name, name, column_name) VALUES (?1, ?2, ?3)"),
        rusqlite::params![table, name, column],
    )?;
    Ok(())
}
/// Reads the managed column names and logical types for a table.
///
/// # Arguments
/// * `connection` - Open SQLite connection.
/// * `table` - Physical table name.
///
/// # Errors
/// Returns an error when metadata cannot be queried or collected.
fn managed_columns(
    connection: &Connection,
    table: &str,
) -> anyhow::Result<HashMap<String, String>> {
    let mut statement = connection.prepare(&format!(
        "SELECT name, logical_type FROM {COLUMNS_META_TABLE} WHERE table_name = ?1"
    ))?;
    let rows = statement.query_map([table], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.collect::<rusqlite::Result<HashMap<_, _>>>()
        .map_err(|error| anyhow::anyhow!("failed to read sqlite column metadata: {error}"))
}
/// Reads the managed index names and columns for a table.
///
/// # Arguments
/// * `connection` - Open SQLite connection.
/// * `table` - Physical table name.
///
/// # Errors
/// Returns an error when metadata cannot be queried or collected.
fn managed_indexes(
    connection: &Connection,
    table: &str,
) -> anyhow::Result<HashMap<String, String>> {
    let mut statement = connection.prepare(&format!(
        "SELECT name, column_name FROM {INDEXES_META_TABLE} WHERE table_name = ?1"
    ))?;
    let rows = statement.query_map([table], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.collect::<rusqlite::Result<HashMap<_, _>>>()
        .map_err(|error| anyhow::anyhow!("failed to read sqlite index metadata: {error}"))
}
/// Lists the physical index names attached to a table.
///
/// # Arguments
/// * `connection` - Open SQLite connection.
/// * `table` - Physical table name.
///
/// # Errors
/// Returns an error when the SQLite catalog cannot be queried.
fn physical_indexes(connection: &Connection, table: &str) -> anyhow::Result<HashSet<String>> {
    let mut statement = connection
        .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = ?1")?;
    let rows = statement.query_map([table], |row| row.get::<_, String>(0))?;
    rows.collect::<rusqlite::Result<HashSet<_>>>()
        .map_err(|error| anyhow::anyhow!("failed to list sqlite indexes: {error}"))
}
/// Returns the index names and columns declared by a schema.
///
/// # Arguments
/// * `schema` - Entity schema declaring indexed fields.
/// * `table` - Physical table name used in generated index names.
fn declared_indexes(schema: &EntitySchema, table: &str) -> Vec<(String, String)> {
    schema
        .fields()
        .iter()
        .filter(|field| field.is_indexed())
        .map(|field| (index_name(table, field.name()), field.name().to_string()))
        .collect()
}
/// Creates an entity table, its metadata rows, and declared indexes.
///
/// # Arguments
/// * `connection` - Open SQLite connection.
/// * `table` - Physical table name.
/// * `config` - Store name and schema used for the table definition.
///
/// # Errors
/// Returns an error when the table or index DDL or metadata writes fail.
fn create_table<E: PersistentEntity>(
    connection: &Connection,
    table: &str,
    config: &PersistenceStoreConfig,
) -> anyhow::Result<()>
where
    E::Key: SqlKey,
{
    let schema = &config.schema;
    let primary_key_fields = schema.primary_key_fields();
    let physical_types = <E::Key as SqlKey>::physical_types();
    let mut definitions = Vec::new();
    for field in schema.fields() {
        let definition = if let Some(index) = primary_key_fields
            .iter()
            .position(|key| key.name() == field.name())
        {
            format!("\"{}\" {} NOT NULL", field.name(), physical_types[index])
        } else {
            let mut definition = format!("\"{}\" {}", field.name(), sql_type(field.field_type()));
            if field.field_type() == FieldType::Boolean {
                definition.push_str(&format!(" CHECK (\"{}\" IN (0, 1))", field.name()));
            }
            if !field.is_nullable() {
                definition.push_str(" NOT NULL");
            }
            definition
        };
        definitions.push(definition);
    }
    definitions.push(format!("PRIMARY KEY ({})", primary_key_clause(schema)));
    connection.execute(
        &format!(
            "CREATE TABLE \"{table}\" ({}) STRICT",
            definitions.join(", ")
        ),
        [],
    )?;
    for field in schema.fields() {
        record_column(connection, table, field.name(), field.field_type())?;
    }
    for (name, column) in declared_indexes(schema, table) {
        connection.execute(
            &format!("CREATE INDEX \"{name}\" ON \"{table}\" (\"{column}\")"),
            [],
        )?;
        record_index(connection, table, &name, &column)?;
    }
    Ok(())
}
/// Evolves an existing entity table to the current schema.
///
/// Added columns are nullable, while primary-key changes and unmanaged type changes are rejected.
///
/// # Arguments
/// * `connection` - Open SQLite connection.
/// * `table` - Physical table name.
/// * `config` - Store name and target schema.
///
/// # Errors
/// Returns an error when the primary key or an existing column is incompatible, or when DDL or metadata writes fail.
fn evolve_table<E: PersistentEntity>(
    connection: &Connection,
    table: &str,
    config: &PersistenceStoreConfig,
) -> anyhow::Result<()>
where
    E::Key: SqlKey,
{
    let schema = &config.schema;
    let primary_key_fields = schema.primary_key_fields();
    let physical_types = <E::Key as SqlKey>::physical_types();
    let managed = managed_columns(connection, table)?;
    let mut statement = connection.prepare(&format!("PRAGMA table_info(\"{table}\")"))?;
    let existing = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| anyhow::anyhow!("failed to read sqlite table info: {error}"))?;
    drop(statement);
    let mut primary_key_columns: Vec<&(String, String, i64)> =
        existing.iter().filter(|(_, _, pk)| *pk > 0).collect();
    primary_key_columns.sort_by_key(|(_, _, position)| *position);
    if primary_key_columns.len() != primary_key_fields.len() {
        anyhow::bail!(
            "sqlite table '{table}' has {} primary key columns but the entity declares {}; primary keys are never migrated automatically",
            primary_key_columns.len(),
            primary_key_fields.len()
        );
    }
    for (index, ((name, physical_type, position), declared)) in primary_key_columns
        .iter()
        .zip(primary_key_fields.iter())
        .enumerate()
    {
        if *position != i64::try_from(index + 1).expect("primary key position fits i64")
            || name != declared.name()
            || !physical_type.eq_ignore_ascii_case(physical_types[index])
        {
            anyhow::bail!(
                "sqlite table '{table}' primary key does not match declared component {} ('{}' {}); primary keys are never migrated automatically",
                index + 1,
                declared.name(),
                physical_types[index]
            );
        }
    }
    let existing_names: HashSet<&str> = existing.iter().map(|(name, _, _)| name.as_str()).collect();
    for field in schema.fields() {
        if existing_names.contains(field.name()) {
            let recorded = managed.get(field.name()).ok_or_else(|| {
                anyhow::anyhow!(
                    "sqlite column '{}' of store '{}' is not managed by arachnea metadata",
                    field.name(),
                    config.name
                )
            })?;
            let expected = logical_type_name(field.field_type());
            if recorded != expected {
                anyhow::bail!(
                    "sqlite column '{}' of store '{}' has logical type '{}' but the entity declares '{}'",
                    field.name(),
                    config.name,
                    recorded,
                    expected
                );
            }
        } else {
            // Added columns are always nullable (confirmed decision).
            connection.execute(
                &format!(
                    "ALTER TABLE \"{table}\" ADD COLUMN \"{}\" {}",
                    field.name(),
                    sql_type(field.field_type())
                ),
                [],
            )?;
            record_column(connection, table, field.name(), field.field_type())?;
        }
    }
    reconcile_indexes(connection, table, schema)
}
/// Reconciles declared indexes with managed and physical SQLite indexes.
///
/// Managed indexes no longer declared by the schema are dropped. External indexes
/// are preserved unless their names collide with declared managed indexes.
///
/// # Arguments
/// * `connection` - Open SQLite connection.
/// * `table` - Physical table name.
/// * `schema` - Entity schema declaring the target indexes.
///
/// # Errors
/// Returns an error when an unmanaged index collides with a declared name or index DDL/metadata writes fail.
fn reconcile_indexes(
    connection: &Connection,
    table: &str,
    schema: &EntitySchema,
) -> anyhow::Result<()> {
    let declared = declared_indexes(schema, table);
    let managed = managed_indexes(connection, table)?;
    let physical = physical_indexes(connection, table)?;
    for (name, column) in &declared {
        if managed.contains_key(name) {
            continue;
        }
        if physical.contains(name) {
            anyhow::bail!(
                "sqlite index '{name}' already exists but is not managed by arachnea metadata"
            );
        }
        connection.execute(
            &format!("CREATE INDEX \"{name}\" ON \"{table}\" (\"{column}\")"),
            [],
        )?;
        record_index(connection, table, name, column)?;
    }
    for name in managed.keys() {
        if !declared.iter().any(|(declared, _)| declared == name) {
            connection.execute(&format!("DROP INDEX IF EXISTS \"{name}\""), [])?;
            connection.execute(
                &format!("DELETE FROM {INDEXES_META_TABLE} WHERE table_name = ?1 AND name = ?2"),
                [table, name],
            )?;
        }
    }
    Ok(())
}
