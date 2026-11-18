//! Synchronous typed transactions spanning tables in one physical database.

use super::*;
use anyhow::Context;
use rusqlite::TransactionBehavior;

/// Borrowed transaction restricted to table views from its physical database.
///
/// Callbacks must not invoke async table methods or nest transactions on the
/// same database: the connection is already locked. Use on a blocking thread.
pub struct SqliteTransaction<'a> {
    connection: &'a Connection,
    owner: &'a Arc<Mutex<Connection>>,
}

impl SqliteDatabase {
    /// Opens a private in-memory database with the same typed transaction API.
    ///
    /// # Errors
    /// Returns an error if SQLite cannot allocate/configure the connection.
    pub fn memory() -> anyhow::Result<Self> {
        Ok(Self {
            connection: Arc::new(Mutex::new(Connection::open_in_memory()?)),
        })
    }

    /// Runs a local IMMEDIATE transaction, committing only a successful callback.
    ///
    /// # Arguments
    /// * `callback` - Synchronous reads/writes using views from this database.
    ///
    /// # Errors
    /// Rolls back on callback failure; reports lock, begin or commit errors.
    /// A panic unwinds and rolls back, poisoning the connection lock.
    pub fn transaction<T>(
        &self,
        callback: impl FnOnce(&SqliteTransaction<'_>) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let mut connection = self.connection.lock().map_err(|_| lock_poisoned())?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = callback(&SqliteTransaction {
            connection: &transaction,
            owner: &self.connection,
        })?;
        transaction
            .commit()
            .context("failed to commit typed SQLite transaction")?;
        Ok(value)
    }
}

impl SqliteTransaction<'_> {
    /// Checks that a table belongs to this connection before accessing it.
    ///
    /// # Arguments
    /// * `table` - Open typed table view to validate.
    fn check<E: PersistentEntity>(&self, table: &SqliteEntityStore<E>) -> anyhow::Result<()> {
        anyhow::ensure!(
            Arc::ptr_eq(self.owner, &table.database.connection),
            "transaction table belongs to another SQLite database"
        );
        Ok(())
    }

    /// Reads one non-expired entity within the transaction snapshot.
    ///
    /// # Arguments
    /// * `table` - Table belonging to this database.
    /// * `key` - Complete primary key.
    ///
    /// # Errors
    /// Returns an ownership, SQL or decoding error.
    pub fn get<E: PersistentEntity>(
        &self,
        table: &SqliteEntityStore<E>,
        key: &E::Key,
    ) -> anyhow::Result<Option<E>>
    where
        E::Key: SqlKey,
    {
        self.check(table)?;
        let mut statement = self.connection.prepare(&format!(
            "SELECT {} FROM \"{}\" WHERE {}",
            column_list(&table.config.schema),
            table.table,
            primary_key_predicate(&table.config.schema)
        ))?;
        let mut rows = statement.query(rusqlite::params_from_iter(key.to_sql_values()))?;
        match rows.next()? {
            None => Ok(None),
            Some(row) => {
                let document = read_row::<E>(&table.config.schema, row)?;
                if expired(&table.config.schema, &document)? {
                    return Ok(None);
                }
                Ok(Some(decode(&table.config.schema, &document)?))
            }
        }
    }

    /// Finds non-expired entities with validated equality predicates.
    ///
    /// # Arguments
    /// * `table` - Table belonging to this database.
    /// * `query` - Declared-field equality filters; empty matches every row.
    ///
    /// # Errors
    /// Returns an ownership, predicate, SQL or decoding error.
    pub fn find<E: PersistentEntity>(
        &self,
        table: &SqliteEntityStore<E>,
        query: &EntityQuery<E>,
    ) -> anyhow::Result<Vec<E>>
    where
        E::Key: SqlKey,
    {
        self.check(table)?;
        let (clause, values) = predicates(table, query)?;
        let mut statement = self.connection.prepare(&format!(
            "SELECT {} FROM \"{}\"{clause}",
            column_list(&table.config.schema),
            table.table
        ))?;
        let mut rows = statement.query(rusqlite::params_from_iter(values))?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            let document = read_row::<E>(&table.config.schema, row)?;
            if !expired(&table.config.schema, &document)? {
                result.push(decode(&table.config.schema, &document)?);
            }
        }
        Ok(result)
    }

    /// Writes an entity using primary-key upsert, preserving unique constraints.
    ///
    /// Unlike REPLACE, a secondary unique collision never deletes another row.
    ///
    /// # Arguments
    /// * `table` - Table belonging to this database.
    /// * `entity` - Complete schema-valid entity.
    ///
    /// # Errors
    /// Returns validation, ownership, uniqueness or SQL errors.
    pub fn put<E: PersistentEntity>(
        &self,
        table: &SqliteEntityStore<E>,
        entity: &E,
    ) -> anyhow::Result<()>
    where
        E::Key: SqlKey,
    {
        self.check(table)?;
        let document = encode(&table.config, entity)?;
        let schema = &table.config.schema;
        let key_values = entity.key().to_sql_values();
        let primary = schema.primary_key_fields();
        let values = schema
            .fields()
            .iter()
            .map(|field| {
                if let Some(index) = primary.iter().position(|key| key.name() == field.name()) {
                    return Ok(key_values[index].clone());
                }
                document
                    .fields
                    .get(field.name())
                    .map(to_sql_value)
                    .unwrap_or(Ok(SqlValue::Null))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let updates = schema
            .fields()
            .iter()
            .filter(|field| !primary.iter().any(|key| key.name() == field.name()))
            .map(|field| format!("{} = excluded.{}", quote(field.name()), quote(field.name())))
            .collect::<Vec<_>>();
        let action = if updates.is_empty() {
            "DO NOTHING".to_string()
        } else {
            format!("DO UPDATE SET {}", updates.join(", "))
        };
        self.connection.execute(
            &format!(
                "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({}) {action}",
                quote(&table.table),
                column_list(schema),
                (1..=values.len())
                    .map(|i| format!("?{i}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                primary
                    .iter()
                    .map(|field| quote(field.name()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            rusqlite::params_from_iter(values),
        )?;
        Ok(())
    }

    /// Deletes all matching rows, including expired rows, atomically.
    ///
    /// # Arguments
    /// * `table` - Table belonging to this database.
    /// * `query` - Validated filters; empty deletes the whole table.
    ///
    /// # Errors
    /// Returns an ownership, predicate or SQL error.
    pub fn delete_where<E: PersistentEntity>(
        &self,
        table: &SqliteEntityStore<E>,
        query: &EntityQuery<E>,
    ) -> anyhow::Result<()>
    where
        E::Key: SqlKey,
    {
        self.check(table)?;
        let (clause, values) = predicates(table, query)?;
        self.connection.execute(
            &format!("DELETE FROM {}{clause}", quote(&table.table)),
            rusqlite::params_from_iter(values),
        )?;
        Ok(())
    }

    /// Creates or verifies a unique index on declared fields in this transaction.
    ///
    /// These indexes are caller-owned, preserved by typed schema reconciliation.
    /// Call again on opening repositories; incompatible indexes fail explicitly.
    ///
    /// # Arguments
    /// * `table` - Table belonging to this database.
    /// * `fields` - Non-empty ordered field names defining uniqueness.
    ///
    /// # Errors
    /// Returns invalid-field, duplicate-data, incompatible-index or SQL errors.
    pub fn ensure_unique<E: PersistentEntity>(
        &self,
        table: &SqliteEntityStore<E>,
        fields: &[&str],
    ) -> anyhow::Result<()>
    where
        E::Key: SqlKey,
    {
        self.check(table)?;
        anyhow::ensure!(!fields.is_empty(), "unique index requires fields");
        let mut seen = std::collections::HashSet::new();
        for field in fields {
            anyhow::ensure!(
                table.config.schema.field_named(field).is_some() && seen.insert(field),
                "invalid unique index field {field}"
            );
        }
        let suffix = fields
            .iter()
            .map(|field| format!("{}_{}", field.len(), safe_name(field)))
            .collect::<Vec<_>>()
            .join("_");
        let name = format!("unique_{}_{}_{suffix}", table.table.len(), table.table);
        self.connection.execute_batch(&format!(
            "CREATE UNIQUE INDEX IF NOT EXISTS {} ON {} ({})",
            quote(&name),
            quote(&table.table),
            fields
                .iter()
                .map(|field| quote(field))
                .collect::<Vec<_>>()
                .join(", ")
        ))?;
        let mut statement = self
            .connection
            .prepare(&format!("PRAGMA index_list({})", quote(&table.table)))?;
        let indexes = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, bool>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        anyhow::ensure!(
            indexes
                .iter()
                .any(|(index, unique, partial)| index == &name && *unique && !partial),
            "incompatible unique index {name}"
        );
        let mut statement = self
            .connection
            .prepare(&format!("PRAGMA index_info({})", quote(&name)))?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(2))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        anyhow::ensure!(
            columns
                .iter()
                .map(String::as_str)
                .eq(fields.iter().copied()),
            "incompatible unique index columns {name}"
        );
        Ok(())
    }
}

/// Quotes a SQLite identifier without allowing embedded SQL.
fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Validates filters and builds a bound WHERE clause.
fn predicates<E: PersistentEntity>(
    table: &SqliteEntityStore<E>,
    query: &EntityQuery<E>,
) -> anyhow::Result<(String, Vec<SqlValue>)>
where
    E::Key: SqlKey,
{
    let mut clauses = Vec::new();
    let mut values = Vec::new();
    for (name, value) in query.predicates() {
        let field = table
            .config
            .schema
            .field_named(name)
            .context("query field is not declared")?;
        anyhow::ensure!(
            field.field_type() == value.field_type(),
            "query field type mismatch: {name}"
        );
        anyhow::ensure!(
            !table
                .config
                .schema
                .primary_key_fields()
                .iter()
                .enumerate()
                .any(|(i, key)| key.name() == name
                    && E::Key::physical_types()[i] != "TEXT"
                    && field.field_type() == super::super::typed_store::FieldType::String),
            "non-text primary-key filter requires get"
        );
        values.push(to_sql_value(value)?);
        clauses.push(format!("{} = ?{}", quote(name), values.len()));
    }
    Ok((
        if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        },
        values,
    ))
}
