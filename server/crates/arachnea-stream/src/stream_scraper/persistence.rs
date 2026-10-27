//! Application store construction and service-default synchronization.

use super::*;

/// Opens the application SQLite stores under `application_data_path`.
///
/// Physical stores use `<root>/<store-name>.sqlite3`. Proxy inventory, failures
/// and preferences share one database and connection through typed table views.
/// Legacy directory-based databases are ignored, not imported or deleted.
/// Existing service activation overrides, credentials and cached data in those
/// legacy databases are not retained. New-format data survives later restarts.
///
/// # Arguments
/// * `application_data_path` - Application directory containing the `data` root.
///
/// # Errors
///
/// Returns an error when a database cannot be opened or its physical schema is
/// incompatible with the declared entity schema.
pub fn sqlite_application_stores(
    application_data_path: impl Into<PathBuf>,
) -> Result<ApplicationStores> {
    let root = application_data_path.into().join(PERSISTENCE_DATA_ROOT);
    let proxies = SqliteDatabase::open(SqliteStoreConfig::new(PROXY_STORE_NAME)?, &root)?;
    let sessions = SqliteDatabase::open(
        SqliteStoreConfig::new(CLOUDFLARE_SESSION_STORE_NAME)?,
        &root,
    )?;
    let services =
        SqliteDatabase::open(SqliteStoreConfig::new(STREAM_SERVICES_STORE_NAME)?, &root)?;
    Ok(ApplicationStores {
        proxy_failures: Arc::new(proxies.table(PersistenceTableConfig::new(
            arachnea_proxy::core::proxy_repository::PROXY_FAILURE_STORE_NAME,
            ProxyRecord::schema(),
        )?)?),
        proxy_preferences: Arc::new(proxies.table(PersistenceTableConfig::new(
            PROXY_PREFERENCE_STORE_NAME,
            ProxyDestinationPreference::schema(),
        )?)?),
        proxy_inventory: Arc::new(proxies.table(PersistenceTableConfig::new(
            PROXY_STORE_NAME,
            ProxyRecord::schema(),
        )?)?),
        cloudflare_session: Arc::new(sessions.table(PersistenceTableConfig::new(
            CLOUDFLARE_SESSION_STORE_NAME,
            CachedChaserSession::schema(),
        )?)?),
        source_enabled: Arc::new(services.table(PersistenceTableConfig::new(
            STREAM_SERVICES_STORE_NAME,
            SourceServiceRecord::schema(),
        )?)?),
    })
}

/// Builds the in-memory application stores used by compatibility constructors.
///
/// # Errors
///
/// Returns an error when a store configuration is invalid.
pub fn memory_application_stores() -> Result<ApplicationStores> {
    Ok(ApplicationStores {
        proxy_failures: arachnea_proxy::core::proxy_repository::memory_proxy_store()?,
        proxy_preferences:
            arachnea_proxy::core::proxy_preference_repository::memory_proxy_preference_store()?,
        proxy_inventory: Arc::new(MemoryEntityStore::new(PersistenceStoreConfig::new(
            "proxy-inventory",
            ProxyRecord::schema(),
        )?)?),
        cloudflare_session: Arc::new(MemoryEntityStore::new(PersistenceStoreConfig::new(
            CLOUDFLARE_SESSION_STORE_NAME,
            CachedChaserSession::schema(),
        )?)?),
        source_enabled: Arc::new(MemoryEntityStore::new(PersistenceStoreConfig::new(
            STREAM_SERVICES_STORE_NAME,
            SourceServiceRecord::schema(),
        )?)?),
    })
}

/// Creates missing persistent service states without changing existing choices.
pub(super) async fn synchronize_service_defaults_async(
    store: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
    sources: Vec<ScraperSourceDescriptor>,
) -> Result<()> {
    PersistenceSourceEnabled::with_typed_store(store)
        .register_defaults(&sources)
        .await
}

/// Blocking variant of [`synchronize_service_defaults_async`] for synchronous callers.
pub(super) fn synchronize_service_defaults(
    store: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
    sources: Vec<ScraperSourceDescriptor>,
) -> Result<()> {
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .map_err(anyhow::Error::from)?
            .block_on(synchronize_service_defaults_async(store, sources))
    })
    .join()
    .map_err(|_| anyhow::anyhow!("service-state synchronization thread panicked"))?
}
