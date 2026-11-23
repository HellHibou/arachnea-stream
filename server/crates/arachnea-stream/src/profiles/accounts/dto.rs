//! Account transport DTOs excluding all password hashes and session secrets.

use crate::profiles::common::models::User;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Safe public identity shared by session and administrator responses.
#[derive(Clone, Serialize)]
pub struct AccountView {
    /// Stable account UUID.
    pub user_id: Uuid,
    /// Canonical login identifier.
    pub login: String,
    /// Whether content remains locked until password replacement.
    pub must_change_password: bool,
}
impl From<&User> for AccountView {
    fn from(user: &User) -> Self {
        Self {
            user_id: user.id,
            login: user.normalized_login.clone(),
            must_change_password: user.must_change_password,
        }
    }
}

/// Session state, never including the underlying authentication secret.
#[derive(Serialize)]
pub struct SessionView {
    /// Whether a persisted session is valid.
    pub authenticated: bool,
    /// Account identity for an authenticated session.
    pub account: Option<AccountView>,
    /// Idle expiration as epoch milliseconds.
    pub expires_at: Option<i64>,
    /// Whether the transport can resume after application restart.
    pub persistent: bool,
}

/// Login request, deliberately without an idempotency receipt or plaintext log.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Login {
    /// Normalized login identifier.
    pub login: String,
    /// Plaintext input used in memory only.
    pub password: String,
}

/// Mandatory password replacement request.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ChangePassword {
    /// Current password used for verification only.
    pub current_password: String,
    /// Replacement password used in memory only.
    pub new_password: String,
}

/// Account creation by an authorized administrator.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateUser {
    /// Normalized login identifier.
    pub login: String,
    /// Administrator-supplied temporary password, never persisted in plaintext.
    pub temporary_password: String,
    /// Whether the account must replace the temporary password at the next sign-in.
    #[serde(default = "require_password_change")]
    pub must_change_password: bool,
}

/// Administrator change of one account's mandatory password replacement flag.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetPasswordChange {
    /// Target account UUID.
    pub user_id: Uuid,
    /// Whether the account must replace its password at the next sign-in.
    pub must_change_password: bool,
}

/// Keeps omitted creation requests on the historical mandatory replacement rule.
fn require_password_change() -> bool {
    true
}

/// Administrator reset of one account's temporary password.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResetPassword {
    /// Target account UUID.
    pub user_id: Uuid,
    /// Administrator-supplied temporary password, never persisted in plaintext.
    pub temporary_password: String,
}

/// Administrator account deletion.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeleteUser {
    /// Target account UUID.
    pub user_id: Uuid,
}

/// Session logout request.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Logout {}
