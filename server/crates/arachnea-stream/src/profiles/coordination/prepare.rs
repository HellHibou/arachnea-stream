//! Intent validation and common-store reservations before any service write.

use super::models::Intent;
use super::*;
use crate::profiles::{by_id, common::models::*};
use arachnea_core::persistence::SqliteTransaction;

impl Inner {
    /// Validates account state even when replaying an idempotency receipt.
    fn active_account(&self, tx: &SqliteTransaction<'_>, user_id: Uuid) -> Result<User> {
        let user = tx
            .get(&self.stores.common.users, &user_id.to_string())?
            .context("account is inaccessible")?;
        anyhow::ensure!(user.state == AccountState::Active, "account is deleting");
        Ok(user)
    }

    /// Returns an existing receipt only when its actor, target and body agree.
    ///
    /// # Arguments
    /// * `tx` - Common transaction containing the receipt.
    /// * `id` - Client operation UUID.
    /// * `user_id` - Authenticated account identity.
    /// * `profile_id` - Optional expected profile target.
    /// * `request` - Normalized complete mutation body including its kind.
    pub(super) fn existing(
        &self,
        tx: &SqliteTransaction<'_>,
        id: Uuid,
        user_id: Uuid,
        profile_id: Option<Uuid>,
        request: &Value,
    ) -> Result<Option<Operation>> {
        let existing = tx.get(&self.stores.common.operations, &id.to_string())?;
        if let Some(op) = &existing {
            anyhow::ensure!(
                op.user_id == user_id
                    && (profile_id.is_none() || op.profile_id == profile_id)
                    && &op.request == request,
                "operation_id_conflict"
            );
        }
        Ok(existing)
    }

    /// Rejects a second mutation until this profile's pending intent is resolved.
    ///
    /// # Arguments
    /// * `tx` - Common transaction containing pending intents.
    /// * `profile_id` - Profile whose mutations must be serialized.
    pub(super) fn no_pending(&self, tx: &SqliteTransaction<'_>, profile_id: Uuid) -> Result<()> {
        let common = &self.stores.common;
        anyhow::ensure!(
            tx.find(&common.operations, &by_id("profile_id", profile_id))?
                .iter()
                .all(|op| op.state == OperationState::Completed),
            "operation_in_progress"
        );
        Ok(())
    }

    /// Reserves identity/name/quota and creation intent in one local transaction.
    ///
    /// # Arguments
    /// * `user_id` - Authorized active account.
    /// * `id` - Stable operation UUID.
    /// * `draft` - Validated common identity and defaults.
    /// * `max_profiles` - Effective server quota.
    ///
    /// # Errors
    /// Returns identity, revision, pending-operation or storage errors.
    pub(super) fn prepare_create(
        &self,
        user_id: Uuid,
        id: Uuid,
        draft: CreateProfile,
        max_profiles: usize,
    ) -> Result<Operation> {
        let request = json!({"kind":"create_profile", "draft":draft});
        let common = &self.stores.common;
        common
            .database
            .transaction(|tx| {
                self.active_account(tx, user_id)?;
                if let Some(op) = self.existing(tx, id, user_id, None, &request)? {
                    let profile = tx
                        .get(
                            &common.profiles,
                            &op.profile_id
                                .context("creation profile missing")?
                                .to_string(),
                        )?
                        .context("profile is inaccessible")?;
                    anyhow::ensure!(
                        profile.user_id == user_id && profile.state != ProfileState::Deleting,
                        "profile is inaccessible"
                    );
                    return Ok(op);
                }
                let profile =
                    common.reserve_profile(tx, user_id, &draft, max_profiles, now_ms()?)?;
                let profile_id = profile.id;
                let op = operation(
                    id,
                    user_id,
                    Some(profile_id),
                    "create_profile",
                    request,
                    Intent::CreateProfile {
                        common: draft.common,
                    },
                )?;
                tx.put(&common.operations, &op)?;
                Ok(op)
            })
            .context("reserving common profile identity")
    }

    /// Prepares a first-use service initialization without service-specific common data.
    ///
    /// # Arguments
    /// * `user_id` - Authorized active account.
    /// * `profile_id` - Active owned common UUID.
    /// * `id` - Stable operation UUID.
    /// * `values` - Validated streaming defaults.
    ///
    /// # Errors
    /// Returns identity, revision, pending-operation or storage errors.
    pub(super) fn prepare_initialize(
        &self,
        user_id: Uuid,
        profile_id: Uuid,
        id: Uuid,
        values: Value,
    ) -> Result<Operation> {
        let request = json!({"kind":"initialize_streaming", "values":values});
        let common = &self.stores.common;
        common.database.transaction(|tx| {
            common.active_profile(tx, user_id, profile_id)?;
            if let Some(op) = self.existing(tx, id, user_id, Some(profile_id), &request)? {
                return Ok(op);
            }
            self.no_pending(tx, profile_id)?;
            anyhow::ensure!(
                self.stores.streaming.settings(profile_id)?.is_none(),
                "streaming profile already initialized"
            );
            let op = operation(
                id,
                user_id,
                Some(profile_id),
                "initialize_streaming",
                request,
                Intent::InitializeStreaming { values },
            )?;
            tx.put(&common.operations, &op)?;
            Ok(op)
        })
    }

    /// Validates both expected revisions before committing a mixed-save intent.
    ///
    /// # Arguments
    /// * `user_id` - Authorized active account.
    /// * `profile_id` - Active owned common UUID.
    /// * `id` - Stable operation UUID.
    /// * `common_change` - Optional common document and expected revision.
    /// * `streaming` - Optional streaming document and expected revision.
    ///
    /// # Errors
    /// Returns identity, revision, pending-operation or storage errors.
    pub(super) fn prepare_save(
        &self,
        user_id: Uuid,
        profile_id: Uuid,
        id: Uuid,
        common_change: Option<SettingsChange<CommonPreferences>>,
        streaming: Option<SettingsChange<Value>>,
    ) -> Result<Operation> {
        anyhow::ensure!(
            common_change.is_some() || streaming.is_some(),
            "preferences request has no domain"
        );
        let request =
            json!({"kind":"save_preferences", "common":common_change, "streaming":streaming});
        let common = &self.stores.common;
        common.database.transaction(|tx| {
            common.active_profile(tx, user_id, profile_id)?;
            if let Some(op) = self.existing(tx, id, user_id, Some(profile_id), &request)? {
                return Ok(op);
            }
            self.no_pending(tx, profile_id)?;
            if let Some(change) = &common_change {
                let settings = tx
                    .get(&common.settings, &profile_id.to_string())?
                    .context("common settings missing")?;
                anyhow::ensure!(
                    settings.revision == change.expected_revision,
                    "revision_conflict: common preferences"
                );
                next_revision(settings.revision)?;
            }
            if let Some(change) = &streaming {
                let settings = self
                    .stores
                    .streaming
                    .settings(profile_id)?
                    .context("streaming profile is not initialized")?;
                anyhow::ensure!(
                    settings.revision == change.expected_revision,
                    "revision_conflict: streaming preferences"
                );
                next_revision(settings.revision)?;
            }
            let op = operation(
                id,
                user_id,
                Some(profile_id),
                "save_preferences",
                request,
                Intent::SavePreferences {
                    common: common_change,
                    streaming,
                },
            )?;
            tx.put(&common.operations, &op)?;
            Ok(op)
        })
    }

    /// Blocks the identity and supersedes earlier unfinished saves atomically.
    ///
    /// # Arguments
    /// * `user_id` - Authorized active account.
    /// * `profile_id` - Owned common UUID to block.
    /// * `id` - Stable deletion operation UUID.
    /// * `expected_revision` - Confirmed identity revision.
    ///
    /// # Errors
    /// Returns identity, revision, pending-operation or storage errors.
    pub(super) fn prepare_delete(
        &self,
        user_id: Uuid,
        profile_id: Uuid,
        id: Uuid,
        expected_revision: i64,
    ) -> Result<Operation> {
        let request = json!({"kind":"delete_profile", "expected_revision":expected_revision});
        let common = &self.stores.common;
        common.database.transaction(|tx| {
            self.active_account(tx, user_id)?;
            if let Some(op) = self.existing(tx, id, user_id, Some(profile_id), &request)? {
                return Ok(op);
            }
            let mut profile = common.active_profile(tx, user_id, profile_id)?;
            anyhow::ensure!(
                profile.revision == expected_revision,
                "revision_conflict: profile identity"
            );
            profile.state = ProfileState::Deleting;
            profile.revision = next_revision(profile.revision)?;
            profile.updated_at = now_ms()?;
            tx.put(&common.profiles, &profile)?;
            self.supersede(tx, user_id, Some(profile_id), id)?;
            let op = operation(
                id,
                user_id,
                Some(profile_id),
                "delete_profile",
                request,
                Intent::DeleteProfile,
            )?;
            tx.put(&common.operations, &op)?;
            Ok(op)
        })
    }

    /// Blocks a whole account and revokes all sessions before touching streaming.
    ///
    /// # Arguments
    /// * `user_id` - Account targeted by the administrator service.
    /// * `id` - Stable deletion operation UUID.
    ///
    /// # Errors
    /// Returns identity, revision, pending-operation or storage errors.
    pub(super) fn prepare_delete_user(&self, user_id: Uuid, id: Uuid) -> Result<Operation> {
        let request = json!({"kind":"delete_user"});
        let common = &self.stores.common;
        common.database.transaction(|tx| {
            if let Some(op) = self.existing(tx, id, user_id, None, &request)? {
                return Ok(op);
            }
            let mut user = self.active_account(tx, user_id)?;
            user.state = AccountState::Deleting;
            user.updated_at = now_ms()?;
            tx.put(&common.users, &user)?;
            for mut session in tx.find(&common.sessions, &by_id("user_id", user_id))? {
                session.revoked = true;
                tx.put(&common.sessions, &session)?;
            }
            for mut profile in tx.find(&common.profiles, &by_id("user_id", user_id))? {
                profile.state = ProfileState::Deleting;
                profile.revision = next_revision(profile.revision)?;
                profile.updated_at = user.updated_at;
                tx.put(&common.profiles, &profile)?;
            }
            self.supersede(tx, user_id, None, id)?;
            let op = operation(
                id,
                user_id,
                None,
                "delete_user",
                request,
                Intent::DeleteUser,
            )?;
            tx.put(&common.operations, &op)?;
            Ok(op)
        })
    }

    /// Makes older pending intents inert before destructive cleanup.
    fn supersede(
        &self,
        tx: &SqliteTransaction<'_>,
        user_id: Uuid,
        profile_id: Option<Uuid>,
        deletion_id: Uuid,
    ) -> Result<()> {
        for mut op in tx.find(&self.stores.common.operations, &by_id("user_id", user_id))? {
            if op.state != OperationState::Completed
                && (profile_id.is_none() || op.profile_id == profile_id)
            {
                self.complete(tx, &mut op, json!({"superseded_by":deletion_id}))?;
            }
        }
        Ok(())
    }
}

/// Advances a revision without exceeding the exact JSON integer range.
///
/// # Arguments
/// * `revision` - Current positive JSON-safe revision.
///
/// # Errors
/// Returns identity, revision, pending-operation or storage errors.
pub(super) fn next_revision(revision: i64) -> Result<i64> {
    anyhow::ensure!(
        (1..crate::PROFILE_MAX_REVISION).contains(&revision),
        "profile revision exhausted or invalid"
    );
    Ok(revision + 1)
}
