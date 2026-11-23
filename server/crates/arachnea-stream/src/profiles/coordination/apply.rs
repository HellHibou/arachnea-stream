//! Profile changes that share a transaction across both repositories.

use super::*;
use crate::profiles::{
    accounts::reject,
    by_id,
    common::models::{CommonSettings, Onboarding, OnboardingState},
    keys::PairKey,
    streaming::models::StreamSettings,
};

impl Inner {
    /// Creates a profile with its initial settings and guide in the caller's transaction.
    ///
    /// # Arguments
    /// * `tx` - Transaction that validated the current session.
    /// * `user_id` - Owner of the new profile.
    /// * `draft` - Validated identity and common preferences.
    /// * `max` - Current server quota for the account.
    ///
    /// # Errors
    /// Returns a quota, uniqueness or storage error.
    pub(super) fn create(
        &self,
        tx: &SqliteTransaction<'_>,
        user_id: Uuid,
        draft: CreateProfile,
        max: usize,
    ) -> Result<Value> {
        let common = &self.stores.common;
        let profile = common.create_profile(tx, user_id, &draft, max, now_ms()?)?;
        tx.put(
            &common.settings,
            &CommonSettings {
                profile_id: profile.id,
                schema_version: crate::PROFILE_DOCUMENT_VERSION,
                revision: 1,
                values: draft.common,
            },
        )?;
        self.start_guide(tx, profile.id, crate::PROFILE_COMMON_SCOPE)?;
        Ok(json!({"profile_id":profile.id, "common_revision":1}))
    }

    /// Creates streaming preferences and their guide together for an active profile.
    ///
    /// # Arguments
    /// * `tx` - Transaction that validated the current session.
    /// * `user_id` - Authenticated account UUID.
    /// * `profile_id` - Profile to initialize if needed.
    /// * `values` - Validated streaming settings with defaults applied.
    ///
    /// # Errors
    /// Returns an ownership, home-reference or storage error.
    pub(super) fn initialize(
        &self,
        tx: &SqliteTransaction<'_>,
        user_id: Uuid,
        profile_id: Uuid,
        values: Value,
    ) -> Result<Value> {
        self.stores.common.active_profile(tx, user_id, profile_id)?;
        let streaming = &self.stores.streaming;
        if tx
            .get(&streaming.settings, &profile_id.to_string())?
            .is_none()
        {
            self.check_home_reference(tx, profile_id, &values)?;
            tx.put(
                &streaming.settings,
                &StreamSettings {
                    profile_id,
                    schema_version: crate::PROFILE_DOCUMENT_VERSION,
                    revision: 1,
                    values,
                },
            )?;
            self.start_guide(tx, profile_id, crate::PROFILE_STREAM_SCOPE)?;
        }
        Ok(json!({"profile_id":profile_id}))
    }

    /// Saves changed settings domains together, rolling back if either revision is stale.
    ///
    /// # Arguments
    /// * `tx` - Transaction that validated the current session.
    /// * `user_id` - Authenticated account UUID.
    /// * `profile_id` - Profile whose settings are being changed.
    /// * `common_change` - Optional common replacement and expected revision.
    /// * `stream_change` - Optional streaming replacement and expected revision.
    ///
    /// # Errors
    /// Returns an ownership, revision, validation or storage error.
    pub(super) fn save(
        &self,
        tx: &SqliteTransaction<'_>,
        user_id: Uuid,
        profile_id: Uuid,
        common_change: Option<SettingsChange<CommonPreferences>>,
        stream_change: Option<SettingsChange<Value>>,
    ) -> Result<Value> {
        let common = &self.stores.common;
        let streaming = &self.stores.streaming;
        common.active_profile(tx, user_id, profile_id)?;
        let key = profile_id.to_string();
        let mut common_settings = tx
            .get(&common.settings, &key)?
            .context("common settings missing")?;
        let mut stream_settings = tx.get(&streaming.settings, &key)?;
        if let Some(change) = common_change {
            anyhow::ensure!(
                common_settings.revision == change.expected_revision,
                "revision_conflict"
            );
            common_settings.revision = next_revision(common_settings.revision)?;
            common_settings.values = change.values;
            tx.put(&common.settings, &common_settings)?;
        }
        if let Some(change) = stream_change {
            let settings = stream_settings
                .as_mut()
                .context("streaming profile is not initialized")?;
            anyhow::ensure!(
                settings.revision == change.expected_revision,
                "revision_conflict"
            );
            self.check_home_reference(tx, profile_id, &change.values)?;
            settings.revision = next_revision(settings.revision)?;
            settings.values = change.values;
            tx.put(&streaming.settings, settings)?;
        }
        Ok(
            json!({"profile_id":profile_id, "common_revision":common_settings.revision,
            "streaming_revision":stream_settings.map(|settings| settings.revision)}),
        )
    }

    /// Removes a profile and its dependent data using the caller's transaction.
    ///
    /// # Arguments
    /// * `tx` - Transaction that authorized the deletion.
    /// * `profile_id` - Profile whose rows are being removed.
    ///
    /// # Errors
    /// Returns a storage error, which the caller must propagate to roll back the deletion.
    pub(super) fn purge_profile(&self, tx: &SqliteTransaction<'_>, profile_id: Uuid) -> Result<()> {
        let common = &self.stores.common;
        self.stores.streaming.delete_profile(tx, profile_id)?;
        tx.delete_where(&common.settings, &by_id("profile_id", profile_id))?;
        tx.delete_where(&common.onboarding, &by_id("profile_id", profile_id))?;
        tx.delete_where(&common.profiles, &by_id("id", profile_id))
    }

    /// Inserts the initial guide state alongside the settings it accompanies.
    fn start_guide(&self, tx: &SqliteTransaction<'_>, profile_id: Uuid, scope: &str) -> Result<()> {
        tx.put(
            &self.stores.common.onboarding,
            &Onboarding {
                profile_id,
                scope: scope.to_owned(),
                state: OnboardingState::Pending,
                revision: 1,
            },
        )
    }

    /// Checks that an optional default home belongs to the edited profile.
    fn check_home_reference(
        &self,
        tx: &SqliteTransaction<'_>,
        profile_id: Uuid,
        values: &Value,
    ) -> Result<()> {
        if let Some(id) = values["defaultHomeId"].as_str() {
            if tx
                .get(
                    &self.stores.streaming.homes,
                    &PairKey([profile_id.to_string(), id.into()]),
                )?
                .is_none()
            {
                return Err(reject(
                    "invalid_data",
                    422,
                    "Default home must belong to this profile.",
                ));
            }
        }
        Ok(())
    }
}
