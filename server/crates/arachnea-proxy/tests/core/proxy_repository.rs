use super::*;
use arachnea_core::persistence::{
    FileEntityStore, SqliteDatabase, SqliteEntityStore, SqliteStoreConfig,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

fn temp_dir(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let directory = std::env::temp_dir().join(format!(
        "arachnea-proxy-persistence-{label}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&directory);
    directory
}

fn record(host: &str, port: u16, country: Option<&str>) -> ProxyRecord {
    let destination_failure_time = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| std::time::UNIX_EPOCH + Duration::from_millis(duration.as_millis() as u64))
        .expect("system time after UNIX_EPOCH");
    ProxyRecord {
        health_evidence: Default::default(),
        observations: Vec::new(),
        probe_attempts: std::collections::HashMap::from([("https".to_string(), 1791540000000)]),
        tcp_reachable: Some(true),
        protocol: Some(ProxyProtocol::Socks5),
        host: host.to_string(),
        port,
        country: country.map(str::to_string),
        supports_https: Some(true),
        declarations: vec![ProxyDeclaration {
            source: "test-provider".to_string(),
            protocol: Some(ProxyProtocol::Socks5),
            supports_https: Some(true),
        }],
        status: ProxyRuntimeStatus::Ok,
        http_forwarding: ProxyCapabilityStatus::Available,
        https_tunnel: ProxyCapabilityStatus::Available,
        destination_tls: ProxyCapabilityStatus::Unknown,
        proxy_tls_certificate: ProxyCapabilityStatus::Unknown,
        latency_ms: Some(42),
        failure_count: 0,
        authentication_required: Some(false),
        availability: ProxyAvailabilityHint::High,
        destination_failures: vec![ProxyDestinationFailure {
            scheme: "https".to_string(),
            host: "origin.example".to_string(),
            port: 443,
            reason: crate::core::ProxyDestinationFailureReason::Timeout,
            failure_count: 1,
            last_failed: destination_failure_time,
            cooldown_until: None,
        }],
        last_checked: Some(SystemTime::now()),
        last_validated_at: Some(SystemTime::now()),
        cooldown_until: None,
    }
}

fn config() -> PersistenceStoreConfig {
    PersistenceStoreConfig::new(PROXY_STORE_NAME, ProxyRecord::schema()).expect("valid config")
}

#[tokio::test]
async fn composite_proxy_key_roundtrips_on_all_backends() -> Result<()> {
    let proxy = record("2001:db8::1", 1080, Some("BE"));
    let key = proxy.key();

    let memory = MemoryEntityStore::<ProxyRecord>::new(config())?;
    memory.put(&proxy).await?;
    assert_eq!(memory.get(&key).await?, Some(proxy.clone()));

    let file_root = temp_dir("file-roundtrip");
    let file = FileEntityStore::<ProxyRecord>::new(file_root.clone(), config())?;
    file.put(&proxy).await?;
    assert_eq!(file.get(&key).await?, Some(proxy.clone()));

    let sqlite_root = temp_dir("sqlite-roundtrip");
    {
        let sqlite = SqliteEntityStore::<ProxyRecord>::new(config(), &sqlite_root)?;
        sqlite.put(&proxy).await?;
        assert_eq!(sqlite.get(&key).await?, Some(proxy.clone()));
        assert_eq!(
            sqlite
                .find(&EntityQuery::new().where_string(&ProxyRecord::country_field(), "BE"))
                .await?,
            vec![proxy.clone()]
        );
    }
    let reopened = SqliteEntityStore::<ProxyRecord>::new(config(), &sqlite_root)?;
    assert_eq!(reopened.get(&key).await?, Some(proxy));

    let preference_config = PersistenceStoreConfig::new(
        crate::core::PROXY_PREFERENCE_STORE_NAME,
        crate::core::ProxyDestinationPreference::schema(),
    )?;
    let destination = crate::core::Destination::host_port("ORIGIN.example", 443)
        .with_protocol(crate::core::ApplicationProtocol::Https);
    let mut preference = crate::core::ProxyDestinationPreference {
        destination: crate::core::ProxyDestinationPreference::destination_key(&destination),
        endpoints: Vec::new(),
    };
    let now = SystemTime::now();
    preference.record_success(key.clone(), now);
    for index in 0..4 {
        preference.record_success(
            ProxyKey {
                host: format!("accepted-{index}.example"),
                port: 8080,
            },
            now + Duration::from_secs(index),
        );
    }
    assert_eq!(preference.endpoints.len(), 3);
    let preference_memory = MemoryEntityStore::new(preference_config.clone())?;
    preference_memory.put(&preference).await?;
    assert_eq!(
        preference_memory.get(&preference.destination).await?,
        Some(preference.clone())
    );
    let preference_file = FileEntityStore::new(file_root.clone(), preference_config.clone())?;
    preference_file.put(&preference).await?;
    assert_eq!(
        preference_file.get(&preference.destination).await?,
        Some(preference.clone())
    );
    {
        let database =
            SqliteDatabase::open(SqliteStoreConfig::new(PROXY_STORE_NAME)?, &sqlite_root)?;
        let preferences = Arc::new(database.table(preference_config.clone())?);
        let repository = crate::core::TypedProxyPreferenceRepository::new(preferences);
        crate::core::ProxyPreferenceRepository::save(&repository, &preference).await?;
        let mut older = preference.clone();
        older.endpoints[0].accepted_at = now - Duration::from_secs(60);
        crate::core::ProxyPreferenceRepository::save(&repository, &older).await?;
    }
    let database = SqliteDatabase::open(SqliteStoreConfig::new(PROXY_STORE_NAME)?, &sqlite_root)?;
    let reopened_preferences =
        database.table::<crate::core::ProxyDestinationPreference>(preference_config)?;
    assert_eq!(
        reopened_preferences.get(&preference.destination).await?,
        Some(preference)
    );

    let _ = std::fs::remove_dir_all(&file_root);
    let _ = std::fs::remove_dir_all(&sqlite_root);
    Ok(())
}

#[tokio::test]
async fn composite_sqlite_batch_is_atomic_and_stale_records_remain_available() -> Result<()> {
    let root = temp_dir("atomic-stale");
    let database = SqliteDatabase::open(SqliteStoreConfig::new(PROXY_STORE_NAME)?, &root)?;
    let store = database.table::<ProxyRecord>(config())?;
    let kept = record("kept.example", 8080, Some("BE"));
    store.put(&kept).await?;

    let mut invalid = record("invalid.example", 8081, Some("BE"));
    invalid.latency_ms = Some(u64::MAX);
    assert!(store
        .put_all(&[record("new.example", 8082, Some("BE")), invalid])
        .await
        .is_err());
    assert_eq!(store.get(&kept.key()).await?, Some(kept));

    let mut stale = record("stale.example", 8083, Some("BE"));
    stale.last_validated_at = Some(SystemTime::now() - PROXY_CACHE_TTL - Duration::from_secs(1));
    store.put(&stale).await?;
    assert_eq!(store.get(&stale.key()).await?, Some(stale.clone()));
    stale.health_evidence.last_positive_at = stale.last_validated_at;
    stale.health_evidence.first_discovered_at = stale.last_validated_at;
    let store = Arc::new(store);
    let failures = Arc::new(database.table(PersistenceStoreConfig::new(
        PROXY_FAILURE_STORE_NAME,
        ProxyRecord::schema(),
    )?)?);
    let repository = Arc::new(TypedProxyRepository::with_failure_store(
        store.clone(),
        failures.clone(),
    ));
    let inventory =
        crate::core::ProxyInventory::new(crate::core::InventoryConfig::default(), None, None)
            .with_proxy_repository(repository.clone());
    inventory.add_or_update(vec![stale.clone()]).await;
    inventory.record_global_failure(&stale.authority()).await;
    assert!(store.get(&stale.key()).await?.is_none());
    assert!(repository.get_failure(&stale.key()).await?.is_some());
    assert!(inventory.is_empty().await);
    let restored =
        crate::core::ProxyInventory::new(crate::core::InventoryConfig::default(), None, None)
            .with_proxy_repository(Arc::new(TypedProxyRepository::with_failure_store(
                store, failures,
            )));
    restored
        .add_or_update(vec![record("stale.example", 8083, Some("BE"))])
        .await;
    assert!(restored.is_empty().await);

    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn legacy_authority_key_sqlite_cache_is_rejected() -> Result<()> {
    let root = temp_dir("legacy-cache");
    std::fs::create_dir_all(&root)?;
    let database_path = root.join(format!("{PROXY_STORE_NAME}.sqlite3"));
    let connection = rusqlite::Connection::open(&database_path)?;
    connection.execute_batch(
        "CREATE TABLE \"proxy-inventory\" (\
                authority TEXT PRIMARY KEY, \
                country TEXT, \
                expires_at INTEGER NOT NULL, \
                record TEXT NOT NULL\
            ) STRICT;",
    )?;
    drop(connection);

    let error = SqliteEntityStore::<ProxyRecord>::new(config(), &root)
        .err()
        .expect("legacy authority key must be rejected");
    assert!(error.to_string().contains("primary key"));

    std::fs::remove_file(&database_path)?;
    assert!(SqliteEntityStore::<ProxyRecord>::new(config(), &root).is_ok());

    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[tokio::test]
async fn interrupted_eviction_hides_available_record_and_orphan_preference() -> Result<()> {
    let root = temp_dir("interrupted-eviction");
    let database = SqliteDatabase::open(SqliteStoreConfig::new(PROXY_STORE_NAME)?, &root)?;
    let available = Arc::new(database.table::<ProxyRecord>(config())?);
    let failures = Arc::new(database.table::<ProxyRecord>(PersistenceStoreConfig::new(
        PROXY_FAILURE_STORE_NAME,
        ProxyRecord::schema(),
    )?)?);
    let preferences = Arc::new(database.table::<crate::core::ProxyDestinationPreference>(
        PersistenceStoreConfig::new(
            crate::core::PROXY_PREFERENCE_STORE_NAME,
            crate::core::ProxyDestinationPreference::schema(),
        )?,
    )?);
    let mut proxy = record("interrupted.example", 8080, Some("BE"));
    proxy.health_evidence.first_discovered_at =
        Some(SystemTime::now() - PROXY_CACHE_TTL - Duration::from_secs(60));
    proxy.health_evidence.last_positive_at = proxy.health_evidence.first_discovered_at;
    proxy.last_checked = proxy.health_evidence.last_positive_at;
    available.put(&proxy).await?;
    let destination = crate::core::Destination::host_port("origin.example", 443)
        .with_protocol(crate::core::ApplicationProtocol::Https);
    let mut preference = crate::core::ProxyDestinationPreference {
        destination: crate::core::ProxyDestinationPreference::destination_key(&destination),
        endpoints: Vec::new(),
    };
    preference.record_success(proxy.key(), proxy.health_evidence.last_positive_at.unwrap());
    preferences.put(&preference).await?;
    let mut tombstone = proxy.clone();
    tombstone.health_evidence.evicted_at = Some(SystemTime::now());
    tombstone.cooldown_until = Some(SystemTime::now() + Duration::from_secs(900));
    tombstone.status = ProxyRuntimeStatus::Ko;
    // Crash-equivalent state after tombstone commit, before available/history deletion.
    failures.put(&tombstone).await?;
    drop((available, failures, preferences, database));
    let database = SqliteDatabase::open(SqliteStoreConfig::new(PROXY_STORE_NAME)?, &root)?;
    let available = Arc::new(database.table::<ProxyRecord>(config())?);
    let failures = Arc::new(database.table::<ProxyRecord>(PersistenceStoreConfig::new(
        PROXY_FAILURE_STORE_NAME,
        ProxyRecord::schema(),
    )?)?);
    let preferences = Arc::new(crate::core::TypedProxyPreferenceRepository::new(Arc::new(
        database.table::<crate::core::ProxyDestinationPreference>(PersistenceStoreConfig::new(
            crate::core::PROXY_PREFERENCE_STORE_NAME,
            crate::core::ProxyDestinationPreference::schema(),
        )?)?,
    )));
    let repository = Arc::new(TypedProxyRepository::with_failure_store(
        available.clone(),
        failures,
    ));
    assert!(available.get(&proxy.key()).await?.is_some());
    assert!(repository.get(&proxy.key()).await?.is_none());
    let inventory =
        crate::core::ProxyInventory::new(crate::core::InventoryConfig::default(), None, None)
            .with_proxy_repository(repository.clone())
            .with_preference_repository(preferences);
    assert!(inventory
        .select_for_destination("BE", true, Some(&destination))
        .await
        .is_err());
    inventory.add_or_update(vec![proxy.clone()]).await;
    assert!(inventory.is_empty().await);
    repository.evict(&tombstone).await?;
    assert!(available.get(&proxy.key()).await?.is_none());
    drop((inventory, repository, available, database));
    std::fs::remove_dir_all(root)?;
    Ok(())
}

#[tokio::test]
async fn failure_retention_expires_after_48_hours_and_prunes_on_demand() -> Result<()> {
    let available = memory_proxy_store()?;
    let failures = memory_proxy_store()?;
    let repository = TypedProxyRepository::with_failure_store(available, failures.clone());
    let mut expired = record("expired.example", 8080, Some("BE"));
    expired.health_evidence.evicted_at = Some(SystemTime::now() - Duration::from_secs(49 * 3600));
    failures.put(&expired).await?;
    assert!(repository.get_failure(&expired.key()).await?.is_none());
    assert!(failures.get(&expired.key()).await?.is_some());
    let mut current = record("current.example", 8081, Some("BE"));
    current.health_evidence.evicted_at = Some(SystemTime::now());
    repository.evict(&current).await?;
    assert!(failures.get(&expired.key()).await?.is_none());
    assert!(repository.get_failure(&current.key()).await?.is_some());
    Ok(())
}

#[tokio::test]
async fn eviction_recovery_handles_remaining_inter_table_commit_boundaries() -> Result<()> {
    use crate::core::ProxyPreferenceRepository;
    for stage in 0..4 {
        let root = temp_dir(&format!("eviction-boundary-{stage}"));
        let database = SqliteDatabase::open(SqliteStoreConfig::new(PROXY_STORE_NAME)?, &root)?;
        let available = database.table::<ProxyRecord>(config())?;
        let failure_config =
            PersistenceStoreConfig::new(PROXY_FAILURE_STORE_NAME, ProxyRecord::schema())?;
        let preference_config = PersistenceStoreConfig::new(
            crate::core::PROXY_PREFERENCE_STORE_NAME,
            crate::core::ProxyDestinationPreference::schema(),
        )?;
        let failures = database.table::<ProxyRecord>(failure_config.clone())?;
        let preferences =
            database.table::<crate::core::ProxyDestinationPreference>(preference_config.clone())?;
        assert!(root.join("proxy-inventory.sqlite3").is_file());
        assert!(!root.join("proxy-failures.sqlite3").exists());
        assert!(!root.join("proxy-preferences.sqlite3").exists());
        let mut proxy = record("boundary.example", 8080, Some("BE"));
        proxy.health_evidence.first_discovered_at =
            Some(SystemTime::now() - Duration::from_secs(25 * 3600));
        proxy.health_evidence.last_positive_at = proxy.health_evidence.first_discovered_at;
        proxy.last_checked = proxy.health_evidence.last_positive_at;
        let mut preference = crate::core::ProxyDestinationPreference {
            destination: "https://origin.example:443".into(),
            endpoints: Vec::new(),
        };
        preference.record_success(proxy.key(), proxy.health_evidence.last_positive_at.unwrap());
        available.put(&proxy).await?;
        preferences.put(&preference).await?;
        let mut tombstone = proxy.clone();
        tombstone.health_evidence.evicted_at = Some(SystemTime::now());
        tombstone.cooldown_until = Some(SystemTime::now() + Duration::from_secs(900));
        tombstone.status = ProxyRuntimeStatus::Ko;
        if stage >= 1 {
            failures.put(&tombstone).await?;
        }
        if stage >= 2 {
            available.delete(&proxy.key()).await?;
        }
        if stage >= 3 {
            preferences.delete(&preference.destination).await?;
        }
        drop((available, failures, preferences, database));
        let database = SqliteDatabase::open(SqliteStoreConfig::new(PROXY_STORE_NAME)?, &root)?;
        let available = Arc::new(database.table::<ProxyRecord>(config())?);
        let failures = Arc::new(database.table::<ProxyRecord>(failure_config)?);
        let preference_store =
            Arc::new(database.table::<crate::core::ProxyDestinationPreference>(preference_config)?);
        let repository = TypedProxyRepository::with_failure_store(available.clone(), failures);
        let histories = crate::core::TypedProxyPreferenceRepository::new(preference_store);
        assert_eq!(repository.get(&proxy.key()).await?.is_some(), stage == 0);
        assert_eq!(
            histories.get(&preference.destination).await?.is_some(),
            stage < 3
        );
        // An interruption before the first durable write cannot preserve eviction.
        // Replay the observed failure/cleanup at demand and verify idempotence.
        repository.evict(&tombstone).await?;
        histories.remove_endpoint(&proxy.key()).await?;
        repository.evict(&tombstone).await?;
        histories.remove_endpoint(&proxy.key()).await?;
        assert!(repository.get(&proxy.key()).await?.is_none());
        assert!(histories.get(&preference.destination).await?.is_none());
        drop((repository, histories, available, database));
        std::fs::remove_dir_all(root)?;
    }
    Ok(())
}

#[tokio::test]
async fn concurrent_file_history_commits_survive_reopen_across_destinations() -> Result<()> {
    use crate::core::ProxyPreferenceRepository;
    let root = temp_dir("concurrent-preferences");
    let config = PersistenceStoreConfig::new(
        crate::core::PROXY_PREFERENCE_STORE_NAME,
        crate::core::ProxyDestinationPreference::schema(),
    )?;
    let store = Arc::new(
        FileEntityStore::<crate::core::ProxyDestinationPreference>::new(
            root.clone(),
            config.clone(),
        )?,
    );
    let first = Arc::new(crate::core::TypedProxyPreferenceRepository::new(
        store.clone(),
    ));
    let second = Arc::new(crate::core::TypedProxyPreferenceRepository::new(
        store.clone(),
    ));
    let now = std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let writes_started = std::time::Instant::now();
    let mut tasks = Vec::new();
    for index in 0..20 {
        let repository = if index % 2 == 0 {
            first.clone()
        } else {
            second.clone()
        };
        tasks.push(tokio::spawn(async move {
            let mut history = crate::core::ProxyDestinationPreference {
                destination: format!("https://origin-{}.example:443", index % 4),
                endpoints: Vec::new(),
            };
            history.record_success(
                crate::core::ProxyKey {
                    host: format!("proxy-{index}.example"),
                    port: 8080,
                },
                now + Duration::from_secs(index),
            );
            repository.save(&history).await
        }));
    }
    for task in tasks {
        task.await??;
    }
    let writes_us = writes_started.elapsed().as_micros();
    drop((first, second, store));
    let reopen_started = std::time::Instant::now();
    let reopened =
        FileEntityStore::<crate::core::ProxyDestinationPreference>::new(root.clone(), config)?;
    let histories = reopened.find(&EntityQuery::new()).await?;
    eprintln!("lot7 durable histories: commits=20, destinations={}, writes_us={writes_us}, reopen_and_read_us={}", histories.len(), reopen_started.elapsed().as_micros());
    assert_eq!(histories.len(), 4);
    for history in histories {
        assert_eq!(history.endpoints.len(), 3);
        assert!(history
            .endpoints
            .windows(2)
            .all(|pair| pair[0].accepted_at > pair[1].accepted_at));
        let group: u64 = history
            .destination
            .strip_prefix("https://origin-")
            .unwrap()
            .split('.')
            .next()
            .unwrap()
            .parse()?;
        assert_eq!(
            history.endpoints[0].accepted_at,
            now + Duration::from_secs(group + 16)
        );
    }
    std::fs::remove_dir_all(root)?;
    Ok(())
}
