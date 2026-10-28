use super::super::test_support::*;
use super::*;
use crate::persistence::test_support;

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
