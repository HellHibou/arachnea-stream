use super::super::test_support;
use super::*;
use crate::persistence::typed_store::{EntityReader, EntityWriter, Field};
use std::time::Duration;

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
async fn store(dir: &PathBuf, with_region: bool) -> anyhow::Result<SqliteEntityStore<TestEntity>> {
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
    let dir = test_support::temp_dir("arachnea-sqlite", "roundtrip");
    let database = SqliteDatabase::open(SqliteStoreConfig::new("test-inventory")?, &dir)?;
    let store = database.table::<TestEntity>(PersistenceTableConfig::new(
        "test-inventory",
        TestEntity::schema(),
    )?)?;
    let other = database.table::<TestEntity>(PersistenceTableConfig::new(
        "other-table",
        TestEntity::schema(),
    )?)?;
    assert!(Arc::ptr_eq(
        &store.database.connection,
        &other.database.connection
    ));
    assert!(dir.join("test-inventory.sqlite3").is_file());
    assert!(!dir.join("test-inventory").exists());
    assert!(!dir.join("other-table.sqlite3").exists());
    assert!(database
        .table::<TestEntity>(PersistenceTableConfig::new(
            "arachnea_columns",
            TestEntity::schema(),
        )?)
        .is_err());
    drop(database);
    store.put(&entity("h1:80", "a", 60)).await?;
    other.put(&entity("h1:80", "other", 60)).await?;
    let loaded = store.get(&"h1:80".to_string()).await?.expect("entity");
    assert_eq!(loaded.name, "a");
    assert_eq!(loaded.port, 8080);
    assert!(store.get(&"missing".to_string()).await?.is_none());
    store.delete(&"h1:80".to_string()).await?;
    assert!(store.get(&"h1:80".to_string()).await?.is_none());
    assert_eq!(
        other.get(&"h1:80".to_string()).await?.unwrap().name,
        "other"
    );
    let independent = SqliteDatabase::open(SqliteStoreConfig::new("independent")?, &dir)?;
    let separate = independent.table::<TestEntity>(PersistenceTableConfig::new(
        "other-table",
        TestEntity::schema(),
    )?)?;
    assert!(separate.get(&"h1:80".to_string()).await?.is_none());
    assert!(dir.join("independent.sqlite3").is_file());
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[tokio::test]
async fn expired_entries_are_filtered_and_purged() -> anyhow::Result<()> {
    let dir = test_support::temp_dir("arachnea-sqlite", "expiration");
    let store = store(&dir, false).await?;
    store.put(&entity("stale:1", "keep-name", 0)).await?;
    store.put(&entity("fresh:1", "keep-name", 60)).await?;
    let keys = vec![
        "stale:1".to_string(),
        "fresh:1".to_string(),
        "missing:1".to_string(),
        "fresh:1".to_string(),
    ];
    let grouped = store.get_many(&keys).await?;
    assert!(grouped[0].is_none());
    assert!(grouped[2].is_none());
    assert_eq!(grouped[1].as_ref().unwrap().authority, "fresh:1");
    assert_eq!(grouped[3].as_ref().unwrap().authority, "fresh:1");
    let connection = store
        .database
        .connection
        .lock()
        .map_err(|_| lock_poisoned())?;
    let stale: i64 = connection.query_row(
        &format!(
            "SELECT COUNT(*) FROM \"{}\" WHERE authority = 'stale:1'",
            store.table
        ),
        [],
        |row| row.get(0),
    )?;
    assert_eq!(stale, 0);
    drop(connection);
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
    store.put(&entity("cancelled:1", "keep-name", 0)).await?;
    let mut cancelled_keys = vec!["fresh:1".to_string(); SQLITE_READ_BATCH_KEYS];
    cancelled_keys.push("cancelled:1".to_string());
    {
        let connection = store
            .database
            .connection
            .lock()
            .map_err(|_| lock_poisoned())?;
        let mut pending = Box::pin(store.get_many(&cancelled_keys));
        assert!(futures::poll!(&mut pending).is_pending());
        drop(pending);
        drop(connection);
    }
    assert!(store.get(&"fresh:1".to_string()).await?.is_some());
    {
        let connection = store
            .database
            .connection
            .lock()
            .map_err(|_| lock_poisoned())?;
        let retained: i64 = connection.query_row(
            &format!(
                "SELECT COUNT(*) FROM \"{}\" WHERE authority = ?1",
                store.table
            ),
            ["cancelled:1"],
            |row| row.get(0),
        )?;
        assert_eq!(
            retained, 1,
            "cancellation must not start the second batch or purge its expired row"
        );
        connection.execute(&format!("DROP TABLE \"{}\"", store.table), [])?;
    }
    assert!(store.get_many(&keys).await.is_err());
    assert!(store.get_many(&[]).await?.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[tokio::test]
async fn put_all_is_atomic() -> anyhow::Result<()> {
    let dir = test_support::temp_dir("arachnea-sqlite", "batch");
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
    let dir = test_support::temp_dir("arachnea-sqlite", "evolution");
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
    let dir = test_support::temp_dir("arachnea-sqlite", "type-conflict");
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
    let database = SqliteDatabase::open(SqliteStoreConfig::new("test-inventory")?, &dir)?;
    // A same-named field in another table can have a different logical type.
    drop(database.table::<Conflicting>(PersistenceTableConfig::new(
        "different-schema",
        Conflicting::schema(),
    )?)?);
    drop(database.table::<TestEntity>(PersistenceTableConfig::new(
        "test-inventory",
        TestEntity::schema(),
    )?)?);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[tokio::test]
async fn restart_keeps_persisted_entities() -> anyhow::Result<()> {
    let dir = test_support::temp_dir("arachnea-sqlite", "restart");
    let legacy = dir.join("test-inventory");
    std::fs::create_dir_all(&legacy)?;
    let legacy_path = legacy.join("records.sqlite3");
    let connection = Connection::open(&legacy_path)?;
    connection.execute_batch(
        "CREATE TABLE legacy_data (value TEXT); INSERT INTO legacy_data VALUES ('ignored');",
    )?;
    drop(connection);
    {
        let store = store(&dir, false).await?;
        store.put(&entity("r:1", "n1", 60)).await?;
        store.put(&entity("r:2", "n2", 60)).await?;
    }
    let store = store(&dir, false).await?;
    assert!(legacy_path.is_file());
    let connection = Connection::open(db_path(&dir))?;
    assert_eq!(
        connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'legacy_data'",
            [],
            |row| row.get::<_, i64>(0),
        )?,
        0
    );
    assert_eq!(
        store.get(&"r:1".to_string()).await?.expect("entity").name,
        "n1"
    );
    assert_eq!(
        store.get(&"r:2".to_string()).await?.expect("entity").name,
        "n2"
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[tokio::test]
async fn find_combines_predicates_with_and() -> anyhow::Result<()> {
    let dir = test_support::temp_dir("arachnea-sqlite", "and-query");
    let store = store(&dir, false).await?;
    let mut first = entity("a:1", "same", 60);
    first.port = 8080;
    let mut second = entity("a:2", "same", 60);
    second.port = 9090;
    store
        .put_all(&[first, second, entity("a:3", "other", 60)])
        .await?;
    let schema = TestEntity::schema();
    let found = store
        .find(
            &EntityQuery::<TestEntity>::new()
                .where_string(schema.field_named("name").unwrap(), "same")
                .where_integer(schema.field_named("port").unwrap(), 8080),
        )
        .await?;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].authority, "a:1");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// Same columns as [`TestEntity`] but with no declared index on `name`.
#[derive(Debug, Clone, PartialEq)]
struct TestEntityPlain {
    authority: String,
    name: String,
    port: i64,
    expires_at: SystemTime,
}
impl PersistentEntity for TestEntityPlain {
    type Key = String;
    fn key(&self) -> String {
        self.authority.clone()
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("authority"))
            .field(Field::string("name"))
            .field(Field::integer("port"))
            .field(Field::date_time("expires_at").expiration())
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

fn db_path(dir: &PathBuf) -> PathBuf {
    dir.join("test-inventory.sqlite3")
}

fn physical_index_exists(dir: &PathBuf, name: &str) -> bool {
    let connection = Connection::open(db_path(dir)).expect("open sqlite database");
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
            [name],
            |row| row.get(0),
        )
        .expect("index lookup");
    count > 0
}

#[tokio::test]
async fn removed_declared_index_is_dropped() -> anyhow::Result<()> {
    let dir = test_support::temp_dir("arachnea-sqlite", "index-drop");
    drop(store(&dir, false).await?);
    assert!(physical_index_exists(
        &dir,
        &index_name("test-inventory", "name")
    ));
    let database = SqliteDatabase::open(SqliteStoreConfig::new("test-inventory")?, &dir)?;
    drop(database.table::<TestEntity>(PersistenceTableConfig::new(
        "other-table",
        TestEntity::schema(),
    )?)?);
    drop(
        database.table::<TestEntityPlain>(PersistenceTableConfig::new(
            "no-index",
            TestEntityPlain::schema(),
        )?)?,
    );
    assert!(physical_index_exists(
        &dir,
        &index_name("test-inventory", "name")
    ));
    assert!(physical_index_exists(
        &dir,
        &index_name("other-table", "name")
    ));
    #[derive(Clone)]
    struct CollisionSchema;
    impl PersistentEntity for CollisionSchema {
        type Key = String;
        fn key(&self) -> String {
            unreachable!()
        }
        fn schema() -> EntitySchema {
            EntitySchema::new()
                .primary_key(Field::string("authority"))
                .field(Field::string("b_name").indexed())
        }
        fn write_to(&self, _writer: &mut EntityWriter) -> anyhow::Result<()> {
            unreachable!()
        }
        fn read_from(_reader: &EntityReader<'_>) -> anyhow::Result<Self> {
            unreachable!()
        }
    }
    // Both pairs previously generated idx_a_b_name in the shared database.
    drop(database.table::<TestEntity>(PersistenceTableConfig::new("a_b", TestEntity::schema())?)?);
    drop(
        database.table::<CollisionSchema>(PersistenceTableConfig::new(
            "a",
            CollisionSchema::schema(),
        )?)?,
    );
    assert!(physical_index_exists(&dir, "arachnea_idx_3_a_b_name"));
    assert!(physical_index_exists(&dir, "arachnea_idx_1_a_b_name"));
    drop(database);
    let reopened = SqliteDatabase::open(SqliteStoreConfig::new("test-inventory")?, &dir)?;
    drop(reopened.table::<TestEntity>(PersistenceTableConfig::new("a_b", TestEntity::schema())?)?);
    drop(
        reopened.table::<CollisionSchema>(PersistenceTableConfig::new(
            "a",
            CollisionSchema::schema(),
        )?)?,
    );
    // Reopening with the same columns but without the declared index on
    // `name` must drop the managed index while keeping the data usable.
    let store = SqliteEntityStore::<TestEntityPlain>::new(
        PersistenceStoreConfig::new("test-inventory", TestEntityPlain::schema())?,
        &dir,
    )?;
    assert!(!physical_index_exists(
        &dir,
        &index_name("test-inventory", "name")
    ));
    assert!(physical_index_exists(
        &dir,
        &index_name("other-table", "name")
    ));
    assert!(physical_index_exists(&dir, "arachnea_idx_3_a_b_name"));
    assert!(physical_index_exists(&dir, "arachnea_idx_1_a_b_name"));
    store
        .put(&TestEntityPlain {
            authority: "p:1".to_string(),
            name: "n".to_string(),
            port: 80,
            expires_at: SystemTime::now() + Duration::from_secs(60),
        })
        .await?;
    assert!(store.get(&"p:1".to_string()).await?.is_some());
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[tokio::test]
async fn external_indexes_are_preserved() -> anyhow::Result<()> {
    let dir = test_support::temp_dir("arachnea-sqlite", "external-index");
    {
        let store = store(&dir, false).await?;
        store.put(&entity("e:1", "n", 60)).await?;
    }
    {
        let connection = Connection::open(db_path(&dir))?;
        let managed_name = index_name("test-inventory", "name");
        connection.execute(&format!("DROP INDEX \"{managed_name}\""), [])?;
        connection.execute(
            "CREATE INDEX \"idx_test-inventory_name\" ON \"test-inventory\" (\"name\")",
            [],
        )?;
        connection.execute(
                "UPDATE arachnea_indexes SET name = 'idx_test-inventory_name' WHERE table_name = 'test-inventory' AND name = ?1",
                [&managed_name],
            )?;
        connection.execute(
            "CREATE INDEX \"external_custom\" ON \"test-inventory\" (\"port\")",
            [],
        )?;
    }
    // Reopening replaces legacy managed names but preserves external indexes and data.
    let reopened = store(&dir, false).await?;
    assert!(physical_index_exists(&dir, "external_custom"));
    assert!(!physical_index_exists(&dir, "idx_test-inventory_name"));
    assert!(physical_index_exists(
        &dir,
        &index_name("test-inventory", "name")
    ));
    assert!(reopened.get(&"e:1".to_string()).await?.is_some());
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
