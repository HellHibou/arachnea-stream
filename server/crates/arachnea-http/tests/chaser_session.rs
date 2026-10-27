use super::*;
use arachnea_core::persistence::{EntityQuery, FileEntityStore, SqliteEntityStore};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

fn temp_dir(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "arachnea-session-{}-{}-{}",
        label,
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn store_config() -> PersistenceStoreConfig {
    PersistenceStoreConfig::new(CLOUDFLARE_SESSION_STORE_NAME, CachedChaserSession::schema())
        .expect("valid config")
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX_EPOCH")
        .as_secs()
}

fn sample(origin: &str, clearance: Option<u64>, stored_at: u64) -> CachedChaserSession {
    CachedChaserSession {
        origin: origin.to_string(),
        cookies: vec![StructuredCookie {
            name: "cf_clearance".into(),
            value: "secret".into(),
            domain: Some(".example.com".into()),
            path: Some("/".into()),
            expires: Some(1234.5),
            http_only: Some(true),
            secure: Some(true),
            same_site: Some("Lax".into()),
        }],
        user_agent: Some("ua".into()),
        clearance_expires_at: clearance,
        stored_at,
    }
}

async fn assert_full_roundtrip(
    store: &dyn TypedEntityStore<CachedChaserSession>,
    origin: &str,
) -> anyhow::Result<()> {
    let stored_at = unix_now();
    store
        .put(&sample(origin, Some(stored_at + 3_600), stored_at))
        .await?;
    let loaded = store
        .get(&origin.to_string())
        .await?
        .expect("session must survive the roundtrip");
    assert_eq!(loaded.origin, origin);
    assert_eq!(loaded.user_agent.as_deref(), Some("ua"));
    assert_eq!(loaded.clearance_expires_at, Some(stored_at + 3_600));
    assert_eq!(loaded.cookies.len(), 1);
    assert_eq!(loaded.cookies[0].name, "cf_clearance");
    assert_eq!(loaded.cookies[0].domain.as_deref(), Some(".example.com"));
    assert_eq!(loaded.cookies[0].path.as_deref(), Some("/"));
    assert_eq!(loaded.cookies[0].expires, Some(1234.5));
    assert_eq!(loaded.cookies[0].http_only, Some(true));
    assert_eq!(loaded.cookies[0].secure, Some(true));
    assert_eq!(loaded.cookies[0].same_site.as_deref(), Some("Lax"));

    // Optional fields stay absent through the roundtrip.
    let mut minimal = sample(origin, None, stored_at);
    minimal.user_agent = None;
    store.put(&minimal).await?;
    let loaded = store
        .get(&origin.to_string())
        .await?
        .expect("minimal session must survive the roundtrip");
    assert_eq!(loaded.user_agent, None);
    assert_eq!(loaded.clearance_expires_at, None);
    assert_eq!(loaded.cookies.len(), 1);
    Ok(())
}

/// Item 1: the scalar `origin` key roundtrips in the memory backend.
#[tokio::test]
async fn memory_roundtrip_by_origin() -> anyhow::Result<()> {
    let store = MemoryEntityStore::<CachedChaserSession>::new(store_config())?;
    assert_full_roundtrip(&store, "https://memory.example.com").await?;
    store
        .delete(&"https://memory.example.com".to_string())
        .await?;
    assert!(store
        .get(&"https://memory.example.com".to_string())
        .await?
        .is_none());
    Ok(())
}

/// Item 1: the scalar `origin` key roundtrips in the file backend and
/// survives a reopen.
#[tokio::test]
async fn file_roundtrip_survives_reopen() -> anyhow::Result<()> {
    let dir = temp_dir("file");
    let stored_at = unix_now();
    let config = store_config();
    {
        let store = FileEntityStore::<CachedChaserSession>::new(&dir, config.clone())?;
        store
            .put(&sample(
                "https://file.example.com",
                Some(stored_at + 3_600),
                stored_at,
            ))
            .await?;
    }
    let store = FileEntityStore::<CachedChaserSession>::new(&dir, config)?;
    let loaded = store
        .get(&"https://file.example.com".to_string())
        .await?
        .expect("file session must survive the reopen");
    assert_eq!(loaded.origin, "https://file.example.com");
    store
        .delete(&"https://file.example.com".to_string())
        .await?;
    assert!(store
        .get(&"https://file.example.com".to_string())
        .await?
        .is_none());
    Ok(())
}

/// Item 2: flattened optional fields roundtrip through SQLite.
#[tokio::test]
async fn sqlite_optional_fields_roundtrip() -> anyhow::Result<()> {
    let root = temp_dir("sqlite-fields");
    let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
    assert_full_roundtrip(&store, "https://sqlite.example.com").await?;
    Ok(())
}

/// Item 4: SQLite reopen keeps the sessions, the origin lookup works, and
/// an invalid batch writes nothing.
#[tokio::test]
async fn sqlite_reopen_batch_and_origin_lookup() -> anyhow::Result<()> {
    let root = temp_dir("sqlite-reopen");
    let stored_at = unix_now();
    let first = sample(
        "https://first.example.com",
        Some(stored_at + 3_600),
        stored_at,
    );
    let second = sample("https://second.example.com", None, stored_at);
    {
        let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
        store.put_all(&[first.clone(), second.clone()]).await?;
    }
    let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
    assert_eq!(
        store
            .get(&"https://first.example.com".to_string())
            .await?
            .expect("first session"),
        first
    );
    assert_eq!(
        store
            .get(&"https://second.example.com".to_string())
            .await?
            .expect("second session"),
        second
    );
    // Origin lookup through a query predicate.
    let schema = CachedChaserSession::schema();
    let found = store
        .find(&EntityQuery::<CachedChaserSession>::new().where_string(
            schema.field_named("origin").expect("origin field"),
            "https://first.example.com",
        ))
        .await?;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].origin, "https://first.example.com");

    // A batch carrying an invalid entity is rejected atomically.
    let mut invalid = sample("https://third.example.com", None, u64::MAX);
    invalid.user_agent = None;
    assert!(store.put_all(&[invalid, second]).await.is_err());
    assert!(
        store
            .get(&"https://third.example.com".to_string())
            .await?
            .is_none(),
        "invalid batch must write nothing"
    );
    Ok(())
}

/// Item 3: expiration honors the `cf_clearance` deadline and the fallback
/// TTL when no clearance expiration is available.
#[tokio::test]
async fn expiration_deadline_and_fallback_ttl() -> anyhow::Result<()> {
    let root = temp_dir("sqlite-expiry");
    let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
    let now = unix_now();

    // Live `cf_clearance` deadline keeps the session readable.
    store
        .put(&sample("https://live.example.com", Some(now + 3_600), now))
        .await?;
    assert!(store
        .get(&"https://live.example.com".to_string())
        .await?
        .is_some());

    // Without a clearance expiration the 1_800s fallback TTL applies.
    store
        .put(&sample("https://stale.example.com", None, now - 3_600))
        .await?;
    assert!(
        store
            .get(&"https://stale.example.com".to_string())
            .await?
            .is_none(),
        "fallback TTL must expire the session"
    );

    store
        .put(&sample("https://fresh.example.com", None, now - 60))
        .await?;
    assert!(store
        .get(&"https://fresh.example.com".to_string())
        .await?
        .is_some());
    Ok(())
}

/// Item 5: a legacy `session TEXT NOT NULL` cache is strictly rejected and
/// a manual deletion of the closed database allows opening a fresh store.
#[tokio::test]
async fn legacy_cache_rejected_until_manual_deletion() -> anyhow::Result<()> {
    let root = temp_dir("sqlite-legacy");
    let database_path = root.join("cloudflare-session.sqlite3");
    // Establish the metadata layout with a fresh flattened store first.
    drop(SqliteEntityStore::<CachedChaserSession>::new(
        store_config(),
        &root,
    )?);

    // Reproduce legacy entity columns using current table-scoped metadata.
    let connection = rusqlite::Connection::open(&database_path)?;
    connection.execute("DROP TABLE \"cloudflare-session\"", [])?;
    connection.execute(
        "CREATE TABLE \"cloudflare-session\" (\
         \"origin\" TEXT NOT NULL, \
         \"expires_at\" INTEGER NOT NULL, \
         \"session\" TEXT NOT NULL, \
         PRIMARY KEY (\"origin\")) STRICT",
        [],
    )?;
    connection.execute("DELETE FROM arachnea_columns", [])?;
    connection.execute("DELETE FROM arachnea_indexes", [])?;
    for (name, logical_type) in [
        ("origin", "string"),
        ("expires_at", "date_time"),
        ("session", "string"),
    ] {
        connection.execute(
            "INSERT INTO arachnea_columns (table_name, name, logical_type) VALUES ('cloudflare-session', ?1, ?2)",
            rusqlite::params![name, logical_type],
        )?;
    }
    let legacy_marker = "legacy-cookie-value";
    let expires_at_nanos = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX_EPOCH")
        .as_nanos()
        + 3_600_000_000_000) as i64;
    connection.execute(
        "INSERT INTO \"cloudflare-session\" (\"origin\", \"expires_at\", \"session\") \
         VALUES (?1, ?2, ?3)",
        rusqlite::params![
            "https://legacy.example.com",
            expires_at_nanos,
            format!("{legacy_marker}")
        ],
    )?;
    drop(connection);

    // Reopening succeeds (reconciliation only adds nullable columns), but
    // the legacy cache is strictly rejected: reads and writes fail.
    let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
    let read_error = store
        .get(&"https://legacy.example.com".to_string())
        .await
        .expect_err("legacy row must fail to decode");
    let write_error = store
        .put(&sample("https://legacy.example.com", None, unix_now()))
        .await
        .expect_err("legacy table must reject flattened inserts");
    let write_error = write_error.to_string();
    assert!(
        write_error.contains("NOT NULL constraint failed"),
        "unexpected write error: {write_error}"
    );
    for error in [read_error.to_string(), write_error] {
        assert!(
            !error.contains(legacy_marker) && !error.contains("secret"),
            "error text must never contain cookie values: {error}"
        );
    }
    drop(store);

    // Closing the last connection checkpoints the WAL before removing the database.
    std::fs::remove_file(&database_path)?;
    let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
    let stored_at = unix_now();
    store
        .put(&sample(
            "https://legacy.example.com",
            Some(stored_at + 3_600),
            stored_at,
        ))
        .await?;
    assert!(store
        .get(&"https://legacy.example.com".to_string())
        .await?
        .is_some());
    Ok(())
}

/// Item 2: corrupted persisted fields fail with contextualized errors that
/// exclude the corrupted payload and cookie values.
#[tokio::test]
async fn corrupted_fields_fail_without_leaking_values() -> anyhow::Result<()> {
    let root = temp_dir("sqlite-corrupt");
    let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
    let db = root.join("cloudflare-session.sqlite3");
    let origin = "https://corrupt.example.com";
    store.put(&sample(origin, None, unix_now())).await?;
    drop(store);

    let connection = rusqlite::Connection::open(&db)?;
    // Valid JSON with the wrong shape reaches `read_from`, which
    // contextualizes the failure without the payload.
    connection.execute(
        "UPDATE \"cloudflare-session\" SET \"cookies\" = ?1 WHERE \"origin\" = ?2",
        rusqlite::params!["{\"unexpected\": true}", origin],
    )?;
    drop(connection);

    let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
    let error = store
        .get(&origin.to_string())
        .await
        .expect_err("corrupted cookies must fail to decode");
    let error = error.to_string();
    assert!(
        !error.contains("legacy-cookie-value") && !error.contains("secret"),
        "error text must never contain cookie values: {error}"
    );
    assert!(
        error.contains("malformed cookie data"),
        "unexpected error: {error}"
    );
    drop(store);

    // Invalid JSON fails earlier, in the SQLite layer, with an error that
    // still excludes the stored value.
    let connection = rusqlite::Connection::open(&db)?;
    connection.execute(
        "UPDATE \"cloudflare-session\" SET \"cookies\" = ?1 WHERE \"origin\" = ?2",
        rusqlite::params!["legacy-cookie-value", origin],
    )?;
    drop(connection);

    let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
    let error = store
        .get(&origin.to_string())
        .await
        .expect_err("invalid cookie JSON must fail to decode");
    let error = error.to_string();
    assert!(
        !error.contains("legacy-cookie-value") && !error.contains("secret"),
        "error text must never contain cookie values: {error}"
    );
    drop(store);

    // An out-of-range clearance expiration is rejected as well.
    let connection = rusqlite::Connection::open(&db)?;
    connection.execute(
        "UPDATE \"cloudflare-session\" SET \"cookies\" = ?1, \"clearance_expires_at\" = -5 \
         WHERE \"origin\" = ?2",
        rusqlite::params!["[]", origin],
    )?;
    drop(connection);
    let store = SqliteEntityStore::<CachedChaserSession>::new(store_config(), &root)?;
    let error = store
        .get(&origin.to_string())
        .await
        .expect_err("out-of-range clearance must fail");
    let error = error.to_string();
    assert!(!error.contains("-5"), "error leaked an integer: {error}");
    assert!(
        error.contains("out-of-range clearance expiration"),
        "unexpected error: {error}"
    );
    Ok(())
}
