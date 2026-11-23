//! Common and streaming table views on one physical profile database.

use super::{common::CommonRepository, streaming::StreamingRepository};
use anyhow::{Context, Result};
use arachnea_core::persistence::{
    PersistenceTableConfig, PersistentEntity, SqlKey, SqliteDatabase, SqliteEntityStore,
};
use std::{path::Path, sync::Arc};

/// Shared profile database handles. Create one owner per application instance.
///
/// Existing service/proxy/Cloudflare stores are never opened or changed here.
#[derive(Clone)]
pub struct ProfileStores {
    /// Common authoritative accounts, sessions and profile identities.
    pub common: Arc<CommonRepository>,
    /// Streaming documents; mutations must go through ProfileCoordinator.
    pub streaming: Arc<StreamingRepository>,
    /// Lifecycle guards shared even when coordinators reuse cloned stores.
    pub(super) guards: Arc<super::coordination::Guards>,
}

impl ProfileStores {
    /// Opens and initializes the profile database under the application data root.
    ///
    /// # Arguments
    /// * `application_path` - Directory containing the conventional `data/` root.
    ///
    /// # Errors
    /// Rejects the legacy two-file format and reports initialization errors.
    pub fn open(application_path: impl AsRef<Path>) -> Result<Self> {
        let root = application_path.as_ref().join(crate::PERSISTENCE_DATA_ROOT);
        let path = root.join(format!("{}.sqlite3", crate::PROFILES_STORE_NAME));
        if path.try_exists()? {
            let connection = rusqlite::Connection::open_with_flags(
                &path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .context("checking profile database format")?;
            let legacy: bool = connection.query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'profile_operations')",
                [],
                |row| row.get(0),
            )?;
            anyhow::ensure!(
                !legacy,
                "legacy profile database at {}; stop the application and move it aside or remove it before starting this version",
                path.display()
            );
        }
        let database = SqliteDatabase::open(
            arachnea_core::persistence::SqliteStoreConfig::new(crate::PROFILES_STORE_NAME)?,
            &root,
        )
        .context("opening profile database")?;
        Self::from_database(database)
    }

    /// Builds an isolated in-memory database with both domains' tables.
    ///
    /// # Errors
    /// Returns SQLite allocation/schema errors; no filesystem is accessed.
    pub fn memory() -> Result<Self> {
        Self::from_database(SqliteDatabase::memory()?)
    }

    /// Initializes both typed repositories on the same connection.
    ///
    /// # Arguments
    /// * `database` - Shared physical database for both domains.
    fn from_database(database: SqliteDatabase) -> Result<Self> {
        Ok(Self {
            common: Arc::new(CommonRepository::new(database.clone())?),
            streaming: Arc::new(StreamingRepository::new(database)?),
            guards: Arc::default(),
        })
    }
}

/// Opens one declared table through Core's existing schema reconciliation.
///
/// # Arguments
/// * `database` - Physical database owner.
/// * `name` - Stable table name, independent of the file name.
///
/// # Errors
/// Returns a contextual table/schema error without altering other stores.
pub(super) fn table<E: PersistentEntity>(
    database: &SqliteDatabase,
    name: &str,
) -> Result<SqliteEntityStore<E>>
where
    E::Key: SqlKey,
{
    database
        .table(PersistenceTableConfig::new(name, E::schema())?)
        .with_context(|| format!("initializing profile table {name}"))
}
