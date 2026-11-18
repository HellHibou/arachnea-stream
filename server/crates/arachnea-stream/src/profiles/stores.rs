//! Two independent physical profile stores and their typed views.

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
    /// Opens and initializes the two new stores under the application data root.
    ///
    /// # Arguments
    /// * `application_path` - Directory containing the conventional `data/` root.
    ///
    /// # Errors
    /// Returns contextual database/schema/unique-index initialization errors.
    pub fn open(application_path: impl AsRef<Path>) -> Result<Self> {
        let root = application_path.as_ref().join(crate::PERSISTENCE_DATA_ROOT);
        let common = SqliteDatabase::open(
            arachnea_core::persistence::SqliteStoreConfig::new(crate::PROFILES_STORE_NAME)?,
            &root,
        )
        .context("opening common profiles database")?;
        let streaming = SqliteDatabase::open(
            arachnea_core::persistence::SqliteStoreConfig::new(crate::STREAM_PROFILES_STORE_NAME)?,
            &root,
        )
        .context("opening streaming profiles database")?;
        Self::from_databases(common, streaming)
    }

    /// Builds isolated in-memory stores with identical transaction semantics.
    ///
    /// # Errors
    /// Returns SQLite allocation/schema errors; no filesystem is accessed.
    pub fn memory() -> Result<Self> {
        Self::from_databases(SqliteDatabase::memory()?, SqliteDatabase::memory()?)
    }

    /// Initializes independent typed views without writing business entities.
    ///
    /// # Arguments
    /// * `common` - Common domain database.
    /// * `streaming` - Separate service domain database.
    fn from_databases(common: SqliteDatabase, streaming: SqliteDatabase) -> Result<Self> {
        Ok(Self {
            common: Arc::new(CommonRepository::new(common)?),
            streaming: Arc::new(StreamingRepository::new(streaming)?),
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
