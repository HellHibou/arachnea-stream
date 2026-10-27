//! SQLite-backed typed persistence store with declared schema evolution.
//!
//! One physical store owns `<data-root>/<store-name>.sqlite3` and a shared
//! connection. Typed table views have independent entity schemas. Tables are
//! `STRICT`, booleans are checked integers, and column/index evolution is
//! reconciled at open time against internal metadata tables.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    marker::PhantomData,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use rusqlite::{
    types::{Value as SqlValue, ValueRef},
    Connection, Row,
};

use super::typed_store::{
    checked_schema, decode, encode, expired, safe_name, EntityDocument, EntityQuery, EntitySchema,
    FieldType, PersistenceStoreConfig, PersistenceTableConfig, PersistentEntity, StoredValue,
    TypedEntityStore,
};

/// Internal metadata table recording the logical type of every managed column.
use crate::SQLITE_COLUMNS_META_TABLE as COLUMNS_META_TABLE;
/// Internal metadata table recording the signature of every managed index.
use crate::SQLITE_INDEXES_META_TABLE as INDEXES_META_TABLE;
/// Bounds the connection-lock hold time of grouped primary-key reads.
use crate::SQLITE_READ_BATCH_KEYS;

/// SQLite physical representation of an entity primary key.
///
/// Composite domain keys implement this trait with components ordered like
/// their entity schema. Scalar key implementations are provided for `String`,
/// `i64`, and `Uuid`.
pub trait SqlKey: Send + Sync + 'static {
    /// Physical SQLite types declared for primary-key components.
    fn physical_types() -> &'static [&'static str];
    /// SQL values bound when addressing this key, in component order.
    fn to_sql_values(&self) -> Vec<SqlValue>;
    /// Normalizes one primary-key cell read from SQLite.
    fn from_sql_value(index: usize, cell: ValueRef<'_>) -> anyhow::Result<SqlValue>;
}
impl SqlKey for String {
    fn physical_types() -> &'static [&'static str] {
        &["TEXT"]
    }
    fn to_sql_values(&self) -> Vec<SqlValue> {
        vec![SqlValue::Text(self.clone())]
    }
    fn from_sql_value(_index: usize, cell: ValueRef<'_>) -> anyhow::Result<SqlValue> {
        match cell {
            ValueRef::Text(value) => Ok(SqlValue::Text(std::str::from_utf8(value)?.to_string())),
            _ => anyhow::bail!("string primary key cell has an unexpected SQLite type"),
        }
    }
}
impl SqlKey for i64 {
    fn physical_types() -> &'static [&'static str] {
        &["INTEGER"]
    }
    fn to_sql_values(&self) -> Vec<SqlValue> {
        vec![SqlValue::Integer(*self)]
    }
    fn from_sql_value(_index: usize, cell: ValueRef<'_>) -> anyhow::Result<SqlValue> {
        match cell {
            ValueRef::Integer(value) => Ok(SqlValue::Integer(value)),
            _ => anyhow::bail!("integer primary key cell has an unexpected SQLite type"),
        }
    }
}
impl SqlKey for uuid::Uuid {
    fn physical_types() -> &'static [&'static str] {
        // UUID primary keys are stored as 16-byte blobs (confirmed decision).
        &["BLOB"]
    }
    fn to_sql_values(&self) -> Vec<SqlValue> {
        vec![SqlValue::Blob(self.as_bytes().to_vec())]
    }
    fn from_sql_value(_index: usize, cell: ValueRef<'_>) -> anyhow::Result<SqlValue> {
        match cell {
            ValueRef::Blob(value) => Ok(SqlValue::Text(uuid::Uuid::from_slice(value)?.to_string())),
            _ => anyhow::bail!("uuid primary key cell has an unexpected SQLite type"),
        }
    }
}

/// Converts a normalized primary key SQL value back into its stored form.
fn stored_from_key_sql(field: &str, value: SqlValue) -> anyhow::Result<StoredValue> {
    Ok(match value {
        SqlValue::Text(value) => StoredValue::String(value),
        SqlValue::Integer(value) => StoredValue::Integer(value),
        _ => anyhow::bail!("primary key column '{field}' holds an unexpected SQLite type"),
    })
}

/// Physical SQLite store configuration, independent of its entity tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqliteStoreConfig {
    /// Stable name used for `<store-name>.sqlite3` and SQLite auxiliary files.
    pub name: String,
}

impl SqliteStoreConfig {
    /// Creates a physical store configuration.
    ///
    /// # Arguments
    /// * `name` - Non-empty physical store name, encoded safely for file placement.
    ///
    /// # Errors
    /// Returns an error if the name is empty.
    pub fn new(name: impl Into<String>) -> anyhow::Result<Self> {
        let name = name.into();
        anyhow::ensure!(!name.is_empty(), "SQLite store name must not be empty");
        Ok(Self { name })
    }
}

/// Shared physical database owner; table views keep its connection alive.
#[derive(Clone)]
pub struct SqliteDatabase {
    connection: Arc<Mutex<Connection>>,
}

impl SqliteDatabase {
    /// Opens `<data-root>/<store-name>.sqlite3` without reading legacy directories.
    ///
    /// # Arguments
    /// * `config` - Physical store identity, independent of any entity schema.
    /// * `data_root` - Directory containing the database and its WAL/SHM files.
    ///
    /// # Errors
    /// Returns an error when the name, directory, database or SQLite setup is invalid.
    pub fn open(config: SqliteStoreConfig, data_root: impl Into<PathBuf>) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !config.name.is_empty(),
            "SQLite store name must not be empty"
        );
        let root = data_root.into();
        std::fs::create_dir_all(&root).map_err(|error| {
            anyhow::anyhow!(
                "failed to create sqlite persistence directory {}: {error}",
                root.display()
            )
        })?;
        let path = root.join(format!("{}.sqlite3", safe_name(&config.name)));
        let connection = Connection::open(&path).map_err(|error| {
            anyhow::anyhow!(
                "failed to open sqlite persistence database {}: {error}",
                path.display()
            )
        })?;
        connection.execute_batch("PRAGMA synchronous = FULL; PRAGMA busy_timeout = 5000;")?;
        connection.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    /// Opens a typed table view on this database, reconciling only its schema.
    ///
    /// # Arguments
    /// * `config` - Table name and entity schema; never a database path.
    ///
    /// # Errors
    /// Returns an error for an invalid entity schema, reserved table name,
    /// incompatible existing table or failed schema transaction.
    pub fn table<E>(&self, config: PersistenceTableConfig) -> anyhow::Result<SqliteEntityStore<E>>
    where
        E: PersistentEntity,
        E::Key: SqlKey,
    {
        checked_schema::<E>(&config)?;
        anyhow::ensure!(
            !config.name.is_empty(),
            "SQLite table name must not be empty"
        );
        let table = safe_name(&config.name);
        anyhow::ensure!(
            !table.to_ascii_lowercase().starts_with("arachnea_")
                && !table.to_ascii_lowercase().starts_with("sqlite_"),
            "SQLite table name '{}' uses a reserved metadata prefix",
            config.name
        );
        anyhow::ensure!(
            E::Key::physical_types().len() == config.schema.primary_key_fields().len(),
            "table '{}' SQLite key representation does not match the declared primary key",
            config.name
        );
        let mut connection = self.connection.lock().map_err(|_| lock_poisoned())?;
        let transaction = connection.transaction()?;
        initialize_schema::<E>(&transaction, &config)?;
        transaction.commit()?;
        Ok(SqliteEntityStore {
            table,
            config,
            database: self.clone(),
            marker: PhantomData,
        })
    }
}

/// Typed table view sharing the physical database owner's connection.
pub struct SqliteEntityStore<E: PersistentEntity> {
    config: PersistenceStoreConfig,
    table: String,
    database: SqliteDatabase,
    marker: PhantomData<E>,
}

impl<E: PersistentEntity> SqliteEntityStore<E>
where
    E::Key: SqlKey,
{
    /// Opens a standalone database and a table with the same name under
    /// `<data-root>/<store-name>.sqlite3`. Use [`SqliteDatabase::table`] to share a store.
    ///
    /// # Arguments
    /// * `config` - Entity table configuration, also naming the standalone database.
    /// * `data_root` - Directory containing the database and its auxiliary files.
    ///
    /// # Errors
    ///
    /// Returns a contextualized error when the directory or database cannot be
    /// opened, or when the existing physical schema is incompatible.
    pub fn new(
        config: PersistenceStoreConfig,
        data_root: impl Into<PathBuf>,
    ) -> anyhow::Result<Self> {
        SqliteDatabase::open(SqliteStoreConfig::new(&config.name)?, data_root)?.table(config)
    }
}

fn lock_poisoned() -> anyhow::Error {
    anyhow::anyhow!("sqlite entity store connection lock was poisoned")
}
fn join_error(error: tokio::task::JoinError) -> anyhow::Error {
    anyhow::anyhow!("sqlite entity store blocking task failed: {error}")
}
fn now_nanos() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX_EPOCH")
        .as_nanos() as i64
}
fn sql_type(field_type: FieldType) -> &'static str {
    match field_type {
        FieldType::String | FieldType::Json => "TEXT",
        FieldType::Integer | FieldType::Boolean | FieldType::DateTime => "INTEGER",
    }
}
fn logical_type_name(field_type: FieldType) -> &'static str {
    match field_type {
        FieldType::String => "string",
        FieldType::Integer => "integer",
        FieldType::Boolean => "boolean",
        FieldType::DateTime => "date_time",
        FieldType::Json => "json",
    }
}
fn index_name(table: &str, column: &str) -> String {
    // The length delimits table names containing underscores; the reserved prefix
    // also avoids collisions with legacy index names during reconciliation.
    format!("arachnea_idx_{}_{table}_{}", table.len(), safe_name(column))
}
fn column_list(schema: &EntitySchema) -> String {
    schema
        .fields()
        .iter()
        .map(|field| format!("\"{}\"", field.name()))
        .collect::<Vec<_>>()
        .join(", ")
}
fn primary_key_clause(schema: &EntitySchema) -> String {
    schema
        .primary_key_fields()
        .iter()
        .map(|field| format!("\"{}\"", field.name()))
        .collect::<Vec<_>>()
        .join(", ")
}
fn primary_key_predicate(schema: &EntitySchema) -> String {
    schema
        .primary_key_fields()
        .iter()
        .enumerate()
        .map(|(index, field)| format!("\"{}\" = ?{}", field.name(), index + 1))
        .collect::<Vec<_>>()
        .join(" AND ")
}
fn to_sql_value(value: &StoredValue) -> anyhow::Result<SqlValue> {
    Ok(match value {
        StoredValue::String(value) => SqlValue::Text(value.clone()),
        StoredValue::Integer(value) => SqlValue::Integer(*value),
        StoredValue::Boolean(value) => SqlValue::Integer(i64::from(*value)),
        StoredValue::DateTime(value) => SqlValue::Integer(
            i64::try_from(*value)
                .map_err(|_| anyhow::anyhow!("date-time value is out of SQLite integer range"))?,
        ),
        StoredValue::Json(value) => SqlValue::Text(serde_json::to_string(value)?),
    })
}
fn stored_from_cell(
    field: &str,
    field_type: FieldType,
    cell: ValueRef<'_>,
) -> anyhow::Result<Option<StoredValue>> {
    let stored = match cell {
        ValueRef::Null => return Ok(None),
        ValueRef::Text(value) => match field_type {
            FieldType::String => StoredValue::String(std::str::from_utf8(value)?.to_string()),
            FieldType::Json => StoredValue::Json(serde_json::from_slice(value)?),
            _ => anyhow::bail!("sqlite column '{field}' holds text but expects {field_type:?}"),
        },
        ValueRef::Integer(value) => match field_type {
            FieldType::Integer => StoredValue::Integer(value),
            FieldType::Boolean => match value {
                0 => StoredValue::Boolean(false),
                1 => StoredValue::Boolean(true),
                _ => anyhow::bail!("sqlite boolean column '{field}' holds {value}"),
            },
            FieldType::DateTime => StoredValue::DateTime(i128::from(value)),
            _ => {
                anyhow::bail!("sqlite column '{field}' holds an integer but expects {field_type:?}")
            }
        },
        ValueRef::Real(value) => {
            anyhow::bail!("sqlite column '{field}' holds an unexpected real value ({value})")
        }
        ValueRef::Blob(_) => {
            anyhow::bail!("sqlite column '{field}' holds an unexpected blob value")
        }
    };
    Ok(Some(stored))
}
fn read_row<E: PersistentEntity>(
    schema: &EntitySchema,
    row: &Row<'_>,
) -> anyhow::Result<EntityDocument>
where
    E::Key: SqlKey,
{
    let primary_key_fields = schema.primary_key_fields();
    let mut fields = BTreeMap::new();
    for (index, field) in schema.fields().iter().enumerate() {
        let cell = row.get_ref(index)?;
        let stored = if let Some(key_index) = primary_key_fields
            .iter()
            .position(|key| key.name() == field.name())
        {
            Some(stored_from_key_sql(
                field.name(),
                <E::Key as SqlKey>::from_sql_value(key_index, cell)?,
            )?)
        } else {
            stored_from_cell(field.name(), field.field_type(), cell)?
        };
        if let Some(stored) = stored {
            fields.insert(field.name().to_string(), stored);
        }
    }
    Ok(EntityDocument { fields })
}
fn insert_entity(
    connection: &Connection,
    table: &str,
    schema: &EntitySchema,
    document: &EntityDocument,
) -> anyhow::Result<()> {
    let names: Vec<&str> = schema.fields().iter().map(|field| field.name()).collect();
    let placeholders = (1..=names.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let columns = names
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let values = names
        .iter()
        .map(|name| match document.fields.get(*name) {
            Some(value) => to_sql_value(value),
            // Nullable absent fields are stored as SQL NULL.
            None => Ok(SqlValue::Null),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    connection.execute(
        &format!("INSERT OR REPLACE INTO \"{table}\" ({columns}) VALUES ({placeholders})"),
        rusqlite::params_from_iter(values),
    )?;
    Ok(())
}

fn initialize_schema<E: PersistentEntity>(
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
fn physical_indexes(connection: &Connection, table: &str) -> anyhow::Result<HashSet<String>> {
    let mut statement = connection
        .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = ?1")?;
    let rows = statement.query_map([table], |row| row.get::<_, String>(0))?;
    rows.collect::<rusqlite::Result<HashSet<_>>>()
        .map_err(|error| anyhow::anyhow!("failed to list sqlite indexes: {error}"))
}
fn declared_indexes(schema: &EntitySchema, table: &str) -> Vec<(String, String)> {
    schema
        .fields()
        .iter()
        .filter(|field| field.is_indexed())
        .map(|field| (index_name(table, field.name()), field.name().to_string()))
        .collect()
}
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

#[async_trait]
impl<E> TypedEntityStore<E> for SqliteEntityStore<E>
where
    E: PersistentEntity,
    E::Key: SqlKey,
{
    async fn get(&self, key: &E::Key) -> anyhow::Result<Option<E>> {
        let connection = self.database.connection.clone();
        let table = self.table.clone();
        let config = self.config.clone();
        let key_values = key.to_sql_values();
        let task = tokio::task::spawn_blocking(move || {
            let connection = connection.lock().map_err(|_| lock_poisoned())?;
            let schema = &config.schema;
            let key_predicate = primary_key_predicate(schema);
            let mut statement = connection.prepare(&format!(
                "SELECT {} FROM \"{table}\" WHERE {key_predicate}",
                column_list(schema),
            ))?;
            let mut rows = statement.query(rusqlite::params_from_iter(key_values.clone()))?;
            let Some(row) = rows.next()? else {
                return Ok(None);
            };
            let document = read_row::<E>(schema, row)?;
            drop(rows);
            drop(statement);
            if expired(schema, &document)? {
                connection.execute(
                    &format!("DELETE FROM \"{table}\" WHERE {key_predicate}",),
                    rusqlite::params_from_iter(key_values),
                )?;
                return Ok(None);
            }
            Ok(Some(decode::<E>(schema, &document)?))
        });
        task.await.map_err(join_error)?
    }
    async fn get_many(&self, keys: &[E::Key]) -> anyhow::Result<Vec<Option<E>>> {
        let width = self.config.schema.primary_key_fields().len();
        if keys.len() <= 1 || width > crate::SQLITE_READ_MAX_PARAMETERS {
            let mut results = Vec::with_capacity(keys.len());
            for key in keys {
                results.push(self.get(key).await?);
            }
            return Ok(results);
        }
        let mut results = Vec::with_capacity(keys.len());
        for chunk in
            keys.chunks(SQLITE_READ_BATCH_KEYS.min(crate::SQLITE_READ_MAX_PARAMETERS / width))
        {
            let connection = self.database.connection.clone();
            let table = self.table.clone();
            let config = self.config.clone();
            let keys = chunk.to_vec();
            let task = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<Option<E>>> {
                let connection = connection.lock().map_err(|_| lock_poisoned())?;
                let schema = &config.schema;
                let mut values = Vec::with_capacity(keys.len() * width);
                let mut predicates = Vec::with_capacity(keys.len());
                for key in &keys {
                    let fields = schema
                        .primary_key_fields()
                        .iter()
                        .enumerate()
                        .map(|(index, field)| {
                            format!("\"{}\" = ?{}", field.name(), values.len() + index + 1)
                        })
                        .collect::<Vec<_>>()
                        .join(" AND ");
                    let key_values = key.to_sql_values();
                    anyhow::ensure!(
                        key_values.len() == width,
                        "sqlite table '{table}' primary key width mismatch"
                    );
                    values.extend(key_values);
                    predicates.push(format!("({fields})"));
                }
                let mut statement = connection.prepare(&format!(
                    "SELECT {} FROM \"{table}\" WHERE {}",
                    column_list(schema),
                    predicates.join(" OR ")
                ))?;
                let mut rows = statement.query(rusqlite::params_from_iter(values))?;
                let mut found = HashMap::new();
                let mut expired_keys = Vec::new();
                while let Some(row) = rows.next()? {
                    let document = read_row::<E>(schema, row)?;
                    if expired(schema, &document)? {
                        let values = schema
                            .primary_key_fields()
                            .iter()
                            .map(|field| row.get::<_, SqlValue>(field.name()))
                            .collect::<rusqlite::Result<Vec<_>>>()?;
                        expired_keys.push(values);
                    } else {
                        let entity = decode::<E>(schema, &document)?;
                        found.insert(entity.key(), entity);
                    }
                }
                drop(rows);
                drop(statement);
                for key_values in expired_keys {
                    connection.execute(
                        &format!(
                            "DELETE FROM \"{table}\" WHERE {}",
                            primary_key_predicate(schema)
                        ),
                        rusqlite::params_from_iter(key_values),
                    )?;
                }
                Ok(keys.iter().map(|key| found.get(key).cloned()).collect())
            });
            results.extend(task.await.map_err(join_error)??);
            tokio::task::yield_now().await;
        }
        Ok(results)
    }
    async fn put(&self, entity: &E) -> anyhow::Result<()> {
        let connection = self.database.connection.clone();
        let table = self.table.clone();
        let config = self.config.clone();
        let entity = entity.clone();
        let task = tokio::task::spawn_blocking(move || {
            let document = encode(&config, &entity)?;
            let connection = connection.lock().map_err(|_| lock_poisoned())?;
            insert_entity(&connection, &table, &config.schema, &document)
        });
        task.await.map_err(join_error)?
    }
    async fn put_all(&self, entities: &[E]) -> anyhow::Result<()> {
        let connection = self.database.connection.clone();
        let table = self.table.clone();
        let config = self.config.clone();
        let entities = entities.to_vec();
        let task = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            // Validate every entity before opening the transaction.
            let documents = entities
                .iter()
                .map(|entity| encode(&config, entity))
                .collect::<anyhow::Result<Vec<_>>>()?;
            let mut connection = connection.lock().map_err(|_| lock_poisoned())?;
            let transaction = connection.transaction()?;
            for document in &documents {
                insert_entity(&transaction, &table, &config.schema, document)?;
            }
            transaction
                .commit()
                .map_err(|error| anyhow::anyhow!("failed to commit sqlite batch: {error}"))
        });
        task.await.map_err(join_error)?
    }
    async fn delete(&self, key: &E::Key) -> anyhow::Result<()> {
        let connection = self.database.connection.clone();
        let table = self.table.clone();
        let config = self.config.clone();
        let key_values = key.to_sql_values();
        let task = tokio::task::spawn_blocking(move || {
            let connection = connection.lock().map_err(|_| lock_poisoned())?;
            connection.execute(
                &format!(
                    "DELETE FROM \"{table}\" WHERE {}",
                    primary_key_predicate(&config.schema)
                ),
                rusqlite::params_from_iter(key_values),
            )?;
            Ok(())
        });
        task.await.map_err(join_error)?
    }
    async fn find(&self, query: &EntityQuery<E>) -> anyhow::Result<Vec<E>> {
        let connection = self.database.connection.clone();
        let table = self.table.clone();
        let config = self.config.clone();
        let predicates = query.predicates().to_vec();
        let task = tokio::task::spawn_blocking(move || {
            let connection = connection.lock().map_err(|_| lock_poisoned())?;
            let schema = &config.schema;
            // Purge expired entries so indexed TTL queries stay bounded.
            if let Some(expiration) = schema.expiration_field() {
                connection.execute(
                    &format!(
                        "DELETE FROM \"{table}\" WHERE \"{}\" IS NOT NULL AND \"{}\" <= ?1",
                        expiration.name(),
                        expiration.name()
                    ),
                    rusqlite::params![now_nanos()],
                )?;
            }
            let mut clauses = Vec::new();
            let mut values = Vec::new();
            for (name, value) in &predicates {
                let field = schema
                    .field_named(name)
                    .ok_or_else(|| anyhow::anyhow!("query field '{name}' is not declared"))?;
                if field.field_type() != value.field_type() {
                    anyhow::bail!("query value for '{name}' has the wrong type");
                }
                values.push(to_sql_value(value)?);
                clauses.push(format!("\"{name}\" = ?{}", values.len()));
            }
            if let Some(expiration) = schema.expiration_field() {
                clauses.push(format!(
                    "(\"{}\" IS NULL OR \"{}\" > ?{})",
                    expiration.name(),
                    expiration.name(),
                    values.len() + 1
                ));
                values.push(SqlValue::Integer(now_nanos()));
            }
            let where_clause = if clauses.is_empty() {
                String::new()
            } else {
                format!(" WHERE {}", clauses.join(" AND "))
            };
            let mut statement = connection.prepare(&format!(
                "SELECT {} FROM \"{table}\"{where_clause}",
                column_list(schema)
            ))?;
            let mut rows = statement.query(rusqlite::params_from_iter(values))?;
            let mut result = Vec::new();
            while let Some(row) = rows.next()? {
                result.push(decode::<E>(schema, &read_row::<E>(schema, row)?)?);
            }
            Ok(result)
        });
        task.await.map_err(join_error)?
    }
}

#[cfg(test)]
#[path = "../../tests/persistence/sqlite_store.rs"]
mod tests;
