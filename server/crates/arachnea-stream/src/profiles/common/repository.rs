//! Common-only repository: no streaming dependencies.

use super::models::*;
use crate::profiles::{by_id, common::operations::Operation, stores::table};
use anyhow::{Context, Result};
use arachnea_core::persistence::{
    EntityQuery, SqliteDatabase, SqliteEntityStore, SqliteTransaction,
};
use uuid::Uuid;

/// Common typed tables and local transaction boundary.
///
/// Synchronous repository methods must run on a blocking thread. Raw table
/// handles stay crate-private so callers cannot bypass coordinated mutations.
pub struct CommonRepository {
    /// Common physical transaction boundary.
    pub(crate) database: SqliteDatabase,
    /// Public account identities and password hashes.
    pub(crate) users: SqliteEntityStore<User>,
    /// Persisted session digests and expiry/revocation.
    pub(crate) sessions: SqliteEntityStore<UserSession>,
    /// Authoritative profile ownership and lifecycle.
    pub(crate) profiles: SqliteEntityStore<Profile>,
    /// Versioned common preferences.
    pub(crate) settings: SqliteEntityStore<CommonSettings>,
    /// Common/service guide completion states.
    pub(crate) onboarding: SqliteEntityStore<Onboarding>,
    /// Opaque durable intents and idempotency receipts.
    pub(crate) operations: SqliteEntityStore<Operation>,
}

impl CommonRepository {
    /// Opens common-domain tables and verifies identity uniqueness constraints.
    ///
    /// # Arguments
    /// * `database` - The common profiles database.
    ///
    /// # Errors
    /// Returns schema, incompatible index or duplicate stored identity errors.
    pub(crate) fn new(database: SqliteDatabase) -> Result<Self> {
        let repository = Self {
            users: table(&database, "users")?,
            sessions: table(&database, "user_sessions")?,
            profiles: table(&database, "profiles")?,
            settings: table(&database, "profile_settings")?,
            onboarding: table(&database, "profile_onboarding")?,
            operations: table(&database, "profile_operations")?,
            database,
        };
        repository.database.transaction(|tx| {
            tx.ensure_unique(&repository.users, &["normalized_login"])?;
            tx.ensure_unique(&repository.profiles, &["user_id", "normalized_name"])?;
            tx.ensure_unique(&repository.sessions, &["session_digest"])
        })?;
        Ok(repository)
    }

    /// Reserves quota/name and generates the UUID inside the common transaction.
    ///
    /// # Arguments
    /// * `tx` - Common transaction also persisting the caller's durable intent.
    /// * `user_id` - Active account already checked by the caller.
    /// * `draft` - Validated common identity request.
    /// * `max_profiles` - Effective server quota.
    /// * `timestamp` - Server epoch milliseconds.
    ///
    /// # Errors
    /// Rejects exhausted quotas, duplicate names and storage failures.
    pub(crate) fn reserve_profile(
        &self,
        tx: &SqliteTransaction<'_>,
        user_id: Uuid,
        draft: &CreateProfile,
        max_profiles: usize,
        timestamp: i64,
    ) -> Result<Profile> {
        anyhow::ensure!(max_profiles > 0, "invalid profile quota");
        let profiles = tx.find(&self.profiles, &by_id("user_id", user_id))?;
        anyhow::ensure!(profiles.len() < max_profiles, "profile_quota_reached");
        anyhow::ensure!(
            profiles
                .iter()
                .all(|p| p.normalized_name != draft.normalized_name),
            "profile_name_taken"
        );
        let profile_id = Uuid::new_v4();
        anyhow::ensure!(
            tx.get(&self.profiles, &profile_id.to_string())?.is_none()
                && tx
                    .find(&self.operations, &by_id("profile_id", profile_id))?
                    .is_empty(),
            "generated profile identity already used"
        );
        let order = profiles
            .iter()
            .map(|p| p.order)
            .max()
            .map(|order| order.checked_add(1).context("profile order overflow"))
            .transpose()?
            .unwrap_or(0);
        let profile = Profile {
            id: profile_id,
            user_id,
            name: draft.name.clone(),
            normalized_name: draft.normalized_name.clone(),
            avatar_id: draft.avatar_id.clone(),
            order,
            state: ProfileState::Creating,
            revision: 1,
            created_at: timestamp,
            updated_at: timestamp,
        };
        tx.put(&self.profiles, &profile)?;
        Ok(profile)
    }

    /// Inserts a previously validated account, without exposing an HTTP API.
    ///
    /// # Arguments
    /// * `user` - New account with a stable UUID and a password hash, never plaintext.
    ///
    /// # Errors
    /// Rejects reused identity/login and storage failures.
    pub fn insert_user(&self, user: &User) -> Result<()> {
        self.database
            .transaction(|tx| {
                anyhow::ensure!(
                    tx.get(&self.users, &user.id.to_string())?.is_none(),
                    "account identity already exists"
                );
                anyhow::ensure!(
                    tx.find(&self.operations, &by_id("user_id", user.id))?
                        .is_empty(),
                    "account identity was previously used"
                );
                tx.put(&self.users, user)
            })
            .context("inserting public account")
    }

    /// Reads an account for the future authentication service.
    ///
    /// # Arguments
    /// * `user_id` - Account UUID.
    ///
    /// # Errors
    /// Returns a storage/decoding error.
    pub fn user(&self, user_id: Uuid) -> Result<Option<User>> {
        self.database
            .transaction(|tx| tx.get(&self.users, &user_id.to_string()))
    }

    /// Lists common profile identities, including unfinished lifecycle states.
    ///
    /// # Arguments
    /// * `user_id` - Account whose identities are requested; service checks authorization.
    ///
    /// # Errors
    /// Returns a storage/decoding error.
    pub fn profiles(&self, user_id: Uuid) -> Result<Vec<Profile>> {
        self.database
            .transaction(|tx| tx.find(&self.profiles, &by_id("user_id", user_id)))
    }

    /// Reads the durable operation receipt without authorizing its caller.
    ///
    /// # Arguments
    /// * `operation_id` - Stable idempotency/recovery identity.
    ///
    /// # Errors
    /// Returns a storage/decoding error.
    pub fn operation(&self, operation_id: Uuid) -> Result<Option<Operation>> {
        self.database
            .transaction(|tx| tx.get(&self.operations, &operation_id.to_string()))
    }

    /// Revalidates active account ownership inside the caller's transaction.
    ///
    /// # Arguments
    /// * `tx` - Common local transaction.
    /// * `user_id` - Authenticated account identity, supplied by the future service.
    /// * `profile_id` - Requested profile identity.
    ///
    /// # Errors
    /// Refuses missing, foreign or non-active identities.
    pub(crate) fn active_profile(
        &self,
        tx: &SqliteTransaction<'_>,
        user_id: Uuid,
        profile_id: Uuid,
    ) -> Result<Profile> {
        let user = tx
            .get(&self.users, &user_id.to_string())?
            .context("account is inaccessible")?;
        anyhow::ensure!(user.state == AccountState::Active, "account is deleting");
        let profile = tx
            .get(&self.profiles, &profile_id.to_string())?
            .context("profile is inaccessible")?;
        anyhow::ensure!(
            profile.user_id == user_id && profile.state == ProfileState::Active,
            "profile is inaccessible"
        );
        Ok(profile)
    }

    /// Lists durable operations for startup recovery, without streaming knowledge.
    ///
    /// # Errors
    /// Returns a storage/decoding error.
    pub(crate) fn operations(&self) -> Result<Vec<Operation>> {
        self.database
            .transaction(|tx| tx.find(&self.operations, &EntityQuery::new()))
    }
}
