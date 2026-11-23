//! Atomic administrator account mutations.

use super::{blocking, credentials, dto::*, native::NativeSessions, reject};
use crate::profiles::{
    by_id,
    common::{
        models::{AccountState, User},
        CommonRepository,
    },
    ProfileCoordinator, ProfileStores,
};
use anyhow::{Context, Result};
use arachnea_core::persistence::{EntityQuery, Field, SqliteTransaction};
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

/// Public-account application service; admin/provider credentials remain separate.
#[derive(Clone)]
pub struct AccountService {
    /// Shared common account repository.
    pub(super) common: Arc<CommonRepository>,
    /// Shared account/profile lifecycle coordinator.
    pub(super) coordinator: ProfileCoordinator,
    /// Native session broker, restricted to trusted public callers.
    pub(super) native: Arc<NativeSessions>,
    /// Login limiter independent of administrator sessions.
    pub(super) limiter: Arc<arachnea_scrapyfy::admin::auth::LoginRateLimiter>,
    /// Dummy Argon2id hash for missing-account verification.
    pub(super) dummy_hash: String,
}

impl AccountService {
    /// Composes account storage, lifecycle coordination and an installation vault.
    ///
    /// # Arguments
    /// * `stores` - Shared profile repositories verified at startup.
    /// * `coordinator` - Coordinator sharing the same lifecycle guards.
    /// * `installation` - Stable installation/server data-directory identity.
    ///
    /// # Errors
    /// Returns an error if the dummy password hash cannot be generated.
    pub fn new(
        stores: &ProfileStores,
        coordinator: ProfileCoordinator,
        installation: &str,
    ) -> Result<Self> {
        Ok(Self {
            common: stores.common.clone(),
            coordinator,
            native: Arc::new(NativeSessions::new(installation)),
            limiter: Arc::new(arachnea_scrapyfy::admin::auth::LoginRateLimiter::new()),
            dummy_hash: credentials::hash(&credentials::token())?,
        })
    }

    /// Returns safe account rows for the authorized administrator adapter.
    ///
    /// # Errors
    /// Returns contextual storage errors, never password data.
    pub async fn users(&self) -> Result<Value> {
        blocking(self.common.clone(), |common| common.database.transaction(|tx| {
            let mut users = tx.find(&common.users, &EntityQuery::new())?;
            users.sort_by(|a,b| a.normalized_login.cmp(&b.normalized_login));
            Ok(json!({"users":users.iter().map(|user| json!({"user_id":user.id,"login":user.normalized_login,"must_change_password":user.must_change_password,"state":user.state,"created_at":user.created_at})).collect::<Vec<_>>()}))
        })).await
    }

    /// Creates an account with a temporary password.
    ///
    /// # Arguments
    /// * `request` - Validated request, with credentials used only in memory.
    pub(super) async fn create_user(&self, request: CreateUser) -> Result<Value> {
        let login = credentials::login(&request.login)?;
        let hash = tokio::task::spawn_blocking(move || {
            credentials::hash(&request.temporary_password)
                .map_err(|error| super::for_field("temporary_password", error))
        })
        .await??;
        blocking(self.common.clone(), move |common| {
            common.database.transaction(|tx| {
                if !tx
                    .find(
                        &common.users,
                        &EntityQuery::new()
                            .where_string(&Field::string("normalized_login"), &login),
                    )?
                    .is_empty()
                {
                    return Err(reject("login_taken", 409, "This login is already in use."));
                }
                let user = User {
                    id: Uuid::new_v4(),
                    normalized_login: login,
                    password_hash: hash,
                    must_change_password: request.must_change_password,
                    state: AccountState::Active,
                    created_at: now_ms()?,
                    updated_at: now_ms()?,
                };
                anyhow::ensure!(
                    tx.get(&common.users, &user.id.to_string())?.is_none(),
                    "generated account identity was already used"
                );
                tx.put(&common.users, &user)?;
                Ok(serde_json::to_value(AccountView::from(&user))?)
            })
        })
        .await
        .context("creating public account")
    }

    /// Deletes an account and its dependent profile data.
    ///
    /// # Arguments
    /// * `request` - Administrator-authorized target.
    pub(super) async fn delete_user(&self, request: DeleteUser) -> Result<Value> {
        self.coordinator.delete_user(request.user_id).await
    }

    /// Replaces a password and revokes every session under the account guard.
    ///
    /// # Arguments
    /// * `request` - Validated request, with credentials used only in memory.
    pub(super) async fn reset_password(&self, request: ResetPassword) -> Result<Value> {
        let hash = tokio::task::spawn_blocking(move || {
            credentials::hash(&request.temporary_password)
                .map_err(|error| super::for_field("temporary_password", error))
        })
        .await??;
        self.coordinator
            .account_task(request.user_id, move |common| {
                common.database.transaction(|tx| {
                    let mut user = tx
                        .get(&common.users, &request.user_id.to_string())?
                        .ok_or_else(|| {
                            reject("resource_not_found", 404, "Account is inaccessible.")
                        })?;
                    if user.state != AccountState::Active {
                        return Err(reject(
                            "resource_not_found",
                            404,
                            "Account is inaccessible.",
                        ));
                    }
                    user.password_hash = hash;
                    user.must_change_password = true;
                    user.updated_at = now_ms()?;
                    tx.put(&common.users, &user)?;
                    revoke_user(common, tx, user.id)?;
                    let result = json!({"user_id":user.id,"must_change_password":true});
                    Ok(result)
                })
            })
            .await
            .context("resetting public account password")
    }

    /// Updates the mandatory password replacement flag without touching credentials.
    ///
    /// # Arguments
    /// * `request` - Administrator-authorized target, requested flag and stable retry UUID.
    ///
    /// # Errors
    /// Returns missing-account or storage failures.
    pub(super) async fn set_password_change(&self, request: SetPasswordChange) -> Result<Value> {
        self.coordinator
            .account_task(request.user_id, move |common| {
                common.database.transaction(|tx| {
                    let mut user = tx
                        .get(&common.users, &request.user_id.to_string())?
                        .ok_or_else(|| {
                            reject("resource_not_found", 404, "Account is inaccessible.")
                        })?;
                    if user.state != AccountState::Active {
                        return Err(reject(
                            "resource_not_found",
                            404,
                            "Account is inaccessible.",
                        ));
                    }
                    user.must_change_password = request.must_change_password;
                    user.updated_at = now_ms()?;
                    tx.put(&common.users, &user)?;
                    let result = json!({
                        "user_id": user.id,
                        "must_change_password": user.must_change_password,
                    });
                    Ok(result)
                })
            })
            .await
            .context("updating public account password change flag")
    }
}

/// Returns epoch milliseconds with checked conversion.
pub(super) fn now_ms() -> Result<i64> {
    Ok(i64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}

/// Revokes every session in the same transaction as a credential mutation.
///
/// # Arguments
/// * `common` - Common account repository.
/// * `tx` - Current local common-store transaction.
/// * `user_id` - Target account UUID.
pub(super) fn revoke_user(
    common: &CommonRepository,
    tx: &SqliteTransaction<'_>,
    user_id: Uuid,
) -> Result<()> {
    for mut session in tx.find(&common.sessions, &by_id("user_id", user_id))? {
        session.revoked = true;
        tx.put(&common.sessions, &session)?;
    }
    Ok(())
}
