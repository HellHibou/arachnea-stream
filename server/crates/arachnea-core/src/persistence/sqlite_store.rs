//! SQLite-backed typed persistence store with declared schema evolution.
//!
//! One physical store owns `<data-root>/<store-name>.sqlite3` and a shared
//! connection. Typed table views have independent entity schemas. Tables are
//! `STRICT`, booleans are checked integers, and column/index evolution is
//! reconciled at open time against internal metadata tables.

mod conversion;
mod schema;

use conversion::{read_row, to_sql_value};
use schema::initialize_schema;

use std::{
    collections::HashMap,
    marker::PhantomData,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use rusqlite::{
    types::{Value as SqlValue, ValueRef},
    Connection,
};

use super::typed_store::{
    checked_schema, decode, encode, expired, safe_name, EntityDocument, EntityQuery, EntitySchema,
    PersistenceStoreConfig, PersistenceTableConfig, PersistentEntity, TypedEntityStore,
};

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
    ///
    /// # Arguments
    /// * `index` - Zero-based component index in the key.
    /// * `cell` - Raw SQLite value for that component.
    ///
    /// # Errors
    /// Returns an error when the cell type does not match the component type.
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
/// Returns the current UNIX time in nanoseconds as a SQLite integer.
fn now_nanos() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX_EPOCH")
        .as_nanos() as i64
}
/// Builds the quoted column list for a schema.
///
/// # Arguments
/// * `schema` - Entity schema whose fields are listed.
fn column_list(schema: &EntitySchema) -> String {
    schema
        .fields()
        .iter()
        .map(|field| format!("\"{}\"", field.name()))
        .collect::<Vec<_>>()
        .join(", ")
}
/// Builds the numbered primary-key predicate for a schema.
///
/// # Arguments
/// * `schema` - Entity schema whose primary-key fields are compared.
fn primary_key_predicate(schema: &EntitySchema) -> String {
    schema
        .primary_key_fields()
        .iter()
        .enumerate()
        .map(|(index, field)| format!("\"{}\" = ?{}", field.name(), index + 1))
        .collect::<Vec<_>>()
        .join(" AND ")
}
/// Inserts or replaces one entity document in the SQLite table.
///
/// # Arguments
/// * `connection` - Open SQLite connection.
/// * `table` - Physical table name.
/// * `schema` - Entity schema defining columns.
/// * `document` - Entity document to store.
///
/// # Errors
/// Returns an error when a document value cannot be encoded or SQLite execution fails.
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
