//! Startup consistency checks after replay, including restored store pairs.

use super::*;
use crate::profiles::common::models::{AccountState, ProfileState};
use arachnea_core::persistence::EntityQuery;
use std::collections::{HashMap, HashSet};

impl Inner {
    /// Rejects orphaned/mismatched restored data before profile writes are served.
    pub(super) fn verify_links(&self) -> Result<()> {
        let common = &self.stores.common;
        let (profiles, common_ids, onboarding) = common.database.transaction(|tx| {
            let users = tx
                .find(&common.users, &EntityQuery::new())?
                .into_iter()
                .map(|user| (user.id, user))
                .collect::<HashMap<_, _>>();
            anyhow::ensure!(
                users
                    .values()
                    .all(|user| user.state == AccountState::Active),
                "unfinished account deletion without a recoverable operation"
            );
            let profiles = tx
                .find(&common.profiles, &EntityQuery::new())?
                .into_iter()
                .map(|profile| (profile.id, profile))
                .collect::<HashMap<_, _>>();
            for profile in profiles.values() {
                anyhow::ensure!(
                    profile.state == ProfileState::Active && users.contains_key(&profile.user_id),
                    "profile {} has an unfinished lifecycle or missing account",
                    profile.id
                );
            }
            let settings = tx.find(&common.settings, &EntityQuery::new())?;
            let mut common_ids = HashSet::new();
            for settings in settings {
                anyhow::ensure!(
                    profiles.contains_key(&settings.profile_id)
                        && settings.schema_version == crate::PROFILE_DOCUMENT_VERSION,
                    "invalid common settings reference/version for {}",
                    settings.profile_id
                );
                common_ids.insert(settings.profile_id);
            }
            anyhow::ensure!(
                common_ids.len() == profiles.len(),
                "active profile common settings missing"
            );
            let onboarding = tx.find(&common.onboarding, &EntityQuery::new())?;
            anyhow::ensure!(
                onboarding
                    .iter()
                    .all(|entry| profiles.contains_key(&entry.profile_id)),
                "orphaned onboarding state"
            );
            for profile_id in profiles.keys() {
                anyhow::ensure!(
                    onboarding
                        .iter()
                        .any(|entry| entry.profile_id == *profile_id
                            && entry.scope == crate::PROFILE_COMMON_SCOPE),
                    "common onboarding state missing for {profile_id}"
                );
            }
            for session in tx.find(&common.sessions, &EntityQuery::new())? {
                anyhow::ensure!(
                    users.contains_key(&session.user_id),
                    "orphaned public session"
                );
            }
            Ok((profiles, common_ids, onboarding))
        })?;
        let streaming = &self.stores.streaming;
        streaming
            .database
            .transaction(|tx| {
                let settings = tx.find(&streaming.settings, &EntityQuery::new())?;
                let mut initialized = HashSet::new();
                for settings in settings {
                    anyhow::ensure!(
                        profiles.contains_key(&settings.profile_id)
                            && settings.schema_version == crate::PROFILE_DOCUMENT_VERSION,
                        "invalid streaming settings reference/version for {}",
                        settings.profile_id
                    );
                    initialized.insert(settings.profile_id);
                    anyhow::ensure!(
                        onboarding
                            .iter()
                            .any(|entry| entry.profile_id == settings.profile_id
                                && entry.scope == crate::PROFILE_STREAM_SCOPE),
                        "streaming onboarding state missing for {}",
                        settings.profile_id
                    );
                }
                for entry in &onboarding {
                    anyhow::ensure!(
                        entry.scope != crate::PROFILE_STREAM_SCOPE
                            || initialized.contains(&entry.profile_id),
                        "streaming onboarding without settings for {}",
                        entry.profile_id
                    );
                }
                for id in tx
                    .find(&streaming.collections, &EntityQuery::new())?
                    .into_iter()
                    .map(|row| row.profile_id)
                    .chain(
                        tx.find(&streaming.mutations, &EntityQuery::new())?
                            .into_iter()
                            .map(|row| row.profile_id),
                    )
                {
                    anyhow::ensure!(
                        initialized.contains(&id),
                        "orphaned streaming collection/receipt for {id}"
                    );
                }
                for (id, version) in tx
                    .find(&streaming.homes, &EntityQuery::new())?
                    .into_iter()
                    .map(|row| (row.profile_id, row.schema_version))
                    .chain(
                        tx.find(&streaming.sections, &EntityQuery::new())?
                            .into_iter()
                            .map(|row| (row.profile_id, row.schema_version)),
                    )
                    .chain(
                        tx.find(&streaming.bookmarks, &EntityQuery::new())?
                            .into_iter()
                            .map(|row| (row.profile_id, row.schema_version)),
                    )
                {
                    anyhow::ensure!(
                        initialized.contains(&id)
                            && common_ids.contains(&id)
                            && version == crate::PROFILE_DOCUMENT_VERSION,
                        "orphaned or unsupported streaming document for {id}"
                    );
                }
                Ok(())
            })
            .context("checking common/streaming profile store consistency")
    }
}
