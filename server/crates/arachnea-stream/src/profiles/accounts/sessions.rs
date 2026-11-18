//! Persistent sessions, idle expiry and mandatory password replacement.

use super::{
    blocking, credentials,
    dto::*,
    reject,
    service::{now_ms, receipt, replay, revoke_user},
    AccountService,
};
use crate::profiles::common::{
    models::{AccountState, User, UserSession},
    CommonRepository,
};
use anyhow::Result;
use arachnea_core::persistence::{EntityQuery, Field, SqliteTransaction};
use serde_json::json;
use uuid::Uuid;

/// Internal authenticated context; never serialized with secrets/hashes.
pub(crate) struct Authenticated {
    /// Internal account record, never serialized to clients.
    pub user: User,
    /// Validated session metadata.
    pub session: UserSession,
}

impl AccountService {
    /// Authenticates and renews a persisted session on every protected request.
    ///
    /// # Arguments
    /// * `token` - Opaque session secret used only in memory.
    /// * `restricted_allowed` - Whether mandatory-password sessions are permitted.
    pub(crate) async fn authenticate(
        &self,
        token: String,
        restricted_allowed: bool,
    ) -> Result<Authenticated> {
        let native_token = token.clone();
        let result = blocking(self.common.clone(), move |common| {
            common.database.transaction(|tx| {
                let auth = validate(common, tx, &token)?;
                if auth.user.must_change_password && !restricted_allowed {
                    return Err(reject(
                        "password_change_required",
                        403,
                        "Password replacement is required.",
                    ));
                }
                Ok(auth)
            })
        })
        .await;
        if result
            .as_ref()
            .err()
            .and_then(|error| error.downcast_ref::<super::AccountError>())
            .is_some_and(|error| error.status == 401)
        {
            let broker = self.native.clone();
            tokio::task::spawn_blocking(move || broker.clear(&native_token)).await??;
        }
        result
    }

    /// Checks credentials and creates a restricted persistent session atomically.
    ///
    /// # Arguments
    /// * `input` - Credential request used only in memory.
    /// * `previous` - Optional current transport secret revoked atomically on successful login.
    pub(super) async fn login(
        &self,
        input: Login,
        previous: Option<String>,
    ) -> Result<(String, Authenticated)> {
        let login = credentials::login(&input.login)
            .map_err(|_| reject("invalid_credentials", 401, "Invalid login or password."))?;
        credentials::password(&input.password)
            .map_err(|_| reject("invalid_credentials", 401, "Invalid login or password."))?;
        let dummy = self.dummy_hash.clone();
        blocking(self.common.clone(), move |common| {
            common.database.transaction(|tx| {
                let user = tx
                    .find(
                        &common.users,
                        &EntityQuery::new().where_string(&Field::string("normalized_login"), login),
                    )?
                    .into_iter()
                    .next();
                let valid = credentials::verify(
                    &input.password,
                    user.as_ref()
                        .map(|u| u.password_hash.as_str())
                        .unwrap_or(&dummy),
                );
                let user = user
                    .filter(|u| valid && u.state == AccountState::Active)
                    .ok_or_else(|| {
                        reject("invalid_credentials", 401, "Invalid login or password.")
                    })?;
                if let Some(previous) = previous {
                    for mut old in tx.find(
                        &common.sessions,
                        &EntityQuery::new().where_string(
                            &Field::string("session_digest"),
                            credentials::digest(&previous),
                        ),
                    )? {
                        old.revoked = true;
                        tx.put(&common.sessions, &old)?;
                    }
                }
                let token = credentials::token();
                let session = new_session(user.id, &token)?;
                tx.put(&common.sessions, &session)?;
                Ok((token, Authenticated { user, session }))
            })
        })
        .await
    }

    /// Revokes a session explicitly; repeated logout remains safe.
    ///
    /// # Arguments
    /// * `token` - Opaque session secret used only in memory.
    pub(super) async fn revoke(&self, token: String) -> Result<()> {
        blocking(self.common.clone(), move |common| {
            common.database.transaction(|tx| {
                for mut session in tx.find(
                    &common.sessions,
                    &EntityQuery::new().where_string(
                        &Field::string("session_digest"),
                        credentials::digest(&token),
                    ),
                )? {
                    session.revoked = true;
                    tx.put(&common.sessions, &session)?;
                }
                Ok(())
            })
        })
        .await
    }

    /// Changes the temporary password, revokes other sessions and rotates this one.
    ///
    /// # Arguments
    /// * `token` - Opaque session secret used only in memory.
    /// * `input` - Credential request used only in memory.
    pub(super) async fn change_password(
        &self,
        token: String,
        input: ChangePassword,
    ) -> Result<(String, Authenticated)> {
        let auth = self.authenticate(token.clone(), true).await?;
        let new_hash = {
            let password = input.new_password.clone();
            tokio::task::spawn_blocking(move || {
                credentials::hash(&password)
                    .map_err(|error| super::for_field("new_password", error))
            })
            .await??
        };
        self.coordinator
            .account_task(auth.user.id, move |common| {
                common.database.transaction(|tx| {
                    let mut auth = validate(common, tx, &token)?;
                    let body = json!({"user_id":auth.user.id,"session_id":auth.session.id});
                    if replay(common, tx, input.operation_id, "change_password", &body)?.is_some() {
                        return Err(reject(
                            "session_revoked",
                            401,
                            "Sign in with the new password.",
                        ));
                    }
                    if !credentials::verify(&input.current_password, &auth.user.password_hash) {
                        return Err(reject(
                            "invalid_credentials",
                            401,
                            "Invalid login or password.",
                        ));
                    }
                    auth.user.password_hash = new_hash;
                    auth.user.must_change_password = false;
                    auth.user.updated_at = now_ms()?;
                    tx.put(&common.users, &auth.user)?;
                    revoke_user(common, tx, auth.user.id)?;
                    let new_token = credentials::token();
                    auth.session = new_session(auth.user.id, &new_token)?;
                    tx.put(&common.sessions, &auth.session)?;
                    receipt(
                        common,
                        tx,
                        input.operation_id,
                        auth.user.id,
                        "change_password",
                        body,
                        json!({"changed":true}),
                    )?;
                    Ok((new_token, auth))
                })
            })
            .await
    }
}

/// Validates account/session state and extends the 30-day inactivity window.
///
/// # Arguments
/// * `common` - Authoritative account/session repository.
/// * `tx` - Common transaction, held inside the account lifecycle guard for writes.
/// * `token` - Opaque transport secret used only in memory.
///
/// # Errors
/// Rejects revoked/expired sessions and inactive accounts; returns storage errors.
pub(crate) fn validate(
    common: &CommonRepository,
    tx: &SqliteTransaction<'_>,
    token: &str,
) -> Result<Authenticated> {
    let mut session = tx
        .find(
            &common.sessions,
            &EntityQuery::new()
                .where_string(&Field::string("session_digest"), credentials::digest(token)),
        )?
        .into_iter()
        .next()
        .ok_or_else(|| reject("session_revoked", 401, "Session is no longer valid."))?;
    if session.revoked {
        return Err(reject(
            "session_revoked",
            401,
            "Session is no longer valid.",
        ));
    }
    let now = now_ms()?;
    if session.expires_at <= now {
        return Err(reject("session_expired", 401, "Session has expired."));
    }
    let user = tx
        .get(&common.users, &session.user_id.to_string())?
        .filter(|u| u.state == AccountState::Active)
        .ok_or_else(|| reject("session_revoked", 401, "Session is no longer valid."))?;
    session.last_activity_at = now;
    session.expires_at = now
        .checked_add(crate::PUBLIC_SESSION_IDLE_TTL_MS)
        .ok_or_else(|| anyhow::anyhow!("session expiry overflow"))?;
    tx.put(&common.sessions, &session)?;
    Ok(Authenticated { user, session })
}

/// Creates metadata storing only a digest of the new secret.
fn new_session(user_id: Uuid, token: &str) -> Result<UserSession> {
    let now = now_ms()?;
    Ok(UserSession {
        id: Uuid::new_v4(),
        user_id,
        session_digest: credentials::digest(token),
        expires_at: now
            .checked_add(crate::PUBLIC_SESSION_IDLE_TTL_MS)
            .ok_or_else(|| anyhow::anyhow!("session expiry overflow"))?,
        last_activity_at: now,
        revoked: false,
    })
}

/// Builds the only session state exposed to clients.
///
/// # Arguments
/// * `auth` - Validated internal session, or absence.
/// * `persistent` - Whether the transport can resume after restart.
pub(super) fn view(auth: Option<&Authenticated>, persistent: bool) -> SessionView {
    SessionView {
        authenticated: auth.is_some(),
        account: auth.map(|a| AccountView::from(&a.user)),
        expires_at: auth.map(|a| a.session.expires_at),
        persistent,
    }
}
