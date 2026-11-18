//! Durable forward recovery across independently committed profile stores.

mod api;
mod apply;
mod integrity;
/// Validated request shapes and application-owned recovery intents.
pub mod models;
mod prepare;

use super::common::models::CommonPreferences;
use super::{
    common::operations::{Operation, OperationState, OperationStep},
    ProfileStores,
};
use anyhow::{Context, Result};
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
    /// Composes the two stores; recovery must run before accepting profile writes.
    ///
    /// # Arguments
    /// * `stores` - Unique application-owned pair of profile stores.
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

    /// Reserves quota/name and generates the authoritative common profile UUID.
    ///
    /// # Arguments
    /// * `user_id` - Existing account UUID, authorized by the calling service.
    /// * `operation_id` - Stable non-secret idempotency UUID.
    /// * `draft` - Name/avatar/preferences validated by the future API service.
    /// * `max_profiles` - Effective persisted application quota, never a client option.
    ///
    /// # Errors
    /// Rejects inactive accounts, quota/name conflicts, ID reuse or storage errors.
    pub async fn create_profile(
        &self,
        user_id: Uuid,
        operation_id: Uuid,
        draft: CreateProfile,
        max_profiles: usize,
    ) -> Result<Operation> {
        self.run(user_id, None, move |inner| {
            let op = inner.prepare_create(user_id, operation_id, draft, max_profiles)?;
            inner.finish(op)
        })
        .await
    }

    /// Initializes streaming data at the same UUID without creating homes/signets.
    ///
    /// # Arguments
    /// * `user_id` - Authorized account identity.
    /// * `profile_id` - Active common profile UUID.
    /// * `operation_id` - Stable operation UUID for retries.
    /// * `values` - Validated streaming preference defaults.
    ///
    /// # Errors
    /// Refuses foreign/inactive profiles, repeat initialization or pending work.
    pub async fn initialize_streaming(
        &self,
        user_id: Uuid,
        profile_id: Uuid,
        operation_id: Uuid,
        values: Value,
    ) -> Result<Operation> {
        self.run(user_id, Some(profile_id), move |inner| {
            let op = inner.prepare_initialize(user_id, profile_id, operation_id, values)?;
            inner.finish(op)
        })
        .await
    }

    /// Saves validated common/streaming documents with durable forward recovery.
    ///
    /// # Arguments
    /// * `user_id` - Authorized account identity.
    /// * `profile_id` - Active common profile UUID.
    /// * `operation_id` - Stable UUID reused after a lost response.
    /// * `common` - Optional full common replacement and expected revision.
    /// * `streaming` - Optional initialized streaming replacement and revision.
    ///
    /// # Errors
    /// Rejects stale revisions before the first write; partial commits remain
    /// discoverable/recoverable by operation ID after storage failure.
    pub async fn save_preferences(
        &self,
        user_id: Uuid,
        profile_id: Uuid,
        operation_id: Uuid,
        common: Option<SettingsChange<CommonPreferences>>,
        streaming: Option<SettingsChange<Value>>,
    ) -> Result<Operation> {
        self.run(user_id, Some(profile_id), move |inner| {
            let op = inner.prepare_save(user_id, profile_id, operation_id, common, streaming)?;
            inner.finish(op)
        })
        .await
    }

    /// Immediately blocks a profile and coordinates complete local cleanup.
    ///
    /// # Arguments
    /// * `user_id` - Authorized account identity.
    /// * `profile_id` - Owned profile to delete after UI confirmation.
    /// * `operation_id` - Stable deletion UUID.
    /// * `expected_revision` - Confirmed identity revision.
    ///
    /// # Errors
    /// Returns an ownership/revision error or a recoverable cleanup error.
    pub async fn delete_profile(
        &self,
        user_id: Uuid,
        profile_id: Uuid,
        operation_id: Uuid,
        expected_revision: i64,
    ) -> Result<Operation> {
        self.run(user_id, Some(profile_id), move |inner| {
            let op = inner.prepare_delete(user_id, profile_id, operation_id, expected_revision)?;
            inner.finish(op)
        })
        .await
    }

    /// Blocks an account, revokes its sessions and deletes both domains' data.
    ///
    /// # Arguments
    /// * `user_id` - Target account; the calling administrator service authorizes it.
    /// * `operation_id` - Stable deletion receipt UUID.
    ///
    /// # Errors
    /// Returns missing-account errors or durable recoverable cleanup failures.
    pub async fn delete_user(&self, user_id: Uuid, operation_id: Uuid) -> Result<Operation> {
        self.run(user_id, None, move |inner| {
            let op = inner.prepare_delete_user(user_id, operation_id)?;
            inner.finish(op)
        })
        .await
    }

    /// Resumes unfinished operations, prioritizing account/profile deletions.
    ///
    /// # Errors
    /// Attempts all pending operations and reports failures; application startup
    /// must not accept profile writes if recovery fails. Completed receipts remain.
    pub async fn recover(&self) -> Result<usize> {
        let inner = self.0.clone();
        let mut operations = tokio::task::spawn_blocking(move || inner.stores.common.operations())
            .await
            .context("loading profile recovery tasks")??;
        operations.retain(|op| op.state != OperationState::Completed);
        operations.sort_by_key(|op| match op.kind.as_str() {
            "delete_user" => 0,
            "delete_profile" => 1,
            _ => 2,
        });
        let mut failures = Vec::new();
        let count = operations.len();
        for operation in operations {
            let id = operation.operation_id;
            if let Err(error) = self
                .run(operation.user_id, operation.profile_id, move |inner| {
                    let current = inner
                        .stores
                        .common
                        .operation(id)?
                        .context("recovery operation disappeared")?;
                    inner.finish(current)
                })
                .await
            {
                failures.push(format!("{id}: {error:#}"));
            }
        }
        anyhow::ensure!(
            failures.is_empty(),
            "profile recovery incomplete: {}",
            failures.join("; ")
        );
        let inner = self.0.clone();
        tokio::task::spawn_blocking(move || inner.verify_links())
            .await
            .context("checking recovered profile references")??;
        Ok(count)
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

impl Inner {
    /// Executes/retries a durable intent and records contextual failures.
    fn finish(&self, mut operation: Operation) -> Result<Operation> {
        if operation.state == OperationState::Completed {
            return Ok(operation);
        }
        anyhow::ensure!(
            operation.schema_version == crate::PROFILE_DOCUMENT_VERSION,
            "unsupported profile operation schema"
        );
        operation.state = OperationState::Running;
        self.store_operation(&operation)?;
        if let Err(error) = self.apply(&mut operation) {
            operation.state = OperationState::RetryPending;
            operation.last_error = format!("{error:#}");
            // Reload the last checkpoint: a domain commit may precede a later failure.
            let mut saved = self
                .stores
                .common
                .operation(operation.operation_id)?
                .context("operation receipt missing")?;
            saved.state = OperationState::RetryPending;
            saved.last_error = operation.last_error;
            self.store_operation(&saved)
                .context("recording profile recovery failure")?;
            return Err(error).with_context(|| {
                format!(
                    "profile operation {} remains pending",
                    operation.operation_id
                )
            });
        }
        self.stores
            .common
            .operation(operation.operation_id)?
            .context("completed operation receipt missing")
    }

    /// Persists a receipt without touching service tables.
    fn store_operation(&self, operation: &Operation) -> Result<()> {
        let common = &self.stores.common;
        common
            .database
            .transaction(|tx| tx.put(&common.operations, operation))
    }

    /// Marks a receipt complete in its caller's common transaction.
    fn complete(
        &self,
        tx: &arachnea_core::persistence::SqliteTransaction<'_>,
        operation: &mut Operation,
        result: Value,
    ) -> Result<()> {
        operation.state = OperationState::Completed;
        operation.step = OperationStep::Completed;
        operation.last_error.clear();
        operation.result = result;
        tx.put(&self.stores.common.operations, operation)
    }
}

/// Returns epoch milliseconds without panicking on a pre-epoch/overflow clock.
fn now_ms() -> Result<i64> {
    Ok(i64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}

/// Builds a versioned, non-secret recovery record before domain writes.
fn operation(
    operation_id: Uuid,
    user_id: Uuid,
    profile_id: Option<Uuid>,
    kind: &str,
    request: Value,
    intent: models::Intent,
) -> Result<Operation> {
    Ok(Operation {
        operation_id,
        user_id,
        profile_id,
        schema_version: crate::PROFILE_DOCUMENT_VERSION,
        kind: kind.to_string(),
        state: OperationState::Pending,
        step: OperationStep::Prepared,
        request,
        payload: serde_json::to_value(intent)?,
        result: json!(null),
        last_error: String::new(),
    })
}
