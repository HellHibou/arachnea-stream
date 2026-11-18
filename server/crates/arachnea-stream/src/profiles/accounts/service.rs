//! Atomic administrator account mutations and non-secret replay receipts.

use super::{blocking, credentials, dto::*, native::NativeSessions, reject};
use crate::profiles::{
    by_id,
    common::{
        models::{AccountState, User},
        operations::{Operation, OperationState, OperationStep},
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
    /// * `stores` - Shared profile stores already recovered at startup.
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

    /// Creates an account with a temporary password, atomically with its receipt.
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
                let body = json!({
                    "login": login,
                    "must_change_password": request.must_change_password,
                });
                if let Some(result) =
                    replay(common, tx, request.operation_id, "create_user", &body)?
                {
                    return Ok(result);
                }
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
                    tx.get(&common.users, &user.id.to_string())?.is_none()
                        && tx
                            .find(&common.operations, &by_id("user_id", user.id))?
                            .is_empty(),
                    "generated account identity was already used"
                );
                tx.put(&common.users, &user)?;
                let value = serde_json::to_value(AccountView::from(&user))?;
                receipt(
                    common,
                    tx,
                    request.operation_id,
                    user.id,
                    "create_user",
                    body,
                    value.clone(),
                )?;
                Ok(value)
            })
        })
        .await
        .context("creating public account")
    }

    /// Resumes coordinated account cleanup, including a previously pending deletion.
    ///
    /// # Arguments
    /// * `request` - Administrator-authorized target and stable retry UUID.
    ///
    /// # Errors
    /// Returns missing-account, ID-conflict or recoverable cleanup failures.
    pub(super) async fn delete_user(&self, request: DeleteUser) -> Result<Value> {
        let operation_id = blocking(self.common.clone(), move |common| {
            common.database.transaction(|tx| {
                if let Some(existing) =
                    tx.get(&common.operations, &request.operation_id.to_string())?
                {
                    if existing.user_id != request.user_id || existing.kind != "delete_user" {
                        return Err(reject(
                            "operation_id_conflict",
                            409,
                            "Operation ID is already in use.",
                        ));
                    }
                    return Ok(request.operation_id);
                }
                let user = tx
                    .get(&common.users, &request.user_id.to_string())?
                    .ok_or_else(|| reject("resource_not_found", 404, "Account is inaccessible."))?;
                if user.state == AccountState::Deleting {
                    if let Some(pending) = tx
                        .find(&common.operations, &by_id("user_id", user.id))?
                        .into_iter()
                        .find(|op| {
                            op.kind == "delete_user" && op.state != OperationState::Completed
                        })
                    {
                        return Ok(pending.operation_id);
                    }
                }
                Ok(request.operation_id)
            })
        })
        .await?;
        let operation = self
            .coordinator
            .delete_user(request.user_id, operation_id)
            .await?;
        Ok(
            json!({"operation_id":operation.operation_id,"kind":operation.kind,"profile_id":null,"state":operation.state,"step":operation.step,"retryable":false,"result":operation.result}),
        )
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
                    let body = json!({"user_id":request.user_id});
                    if let Some(result) = replay(
                        common,
                        tx,
                        request.operation_id,
                        "reset_user_password",
                        &body,
                    )? {
                        return Ok(result);
                    }
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
                    receipt(
                        common,
                        tx,
                        request.operation_id,
                        user.id,
                        "reset_user_password",
                        body,
                        result.clone(),
                    )?;
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
    /// Returns missing-account, ID-conflict or recoverable storage failures.
    pub(super) async fn set_password_change(&self, request: SetPasswordChange) -> Result<Value> {
        self.coordinator
            .account_task(request.user_id, move |common| {
                common.database.transaction(|tx| {
                    let body = json!({
                        "user_id": request.user_id,
                        "must_change_password": request.must_change_password,
                    });
                    if let Some(result) = replay(
                        common,
                        tx,
                        request.operation_id,
                        "set_user_password_change",
                        &body,
                    )? {
                        return Ok(result);
                    }
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
                    receipt(
                        common,
                        tx,
                        request.operation_id,
                        user.id,
                        "set_user_password_change",
                        body,
                        result.clone(),
                    )?;
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

/// Reads an atomic replay receipt without retaining password-derived request data.
///
/// # Arguments
/// * `common` - Common account repository.
/// * `tx` - Current local common-store transaction.
/// * `id` - Stable operation UUID.
/// * `kind` - Stable local operation name.
/// * `body` - Non-secret normalized request metadata.
pub(super) fn replay(
    common: &CommonRepository,
    tx: &SqliteTransaction<'_>,
    id: Uuid,
    kind: &str,
    body: &Value,
) -> Result<Option<Value>> {
    match tx.get(&common.operations, &id.to_string())? {
        Some(op)
            if op.kind == kind && &op.request == body && op.state == OperationState::Completed =>
        {
            Ok(Some(op.result))
        }
        Some(_) => Err(reject(
            "operation_id_conflict",
            409,
            "Operation ID is already in use.",
        )),
        None => Ok(None),
    }
}

/// Persists a completed local mutation receipt in the same transaction.
///
/// # Arguments
/// * `common` - Common account repository.
/// * `tx` - Current local common-store transaction.
/// * `id` - Stable operation UUID.
/// * `user_id` - Target account UUID.
/// * `kind` - Stable local operation name.
/// * `body` - Non-secret normalized request metadata.
/// * `result` - Safe confirmed result.
pub(super) fn receipt(
    common: &CommonRepository,
    tx: &SqliteTransaction<'_>,
    id: Uuid,
    user_id: Uuid,
    kind: &str,
    body: Value,
    result: Value,
) -> Result<()> {
    tx.put(
        &common.operations,
        &Operation {
            operation_id: id,
            user_id,
            profile_id: None,
            schema_version: crate::PROFILE_DOCUMENT_VERSION,
            kind: kind.to_string(),
            state: OperationState::Completed,
            step: OperationStep::Completed,
            request: body,
            payload: json!({}),
            result,
            last_error: String::new(),
        },
    )
}
