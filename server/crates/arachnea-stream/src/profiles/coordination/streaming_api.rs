//! Shared lifecycle authorization for streaming resource reads and local transactions.

use super::*;
use crate::profiles::accounts::{reject, sessions};

/// Identifies the supported personal streaming endpoints without accepting arbitrary commands.
///
/// # Arguments
/// * `command` - Registered controller command.
pub(super) fn is_streaming_command(command: &str) -> bool {
    matches!(
        command,
        "profiles/homes"
            | "profiles/sections"
            | "profiles/bookmarks"
            | "profiles/save_home"
            | "profiles/delete_home"
            | "profiles/reorder_homes"
            | "profiles/save_section"
            | "profiles/delete_section"
            | "profiles/reorder_sections"
            | "profiles/save_bookmark"
            | "profiles/delete_bookmark"
    )
}

impl ProfileCoordinator {
    /// Revalidates session and ownership in the transaction handling the streaming request.
    ///
    /// # Arguments
    /// * `user_id` - Transport-authenticated account.
    /// * `token` - Session secret retained only in memory.
    /// * `command` - Whitelisted streaming operation.
    /// * `payload` - Strict operation DTO containing the requested profile UUID.
    pub(super) async fn streaming_request(
        &self,
        user_id: Uuid,
        token: String,
        command: String,
        payload: Value,
    ) -> Result<Value> {
        let profile_id = payload
            .get("profile_id")
            .and_then(Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or_else(|| reject("invalid_data", 422, "Profile UUID is required."))?;
        self.run(user_id, Some(profile_id), move |inner| {
            let common = &inner.stores.common;
            common.database.transaction(|tx| {
                let auth = sessions::validate(common, tx, &token)?;
                if auth.user.id != user_id || auth.user.must_change_password {
                    return Err(reject(
                        "password_change_required",
                        403,
                        "Unrestricted session required.",
                    ));
                }
                common.active_profile(tx, user_id, profile_id)?;
                inner.stores.streaming.request(tx, &command, payload)
            })
        })
        .await
        .map_err(super::api::api_error)
    }
}
