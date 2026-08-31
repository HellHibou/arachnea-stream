//! SQLite-backed typed persistence store with declared schema evolution.
//!
//! One store owns one SQLite database placed in its own directory
//! (`<data-root>/<store-name>/records.sqlite3`) so WAL auxiliary files stay
//! contained. The physical schema is derived from the entity schema: tables are
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
    FieldType, PersistenceStoreConfig, PersistentEntity, StoredValue, TypedEntityStore,
};

/// Internal metadata table recording the logical type of every managed column.
const COLUMNS_META_TABLE: &str = "arachnea_columns";
/// Internal metadata table recording the signature of every managed index.
const INDEXES_META_TABLE: &str = "arachnea_indexes";

/// SQLite physical representation of an entity primary key.
pub trait SqlKey: Send + Sync + 'static {
    /// Physical SQLite column type declared for the primary key column.
    fn physical_type() -> &'static str;
    /// SQL value bound when addressing this key.
    fn to_sql(&self) -> SqlValue;
    /// Rebuilds the stored logical value from the primary key cell.
    fn from_sql(cell: ValueRef<'_>) -> anyhow::Result<StoredValue>;
}
impl SqlKey for String {
    fn physical_type() -> &'static str {
        "TEXT"
    }
    fn to_sql(&self) -> SqlValue {
        SqlValue::Text(self.clone())
    }
    fn from_sql(cell: ValueRef<'_>) -> anyhow::Result<StoredValue> {
        match cell {
            ValueRef::Text(value) => {
                Ok(StoredValue::String(std::str::from_utf8(value)?.to_string()))
            }
            _ => anyhow::bail!("string primary key cell has an unexpected SQLite type"),
        }
    }
}
impl SqlKey for i64 {
    fn physical_type() -> &'static str {
        "INTEGER"
    }
    fn to_sql(&self) -> SqlValue {
        SqlValue::Integer(*self)
    }
    fn from_sql(cell: ValueRef<'_>) -> anyhow::Result<StoredValue> {
        match cell {
            ValueRef::Integer(value) => Ok(StoredValue::Integer(value)),
            _ => anyhow::bail!("integer primary key cell has an unexpected SQLite type"),
        }
    }
}
impl SqlKey for uuid::Uuid {
    fn physical_type() -> &'static str {
        // UUID primary keys are stored as 16-byte blobs (confirmed decision).
        "BLOB"
    }
    fn to_sql(&self) -> SqlValue {
        SqlValue::Blob(self.as_bytes().to_vec())
    }
    fn from_sql(cell: ValueRef<'_>) -> anyhow::Result<StoredValue> {
        match cell {
            ValueRef::Blob(value) => Ok(StoredValue::String(
                uuid::Uuid::from_slice(value)?.to_string(),
            )),
            _ => anyhow::bail!("uuid primary key cell has an unexpected SQLite type"),
        }
    }
}

/// SQLite-backed store bound to exactly one entity and one schema.
pub struct SqliteEntityStore<E: PersistentEntity> {
    config: PersistenceStoreConfig,
    table: String,
    connection: Arc<Mutex<Connection>>,
    marker: PhantomData<E>,
}

impl<E: PersistentEntity> SqliteEntityStore<E>
where
    E::Key: SqlKey,
{
    /// Opens (and reconciles) the store database under
    /// `<data-root>/<store-name>/records.sqlite3`.
    ///
    /// # Errors
    ///
    /// Returns a contextualized error when the directory or database cannot be
    /// opened, or when the existing physical schema is incompatible.
    pub fn new(
        config: PersistenceStoreConfig,
        data_root: impl Into<PathBuf>,
    ) -> anyhow::Result<Self> {
        checked_schema::<E>(&config)?;
        let directory = data_root.into().join(safe_name(&config.name));
        std::fs::create_dir_all(&directory).map_err(|error| {
            anyhow::anyhow!(
                "failed to create sqlite persistence directory {}: {error}",
                directory.display()
            )
        })?;
        let connection = Connection::open(directory.join("records.sqlite3")).map_err(|error| {
            anyhow::anyhow!(
                "failed to open sqlite persistence database for store '{}': {error}",
                config.name
            )
        })?;
        connection.execute_batch("PRAGMA synchronous = FULL; PRAGMA busy_timeout = 5000;")?;
        connection.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        initialize_schema::<E>(&connection, &config)?;
        Ok(Self {
            table: safe_name(&config.name),
            config,
            connection: Arc::new(Mutex::new(connection)),
            marker: PhantomData,
        })
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
    format!("idx_{table}_{column}")
}
fn column_list(schema: &EntitySchema) -> String {
    schema
        .fields()
        .iter()
        .map(|field| format!("\"{}\"", field.name()))
        .collect::<Vec<_>>()
        .join(", ")
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
    let primary_key = schema.primary_key_field().expect("validated schema");
    let mut fields = BTreeMap::new();
    for (index, field) in schema.fields().iter().enumerate() {
        let cell = row.get_ref(index)?;
        let stored = if field.name() == primary_key.name() {
            Some(<E::Key as SqlKey>::from_sql(cell)?)
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
             (name TEXT PRIMARY KEY, logical_type TEXT NOT NULL) STRICT;\
         CREATE TABLE IF NOT EXISTS {INDEXES_META_TABLE} \
             (name TEXT PRIMARY KEY, column_name TEXT NOT NULL) STRICT;"
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
fn record_column(connection: &Connection, name: &str, field_type: FieldType) -> anyhow::Result<()> {
    connection.execute(
        &format!(
            "INSERT OR REPLACE INTO {COLUMNS_META_TABLE} (name, logical_type) VALUES (?1, ?2)"
        ),
        rusqlite::params![name, logical_type_name(field_type)],
    )?;
    Ok(())
}
fn record_index(connection: &Connection, name: &str, column: &str) -> anyhow::Result<()> {
    connection.execute(
        &format!("INSERT OR REPLACE INTO {INDEXES_META_TABLE} (name, column_name) VALUES (?1, ?2)"),
        rusqlite::params![name, column],
    )?;
    Ok(())
}
fn managed_columns(connection: &Connection) -> anyhow::Result<HashMap<String, String>> {
    let mut statement = connection.prepare(&format!(
        "SELECT name, logical_type FROM {COLUMNS_META_TABLE}"
    ))?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.collect::<rusqlite::Result<HashMap<_, _>>>()
        .map_err(|error| anyhow::anyhow!("failed to read sqlite column metadata: {error}"))
}
fn managed_indexes(connection: &Connection) -> anyhow::Result<HashMap<String, String>> {
    let mut statement = connection.prepare(&format!(
        "SELECT name, column_name FROM {INDEXES_META_TABLE}"
    ))?;
    let rows = statement.query_map([], |row| {
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
    let primary_key = schema.primary_key_field().expect("validated schema");
    let mut definitions = Vec::new();
    for field in schema.fields() {
        let definition = if field.name() == primary_key.name() {
            format!(
                "\"{}\" {} NOT NULL PRIMARY KEY",
                field.name(),
                <E::Key as SqlKey>::physical_type()
            )
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
    connection.execute(
        &format!(
            "CREATE TABLE \"{table}\" ({}) STRICT",
            definitions.join(", ")
        ),
        [],
    )?;
    for field in schema.fields() {
        record_column(connection, field.name(), field.field_type())?;
    }
    for (name, column) in declared_indexes(schema, table) {
        connection.execute(
            &format!("CREATE INDEX \"{name}\" ON \"{table}\" (\"{column}\")"),
            [],
        )?;
        record_index(connection, &name, &column)?;
    }
    Ok(())
}
fn evolve_table<E: PersistentEntity>(
    connection: &Connection,
    table: &str,
    config: &PersistenceStoreConfig,
) -> anyhow::Result<()> {
    let schema = &config.schema;
    let primary_key = schema.primary_key_field().expect("validated schema");
    let managed = managed_columns(connection)?;
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
    let primary_key_columns: Vec<&(String, String, i64)> =
        existing.iter().filter(|(_, _, pk)| *pk > 0).collect();
    if primary_key_columns.len() != 1 {
        anyhow::bail!("sqlite table '{table}' must declare exactly one primary key column");
    }
    let (pk_name, _, _) = primary_key_columns[0];
    if pk_name != primary_key.name() {
        anyhow::bail!(
            "sqlite table '{table}' primary key is '{pk_name}' but the entity declares '{}'",
            primary_key.name()
        );
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
            record_column(connection, field.name(), field.field_type())?;
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
    let managed = managed_indexes(connection)?;
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
        record_index(connection, name, column)?;
    }
    for name in managed.keys() {
        if !declared.iter().any(|(declared, _)| declared == name) {
            connection.execute(&format!("DROP INDEX IF EXISTS \"{name}\""), [])?;
            connection.execute(
                &format!("DELETE FROM {INDEXES_META_TABLE} WHERE name = ?1"),
                [name],
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
        let connection = self.connection.clone();
        let table = self.table.clone();
        let config = self.config.clone();
        let key_value = key.to_sql();
        let task = tokio::task::spawn_blocking(move || {
            let connection = connection.lock().map_err(|_| lock_poisoned())?;
            let schema = &config.schema;
            let primary_key = schema.primary_key_field().expect("validated schema");
            let mut statement = connection.prepare(&format!(
                "SELECT {} FROM \"{table}\" WHERE \"{}\" = ?1",
                column_list(schema),
                primary_key.name()
            ))?;
            let mut rows = statement.query(rusqlite::params![key_value])?;
            let Some(row) = rows.next()? else {
                return Ok(None);
            };
            let document = read_row::<E>(schema, row)?;
            drop(rows);
            drop(statement);
            if expired(schema, &document)? {
                connection.execute(
                    &format!(
                        "DELETE FROM \"{table}\" WHERE \"{}\" = ?1",
                        primary_key.name()
                    ),
                    rusqlite::params![key_value],
                )?;
                return Ok(None);
            }
            Ok(Some(decode::<E>(schema, &document)?))
        });
        task.await.map_err(join_error)?
    }
    async fn put(&self, entity: &E) -> anyhow::Result<()> {
        let connection = self.connection.clone();
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
        let connection = self.connection.clone();
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
        let connection = self.connection.clone();
        let table = self.table.clone();
        let config = self.config.clone();
        let key_value = key.to_sql();
        let task = tokio::task::spawn_blocking(move || {
            let connection = connection.lock().map_err(|_| lock_poisoned())?;
            let primary_key = config.schema.primary_key_field().expect("validated schema");
            connection.execute(
                &format!(
                    "DELETE FROM \"{table}\" WHERE \"{}\" = ?1",
                    primary_key.name()
                ),
                rusqlite::params![key_value],
            )?;
            Ok(())
        });
        task.await.map_err(join_error)?
    }
    async fn find(&self, query: &EntityQuery<E>) -> anyhow::Result<Vec<E>> {
        let connection = self.connection.clone();
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
mod tests {
    use super::*;
    use crate::persistence::typed_store::{EntityReader, EntityWriter, Field};
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };

    fn temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "arachnea-sqlite-{}-{}-{}",
            label,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[derive(Debug, Clone, PartialEq)]
    struct TestEntity {
        authority: String,
        name: String,
        port: i64,
        expires_at: SystemTime,
    }
    impl PersistentEntity for TestEntity {
        type Key = String;
        fn key(&self) -> String {
            self.authority.clone()
        }
        fn schema() -> EntitySchema {
            Self::schema_with_region(false)
        }
        fn write_to(&self, writer: &mut EntityWriter) -> anyhow::Result<()> {
            writer.string("authority", self.authority.clone())?;
            writer.string("name", self.name.clone())?;
            writer.integer("port", self.port)?;
            writer.date_time("expires_at", self.expires_at)?;
            Ok(())
        }
        fn read_from(reader: &EntityReader<'_>) -> anyhow::Result<Self> {
            Ok(Self {
                authority: reader.string("authority")?.to_string(),
                name: reader.string("name")?.to_string(),
                port: reader.integer("port")?,
                expires_at: reader.date_time("expires_at")?,
            })
        }
    }
    impl TestEntity {
        fn schema_with_region(with_region: bool) -> EntitySchema {
            let schema = EntitySchema::new()
                .primary_key(Field::string("authority"))
                .field(Field::string("name").indexed())
                .field(Field::integer("port"))
                .field(Field::date_time("expires_at").expiration());
            if with_region {
                schema.field(Field::string("region").nullable())
            } else {
                schema
            }
        }
    }
    fn entity(authority: &str, name: &str, ttl_secs: u64) -> TestEntity {
        TestEntity {
            authority: authority.to_string(),
            name: name.to_string(),
            port: 8080,
            expires_at: SystemTime::now() + Duration::from_secs(ttl_secs),
        }
    }
    async fn store(
        dir: &PathBuf,
        with_region: bool,
    ) -> anyhow::Result<SqliteEntityStore<TestEntity>> {
        SqliteEntityStore::new(
            PersistenceStoreConfig::new(
                "test-inventory",
                TestEntity::schema_with_region(with_region),
            )?,
            dir,
        )
    }

    #[tokio::test]
    async fn put_get_delete_roundtrip() -> anyhow::Result<()> {
        let dir = temp_dir("roundtrip");
        let store = store(&dir, false).await?;
        store.put(&entity("h1:80", "a", 60)).await?;
        let loaded = store.get(&"h1:80".to_string()).await?.expect("entity");
        assert_eq!(loaded.name, "a");
        assert_eq!(loaded.port, 8080);
        assert!(store.get(&"missing".to_string()).await?.is_none());
        store.delete(&"h1:80".to_string()).await?;
        assert!(store.get(&"h1:80".to_string()).await?.is_none());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[tokio::test]
    async fn expired_entries_are_filtered_and_purged() -> anyhow::Result<()> {
        let dir = temp_dir("expiration");
        let store = store(&dir, false).await?;
        store.put(&entity("stale:1", "keep-name", 0)).await?;
        store.put(&entity("fresh:1", "keep-name", 60)).await?;
        assert!(store.get(&"stale:1".to_string()).await?.is_none());
        let found = store.find(&EntityQuery::<TestEntity>::new()).await?;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].authority, "fresh:1");
        let found = store
            .find(&EntityQuery::<TestEntity>::new().where_string(
                TestEntity::schema().field_named("name").unwrap(),
                "keep-name",
            ))
            .await?;
        assert_eq!(found.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[tokio::test]
    async fn put_all_is_atomic() -> anyhow::Result<()> {
        let dir = temp_dir("batch");
        let store = store(&dir, false).await?;
        let batch = vec![entity("b:1", "n1", 60), entity("b:2", "n2", 60)];
        store.put_all(&batch).await?;
        let found = store.find(&EntityQuery::<TestEntity>::new()).await?;
        assert_eq!(found.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[tokio::test]
    async fn reopening_adds_declared_columns_and_indexes() -> anyhow::Result<()> {
        let dir = temp_dir("evolution");
        drop(store(&dir, false).await?);
        // Reopen with an entity whose schema declares an extra nullable field:
        // schema evolution must succeed and keep the store usable.
        let store = SqliteEntityStore::<TestEntityV2>::new(
            PersistenceStoreConfig::new("test-inventory", TestEntityV2::schema())?,
            &dir,
        )?;
        let found = store.find(&EntityQuery::<TestEntityV2>::new()).await?;
        assert!(found.is_empty());
        store
            .put(&TestEntityV2 {
                authority: "e:1".to_string(),
                name: "n".to_string(),
                port: 80,
                expires_at: SystemTime::now() + Duration::from_secs(60),
                region: Some("eu".to_string()),
            })
            .await?;
        let loaded = store.get(&"e:1".to_string()).await?.expect("entity");
        assert_eq!(loaded.region.as_deref(), Some("eu"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[derive(Debug, Clone, PartialEq)]
    struct TestEntityV2 {
        authority: String,
        name: String,
        port: i64,
        expires_at: SystemTime,
        region: Option<String>,
    }
    impl PersistentEntity for TestEntityV2 {
        type Key = String;
        fn key(&self) -> String {
            self.authority.clone()
        }
        fn schema() -> EntitySchema {
            EntitySchema::new()
                .primary_key(Field::string("authority"))
                .field(Field::string("name").indexed())
                .field(Field::integer("port"))
                .field(Field::date_time("expires_at").expiration())
                .field(Field::string("region").nullable())
        }
        fn write_to(&self, writer: &mut EntityWriter) -> anyhow::Result<()> {
            writer.string("authority", self.authority.clone())?;
            writer.string("name", self.name.clone())?;
            writer.integer("port", self.port)?;
            writer.date_time("expires_at", self.expires_at)?;
            if let Some(region) = &self.region {
                writer.string("region", region.clone())?;
            }
            Ok(())
        }
        fn read_from(reader: &EntityReader<'_>) -> anyhow::Result<Self> {
            Ok(Self {
                authority: reader.string("authority")?.to_string(),
                name: reader.string("name")?.to_string(),
                port: reader.integer("port")?,
                expires_at: reader.date_time("expires_at")?,
                region: reader.optional_string("region")?.map(str::to_string),
            })
        }
    }

    #[tokio::test]
    async fn incompatible_type_change_fails() -> anyhow::Result<()> {
        let dir = temp_dir("type-conflict");
        drop(store(&dir, false).await?);
        #[derive(Clone)]
        struct Conflicting;
        impl PersistentEntity for Conflicting {
            type Key = String;
            fn key(&self) -> String {
                String::new()
            }
            fn schema() -> EntitySchema {
                EntitySchema::new()
                    .primary_key(Field::string("authority"))
                    .field(Field::string("name"))
                    .field(Field::string("port"))
                    .field(Field::date_time("expires_at").expiration())
            }
            fn write_to(&self, _writer: &mut EntityWriter) -> anyhow::Result<()> {
                unreachable!()
            }
            fn read_from(_reader: &EntityReader<'_>) -> anyhow::Result<Self> {
                unreachable!()
            }
        }
        let result = SqliteEntityStore::<Conflicting>::new(
            PersistenceStoreConfig::new("test-inventory", Conflicting::schema())?,
            &dir,
        );
        assert!(result.is_err(), "type change must be rejected");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
