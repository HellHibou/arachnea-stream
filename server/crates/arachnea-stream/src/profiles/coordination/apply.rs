//! Idempotent local commits and application-owned cross-store recovery.

use super::*;
use super::{models::Intent, prepare::next_revision};
use crate::profiles::{by_id, common::models::*, keys::PairKey, streaming::models::StreamSettings};
use arachnea_core::persistence::SqliteTransaction;

impl Inner {
    /// Applies the stored intent while the caller retains lifecycle guards.
    ///
    /// # Arguments
    /// * `operation` - Durable intent and last confirmed local checkpoint.
    ///
    /// # Errors
    /// Returns state, ownership, version or storage errors for durable retry.
    pub(super) fn apply(&self, operation: &mut Operation) -> Result<()> {
        let intent: Intent = serde_json::from_value(operation.payload.clone())
            .context("decoding profile operation intent")?;
        match intent {
            Intent::CreateProfile { common } => self.create(operation, common),
            Intent::InitializeStreaming { values } => self.initialize(operation, values),
            Intent::SavePreferences { common, streaming } => {
                self.save(operation, common, streaming)
            }
            Intent::DeleteProfile => self.delete(operation),
            Intent::DeleteUser => self.delete_account(operation),
        }
    }

    /// Publishes a common identity only together with its settings and guide.
    fn create(&self, op: &mut Operation, values: CommonPreferences) -> Result<()> {
        let id = op.profile_id.context("creation profile UUID missing")?;
        let common = &self.stores.common;
        common.database.transaction(|tx| {
            let user = tx
                .get(&common.users, &op.user_id.to_string())?
                .context("creation account missing")?;
            anyhow::ensure!(
                user.state == AccountState::Active,
                "creation account is deleting"
            );
            let mut profile = tx
                .get(&common.profiles, &id.to_string())?
                .context("reserved profile missing")?;
            anyhow::ensure!(
                profile.user_id == op.user_id && profile.state == ProfileState::Creating,
                "reserved profile is inaccessible"
            );
            tx.put(
                &common.settings,
                &CommonSettings {
                    profile_id: id,
                    schema_version: crate::PROFILE_DOCUMENT_VERSION,
                    revision: 1,
                    values,
                    last_operation_id: op.operation_id,
                },
            )?;
            tx.put(
                &common.onboarding,
                &Onboarding {
                    profile_id: id,
                    scope: crate::PROFILE_COMMON_SCOPE.to_string(),
                    state: OnboardingState::Pending,
                    revision: 1,
                },
            )?;
            profile.state = ProfileState::Active;
            profile.updated_at = now_ms()?;
            tx.put(&common.profiles, &profile)?;
            self.complete(tx, op, json!({"profile_id":id, "common_revision":1}))
        })
    }

    /// Checks active ownership immediately before a service transaction.
    fn authorize(&self, op: &Operation) -> Result<Uuid> {
        let id = op.profile_id.context("operation profile UUID missing")?;
        let common = &self.stores.common;
        common
            .database
            .transaction(|tx| common.active_profile(tx, op.user_id, id))?;
        Ok(id)
    }

    /// Initializes streaming preferences once, then records common service-guide state.
    fn initialize(&self, op: &mut Operation, values: Value) -> Result<()> {
        let id = self.authorize(op)?;
        let streaming = &self.stores.streaming;
        streaming.database.transaction(|tx| {
            if let Some(settings) = tx.get(&streaming.settings, &id.to_string())? {
                anyhow::ensure!(
                    settings.last_operation_id == op.operation_id,
                    "streaming initialization marker conflict"
                );
            } else {
                tx.put(
                    &streaming.settings,
                    &StreamSettings {
                        profile_id: id,
                        schema_version: crate::PROFILE_DOCUMENT_VERSION,
                        revision: 1,
                        values,
                        last_operation_id: op.operation_id,
                    },
                )?;
            }
            Ok(())
        })?;
        let common = &self.stores.common;
        common.database.transaction(|tx| {
            common.active_profile(tx, op.user_id, id)?;
            let key = PairKey([id.to_string(), crate::PROFILE_STREAM_SCOPE.to_string()]);
            if tx.get(&common.onboarding, &key)?.is_none() {
                tx.put(
                    &common.onboarding,
                    &Onboarding {
                        profile_id: id,
                        scope: crate::PROFILE_STREAM_SCOPE.to_string(),
                        state: OnboardingState::Pending,
                        revision: 1,
                    },
                )?;
            }
            self.complete(tx, op, json!({"profile_id":id, "streaming_revision":1}))
        })
    }

    /// Commits common then streaming preferences, replaying through durable markers.
    fn save(
        &self,
        op: &mut Operation,
        common_change: Option<SettingsChange<CommonPreferences>>,
        stream_change: Option<SettingsChange<Value>>,
    ) -> Result<()> {
        let id = self.authorize(op)?;
        let common = &self.stores.common;
        common.database.transaction(|tx| {
            common.active_profile(tx, op.user_id, id)?;
            if let Some(change) = common_change {
                let mut settings = tx
                    .get(&common.settings, &id.to_string())?
                    .context("common preferences missing")?;
                if settings.last_operation_id != op.operation_id {
                    anyhow::ensure!(
                        settings.revision == change.expected_revision,
                        "revision_conflict during common recovery"
                    );
                    settings.revision = next_revision(settings.revision)?;
                    settings.values = change.values;
                    settings.last_operation_id = op.operation_id;
                    tx.put(&common.settings, &settings)?;
                }
            }
            op.step = OperationStep::CommonApplied;
            tx.put(&common.operations, op)
        })?;
        self.authorize(op)?;
        let streaming = &self.stores.streaming;
        streaming.database.transaction(|tx| {
            if let Some(change) = stream_change {
                let mut settings = tx
                    .get(&streaming.settings, &id.to_string())?
                    .context("streaming preferences missing")?;
                if settings.last_operation_id != op.operation_id {
                    anyhow::ensure!(
                        settings.revision == change.expected_revision,
                        "revision_conflict during streaming recovery"
                    );
                    settings.revision = next_revision(settings.revision)?;
                    settings.values = change.values;
                    settings.last_operation_id = op.operation_id;
                    tx.put(&streaming.settings, &settings)?;
                }
            }
            Ok(())
        })?;
        let streaming_revision = streaming.settings(id)?.map(|settings| settings.revision);
        common.database.transaction(|tx| {
            common.active_profile(tx, op.user_id, id)?;
            let common_revision = tx.get(&common.settings, &id.to_string())?.context("common preferences missing after save")?.revision;
            self.complete(tx, op, json!({"profile_id":id, "common_revision":common_revision, "streaming_revision":streaming_revision}))
        })
    }

    /// Purges service data before removing an already inaccessible common identity.
    fn delete(&self, op: &mut Operation) -> Result<()> {
        let id = op.profile_id.context("deletion profile UUID missing")?;
        let common = &self.stores.common;
        common.database.transaction(|tx| {
            let profile = tx
                .get(&common.profiles, &id.to_string())?
                .context("deleting profile missing")?;
            anyhow::ensure!(
                profile.user_id == op.user_id && profile.state == ProfileState::Deleting,
                "profile deletion state mismatch"
            );
            Ok(())
        })?;
        self.stores
            .streaming
            .delete_profile(id)
            .context("purging streaming profile")?;
        common.database.transaction(|tx| {
            self.purge_common_profile(tx, id)?;
            self.complete(tx, op, json!({"profile_id":id, "deleted":true}))
        })
    }

    /// Cleans every profile before atomically removing the common account records.
    fn delete_account(&self, op: &mut Operation) -> Result<()> {
        let common = &self.stores.common;
        let profiles = common.database.transaction(|tx| {
            let user = tx
                .get(&common.users, &op.user_id.to_string())?
                .context("deleting account missing")?;
            anyhow::ensure!(
                user.state == AccountState::Deleting,
                "account deletion state mismatch"
            );
            tx.find(&common.profiles, &by_id("user_id", op.user_id))
        })?;
        for profile in &profiles {
            anyhow::ensure!(
                profile.state == ProfileState::Deleting,
                "account child profile is not blocked"
            );
            self.stores.streaming.delete_profile(profile.id)?;
        }
        common.database.transaction(|tx| {
            for profile in profiles {
                self.purge_common_profile(tx, profile.id)?;
            }
            tx.delete_where(&common.sessions, &by_id("user_id", op.user_id))?;
            tx.delete_where(&common.users, &by_id("id", op.user_id))?;
            self.complete(tx, op, json!({"user_id":op.user_id, "deleted":true}))
        })
    }

    /// Removes only common data, retaining operation receipts for safe retries.
    fn purge_common_profile(&self, tx: &SqliteTransaction<'_>, id: Uuid) -> Result<()> {
        let common = &self.stores.common;
        tx.delete_where(&common.settings, &by_id("profile_id", id))?;
        tx.delete_where(&common.onboarding, &by_id("profile_id", id))?;
        tx.delete_where(&common.profiles, &by_id("id", id))
    }
}
