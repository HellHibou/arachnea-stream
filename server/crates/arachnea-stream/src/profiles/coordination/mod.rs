//! Atomic account and profile mutations across shared database tables.

mod api;
mod apply;
mod integrity;
/// Validated profile request shapes.
pub mod models;
mod streaming_api;

use super::common::models::CommonPreferences;
use super::{accounts::reject, by_id, ProfileStores};
use anyhow::{Context, Result};
use arachnea_core::persistence::SqliteTransaction;
use models::{CreateProfile, SettingsChange};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

/// Shared account/profile guards. Weak entries avoid retaining every historical ID.
#[derive(Default)]
pub(super) struct Guards(Mutex<HashMap<(bool, Uuid), Weak<Mutex<()>>>>);

impl Guards {
    /// Returns the shared lifecycle guard for an account or profile.
    fn get(&self, account: bool, id: Uuid) -> Result<Arc<Mutex<()>>> {
        let mut guards = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("profile guard registry poisoned"))?;
        guards.retain(|_, guard| guard.strong_count() != 0);
        let entry = guards.entry((account, id)).or_default();
        if let Some(guard) = entry.upgrade() {
            return Ok(guard);
        }
        let guard = Arc::new(Mutex::new(()));
        *entry = Arc::downgrade(&guard);
        Ok(guard)
    }
}

/// Store handles and a single shared mutation guard registry.
struct Inner {
    stores: ProfileStores,
    guards: Arc<Guards>,
}

/// Application mutation boundary; clone this handle rather than constructing peers.
///
/// Work and lifecycle guards live inside a blocking task, so cancelling a future
/// cannot release its guard while an SQLite commit is still running. Account
/// guards precede profile guards; this first version serializes mutations of
/// the same account, including different profiles.
#[derive(Clone)]
pub struct ProfileCoordinator(Arc<Inner>);

impl ProfileCoordinator {
    /// Composes the repositories sharing the profile database.
    ///
    /// # Arguments
    /// * `stores` - Application-owned common and streaming table views.
    pub fn new(stores: ProfileStores) -> Self {
        Self(Arc::new(Inner {
            guards: stores.guards.clone(),
            stores,
        }))
    }

    /// Runs an account credential mutation under the shared lifecycle guard.
    ///
    /// # Arguments
    /// * `user_id` - Account whose sessions/credentials are being changed.
    /// * `work` - Synchronous local mutation, retained through future cancellation.
    ///
    /// # Errors
    /// Returns the callback or blocking-task error.
    pub(crate) async fn account_task<T: Send + 'static>(
        &self,
        user_id: Uuid,
        work: impl FnOnce(&super::common::CommonRepository) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.run(user_id, None, move |inner| work(&inner.stores.common))
            .await
    }

    /// Deletes an administrator-authorized account, all its profiles and sessions atomically.
    ///
    /// # Arguments
    /// * `user_id` - Target account UUID.
    ///
    /// # Errors
    /// Returns a missing-account or storage error.
    pub(crate) async fn delete_user(&self, user_id: Uuid) -> Result<Value> {
        self.run(user_id, None, move |inner| {
            let common = &inner.stores.common;
            common.database.transaction(|tx| {
                if tx.get(&common.users, &user_id.to_string())?.is_none() {
                    return Err(reject(
                        "resource_not_found",
                        404,
                        "Account is inaccessible.",
                    ));
                }
                for profile in tx.find(&common.profiles, &by_id("user_id", user_id))? {
                    inner.purge_profile(tx, profile.id)?;
                }
                tx.delete_where(&common.sessions, &by_id("user_id", user_id))?;
                tx.delete_where(&common.users, &by_id("id", user_id))?;
                Ok(json!({"user_id":user_id, "deleted":true}))
            })
        })
        .await
    }

    /// Checks profile references before application routes become available.
    ///
    /// # Errors
    /// Returns a storage or profile consistency error.
    pub async fn verify(&self) -> Result<()> {
        let inner = self.0.clone();
        tokio::task::spawn_blocking(move || inner.verify_links())
            .await
            .context("checking profile references")?
    }

    /// Runs one entire mutation while retaining guards through blocking commits.
    async fn run<T: Send + 'static>(
        &self,
        user_id: Uuid,
        profile_id: Option<Uuid>,
        work: impl FnOnce(&Inner) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let inner = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let account = inner.guards.get(true, user_id)?;
            let _account = account
                .lock()
                .map_err(|_| anyhow::anyhow!("account lifecycle guard poisoned"))?;
            let profile = profile_id
                .map(|id| inner.guards.get(false, id))
                .transpose()?;
            let _profile = profile
                .as_ref()
                .map(|guard| {
                    guard
                        .lock()
                        .map_err(|_| anyhow::anyhow!("profile lifecycle guard poisoned"))
                })
                .transpose()?;
            work(&inner)
        })
        .await
        .context("profile blocking operation failed")?
    }
}

/// Returns epoch milliseconds without panicking on a pre-epoch/overflow clock.
fn now_ms() -> Result<i64> {
    Ok(i64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}

/// Advances a revision without exceeding the exact JSON integer range.
fn next_revision(revision: i64) -> Result<i64> {
    anyhow::ensure!(
        (1..crate::PROFILE_MAX_REVISION).contains(&revision),
        "profile revision exhausted"
    );
    Ok(revision + 1)
}
