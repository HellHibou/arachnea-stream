//! Application store construction.

use super::*;

/// Opens the application SQLite stores under `application_data_path`.
///
/// Physical stores use `<root>/<store-name>.sqlite3`. Proxy inventory, failures
/// and preferences share one database and connection through typed table views.
/// The parameter table requires one row per parameter; historical JSON tables
/// with a different primary key are rejected without data conversion.
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
    let subtitle_translations = SqliteDatabase::open(
        SqliteStoreConfig::new(crate::SUBTITLE_TRANSLATION_STORE_NAME)?,
        &root,
    )?;
    Ok(ApplicationStores {
        service_parameters:
            arachnea_scrapyfy::scrapyfy::service_parameters_store::sqlite_parameters_store(
                &services,
            )?,
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
        source_enabled: arachnea_scrapyfy::scrapyfy::source_enabled::sqlite_source_store(
            &services,
        )?,
        subtitle_translations: Arc::new(subtitle_translations.table(
            PersistenceTableConfig::new(
                crate::SUBTITLE_TRANSLATION_STORE_NAME,
                crate::subtitles::SubtitleTranslationDescriptor::schema(),
            )?,
        )?),
    })
}

/// Builds the in-memory application stores used by compatibility constructors.
///
/// # Errors
///
/// Returns an error when a store configuration is invalid.
pub fn memory_application_stores() -> Result<ApplicationStores> {
    Ok(ApplicationStores {
        service_parameters:
            arachnea_scrapyfy::scrapyfy::service_parameters_store::memory_parameters_store(),
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
        source_enabled: arachnea_scrapyfy::scrapyfy::source_enabled::memory_source_store()?,
        subtitle_translations: Arc::new(MemoryEntityStore::new(PersistenceStoreConfig::new(
            crate::SUBTITLE_TRANSLATION_STORE_NAME,
            crate::subtitles::SubtitleTranslationDescriptor::schema(),
        )?)?),
    })
}
