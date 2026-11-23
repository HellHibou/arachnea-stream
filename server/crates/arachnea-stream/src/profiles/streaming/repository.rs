//! Service-local tables; no authority to create common profile identities.

use super::models::*;
use crate::profiles::{by_id, stores::table};
use anyhow::Result;
use arachnea_core::persistence::{SqliteDatabase, SqliteEntityStore, SqliteTransaction};
use uuid::Uuid;

/// Streaming table views, privately mutated under the coordinator's guards.
pub struct StreamingRepository {
    /// Collection revisions and deleted-record versions.
    pub(crate) collections: SqliteEntityStore<super::state::Collection>,
    /// Shared profile database.
    pub(crate) database: SqliteDatabase,
    /// Versioned streaming settings.
    pub(crate) settings: SqliteEntityStore<StreamSettings>,
    /// Custom streaming home definitions.
    pub(crate) homes: SqliteEntityStore<Home>,
    /// Ordered section preferences.
    pub(crate) sections: SqliteEntityStore<HomeSection>,
    /// Entry bookmarks and playback progress.
    pub(crate) bookmarks: SqliteEntityStore<Bookmark>,
}

impl StreamingRepository {
    /// Opens the service's typed tables without creating any profile data.
    ///
    /// # Arguments
    /// * `database` - Shared profile database.
    ///
    /// # Errors
    /// Returns a contextual schema initialization error.
    pub(crate) fn new(database: SqliteDatabase) -> Result<Self> {
        Ok(Self {
            collections: table(&database, "stream_profile_collections")?,
            settings: table(&database, "stream_profile_settings")?,
            homes: table(&database, "profile_homes")?,
            sections: table(&database, "profile_home_sections")?,
            bookmarks: table(&database, "profile_bookmarks")?,
            database,
        })
    }

    /// Reads preferences for internal snapshot construction, without authorization.
    ///
    /// # Arguments
    /// * `profile_id` - Common UUID; the caller must verify active ownership.
    ///
    /// # Errors
    /// Returns a storage/decoding error; absence means not initialized.
    pub fn settings(&self, profile_id: Uuid) -> Result<Option<StreamSettings>> {
        self.database
            .transaction(|tx| tx.get(&self.settings, &profile_id.to_string()))
    }

    /// Deletes streaming data within the profile or account deletion transaction.
    ///
    /// # Arguments
    /// * `tx` - Shared transaction covering the complete deletion.
    /// * `profile_id` - Identity whose ownership the caller has verified.
    pub(crate) fn delete_profile(
        &self,
        tx: &SqliteTransaction<'_>,
        profile_id: Uuid,
    ) -> Result<()> {
        tx.delete_where(&self.settings, &by_id("profile_id", profile_id))?;
        tx.delete_where(&self.homes, &by_id("profile_id", profile_id))?;
        tx.delete_where(&self.sections, &by_id("profile_id", profile_id))?;
        tx.delete_where(&self.bookmarks, &by_id("profile_id", profile_id))?;
        tx.delete_where(&self.collections, &by_id("profile_id", profile_id))
    }
}
