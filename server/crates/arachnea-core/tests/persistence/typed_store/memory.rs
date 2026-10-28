use super::super::test_support::*;
use super::*;

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
