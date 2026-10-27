use super::super::test_support;
use super::*;
use std::time::Duration;

fn test_schema() -> EntitySchema {
    EntitySchema::new()
        .primary_key(Field::string("authority"))
        .field(Field::string("name").indexed())
        .field(Field::integer("port"))
        .field(Field::date_time("expires_at").expiration())
}

fn config() -> PersistenceStoreConfig {
    PersistenceStoreConfig::new("typed-store-test", test_schema()).expect("valid config")
}

fn mismatched_config() -> PersistenceStoreConfig {
    // Same store contract but a schema that does not match the entities.
    PersistenceStoreConfig::new(
        "typed-store-test",
        EntitySchema::new()
            .primary_key(Field::string("authority"))
            .field(Field::string("name"))
            .field(Field::string("port"))
            .field(Field::date_time("expires_at").expiration()),
    )
    .expect("valid config")
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
        test_schema()
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

/// Same schema as [`TestEntity`] but the port stays optional: an entity
/// without a port fails validation because the field is required.
#[derive(Debug, Clone, PartialEq)]
struct FlexibleEntity {
    authority: String,
    name: String,
    port: Option<i64>,
    expires_at: SystemTime,
}
impl PersistentEntity for FlexibleEntity {
    type Key = String;
    fn key(&self) -> String {
        self.authority.clone()
    }
    fn schema() -> EntitySchema {
        test_schema()
    }
    fn write_to(&self, writer: &mut EntityWriter) -> anyhow::Result<()> {
        writer.string("authority", self.authority.clone())?;
        writer.string("name", self.name.clone())?;
        if let Some(port) = self.port {
            writer.integer("port", port)?;
        }
        writer.date_time("expires_at", self.expires_at)?;
        Ok(())
    }
    fn read_from(reader: &EntityReader<'_>) -> anyhow::Result<Self> {
        Ok(Self {
            authority: reader.string("authority")?.to_string(),
            name: reader.string("name")?.to_string(),
            port: Some(reader.integer("port")?),
            expires_at: reader.date_time("expires_at")?,
        })
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

fn flexible(authority: &str, name: &str, port: Option<i64>) -> FlexibleEntity {
    FlexibleEntity {
        authority: authority.to_string(),
        name: name.to_string(),
        port,
        expires_at: SystemTime::now() + Duration::from_secs(60),
    }
}

#[tokio::test]
async fn memory_put_get_delete_roundtrip() -> anyhow::Result<()> {
    let store = MemoryEntityStore::<TestEntity>::new(config())?;
    store.put(&entity("m:1", "a", 60)).await?;
    let loaded = store.get(&"m:1".to_string()).await?.expect("entity");
    assert_eq!(loaded.name, "a");
    assert_eq!(loaded.port, 8080);
    assert!(store.get(&"missing".to_string()).await?.is_none());
    store.delete(&"m:1".to_string()).await?;
    assert!(store.get(&"m:1".to_string()).await?.is_none());
    Ok(())
}

#[tokio::test]
async fn memory_find_combines_predicates_with_and() -> anyhow::Result<()> {
    let store = MemoryEntityStore::<TestEntity>::new(config())?;
    let mut first = entity("m:1", "same", 60);
    first.port = 8080;
    let mut second = entity("m:2", "same", 60);
    second.port = 9090;
    store
        .put_all(&[first, second, entity("m:3", "other", 60)])
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
    assert_eq!(found[0].authority, "m:1");
    Ok(())
}

#[tokio::test]
async fn memory_expired_entries_are_filtered() -> anyhow::Result<()> {
    let store = MemoryEntityStore::<TestEntity>::new(config())?;
    store.put(&entity("stale:1", "n", 0)).await?;
    store.put(&entity("fresh:1", "n", 60)).await?;
    assert!(store.get(&"stale:1".to_string()).await?.is_none());
    let found = store.find(&EntityQuery::<TestEntity>::new()).await?;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].authority, "fresh:1");
    Ok(())
}

#[tokio::test]
async fn memory_put_all_rejects_invalid_batch_atomically() -> anyhow::Result<()> {
    let store = MemoryEntityStore::<FlexibleEntity>::new(config())?;
    store.put(&flexible("kept:1", "n", Some(8080))).await?;
    let result = store
        .put_all(&[
            flexible("new:1", "n", Some(9090)),
            flexible("bad:1", "n", None),
        ])
        .await;
    assert!(result.is_err(), "invalid batch must be rejected");
    let found = store.find(&EntityQuery::<FlexibleEntity>::new()).await?;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].authority, "kept:1");
    Ok(())
}

#[tokio::test]
async fn memory_rejects_schema_mismatch() {
    assert!(MemoryEntityStore::<TestEntity>::new(mismatched_config()).is_err());
}

#[tokio::test]
async fn file_roundtrip_survives_restart() -> anyhow::Result<()> {
    let dir = test_support::temp_dir("arachnea-typed", "restart");
    {
        let store = FileEntityStore::<TestEntity>::new(dir.clone(), config())?;
        store.put(&entity("f:1", "a", 60)).await?;
        store.put(&entity("f:2", "b", 60)).await?;
        assert!(store.get(&"missing".to_string()).await?.is_none());
    }
    // A fresh instance reads the persisted document: entities survive a restart.
    let store = FileEntityStore::<TestEntity>::new(dir.clone(), config())?;
    assert_eq!(
        store.get(&"f:1".to_string()).await?.expect("entity").name,
        "a"
    );
    store.delete(&"f:1".to_string()).await?;
    assert!(store.get(&"f:1".to_string()).await?.is_none());
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[tokio::test]
async fn file_find_combines_predicates_with_and() -> anyhow::Result<()> {
    let dir = test_support::temp_dir("arachnea-typed", "and-query");
    let store = FileEntityStore::<TestEntity>::new(dir.clone(), config())?;
    let mut first = entity("f:1", "same", 60);
    first.port = 8080;
    let mut second = entity("f:2", "same", 60);
    second.port = 9090;
    store
        .put_all(&[first, second, entity("f:3", "other", 60)])
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
    assert_eq!(found[0].authority, "f:1");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[tokio::test]
async fn file_expired_entries_are_pruned_on_read() -> anyhow::Result<()> {
    let dir = test_support::temp_dir("arachnea-typed", "expiration");
    {
        let store = FileEntityStore::<TestEntity>::new(dir.clone(), config())?;
        store.put(&entity("stale:1", "n", 0)).await?;
        store.put(&entity("fresh:1", "n", 60)).await?;
        let found = store.find(&EntityQuery::<TestEntity>::new()).await?;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].authority, "fresh:1");
    }
    // The pruning is persisted: a restarted store no longer holds the stale entity.
    let store = FileEntityStore::<TestEntity>::new(dir.clone(), config())?;
    assert!(store.get(&"stale:1".to_string()).await?.is_none());
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[tokio::test]
async fn file_put_all_rejects_invalid_batch_without_writing() -> anyhow::Result<()> {
    let dir = test_support::temp_dir("arachnea-typed", "atomic");
    {
        let store = FileEntityStore::<FlexibleEntity>::new(dir.clone(), config())?;
        store.put(&flexible("kept:1", "n", Some(8080))).await?;
        let result = store
            .put_all(&[
                flexible("new:1", "n", Some(9090)),
                flexible("bad:1", "n", None),
            ])
            .await;
        assert!(result.is_err(), "invalid batch must be rejected");
    }
    // A fresh instance reads the last persisted snapshot: no partial write.
    let store = FileEntityStore::<FlexibleEntity>::new(dir.clone(), config())?;
    let found = store.find(&EntityQuery::<FlexibleEntity>::new()).await?;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].authority, "kept:1");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[tokio::test]
async fn file_rejects_schema_mismatch() {
    let dir = test_support::temp_dir("arachnea-typed", "mismatch");
    assert!(FileEntityStore::<TestEntity>::new(dir.clone(), mismatched_config()).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
