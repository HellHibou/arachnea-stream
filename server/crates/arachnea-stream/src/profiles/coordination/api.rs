//! Authenticated application operations under the shared account/profile lifecycle guard.

use super::*;
use crate::profiles::{
    accounts::{reject, sessions},
    by_id,
    common::models::{CommonPreferences, OnboardingState, Profile},
    keys::PairKey,
    validation,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "command", deny_unknown_fields)]
enum Request {
    #[serde(rename = "profiles/list")]
    List,
    #[serde(rename = "profiles/get")]
    Get { profile_id: Uuid },
    #[serde(rename = "profiles/create")]
    Create {
        operation_id: Uuid,
        name: String,
        avatar_id: String,
        common_values: CommonPreferences,
    },
    #[serde(rename = "profiles/update")]
    Update {
        operation_id: Uuid,
        profile_id: Uuid,
        expected_revision: i64,
        name: String,
        avatar_id: String,
    },
    #[serde(rename = "profiles/delete")]
    Delete {
        operation_id: Uuid,
        profile_id: Uuid,
        expected_revision: i64,
    },
    #[serde(rename = "profiles/initialize_streaming")]
    Initialize {
        operation_id: Uuid,
        profile_id: Uuid,
        values: Value,
    },
    #[serde(rename = "profiles/save_preferences")]
    Save {
        operation_id: Uuid,
        profile_id: Uuid,
        common: Option<SettingsChange<CommonPreferences>>,
        streaming: Option<SettingsChange<Value>>,
    },
    #[serde(rename = "profiles/complete_onboarding")]
    Complete {
        operation_id: Uuid,
        profile_id: Uuid,
        scope: String,
        expected_revision: i64,
    },
    #[serde(rename = "profiles/operation")]
    Operation { operation_id: Uuid },
}

impl Request {
    fn profile_id(&self) -> Option<Uuid> {
        match self {
            Self::Get { profile_id }
            | Self::Update { profile_id, .. }
            | Self::Delete { profile_id, .. }
            | Self::Initialize { profile_id, .. }
            | Self::Save { profile_id, .. }
            | Self::Complete { profile_id, .. } => Some(*profile_id),
            _ => None,
        }
    }
}

impl ProfileCoordinator {
    /// Dispatches a validated profile request and rechecks its session inside lifecycle guards.
    ///
    /// # Arguments
    /// * `user_id` - Account established by the transport's first authentication.
    /// * `token` - Transport secret, revalidated before reading or mutating stores.
    /// * `command` - Registered profile operation name.
    /// * `payload` - Strict operation-specific DTO without caller-provided ownership.
    /// * `max_profiles` - Latest successfully persisted server quota.
    ///
    /// # Errors
    /// Returns safe validation/ownership/revision errors or contextual storage errors.
    pub(crate) async fn request(
        &self,
        user_id: Uuid,
        token: String,
        command: &str,
        mut payload: Value,
        max_profiles: usize,
    ) -> Result<Value> {
        if super::streaming_api::is_streaming_command(command) {
            return self
                .streaming_request(user_id, token, command.to_owned(), payload)
                .await;
        }
        let object = payload
            .as_object_mut()
            .ok_or_else(|| reject("invalid_data", 422, "Invalid profile request."))?;
        if object.contains_key("command") {
            return Err(reject("invalid_data", 422, "Unexpected command field."));
        }
        object.insert("command".into(), json!(command));
        let request: Request = serde_json::from_value(payload)
            .map_err(|_| reject("invalid_data", 422, "Invalid profile request fields."))?;
        let mutation_id = match &request {
            Request::Create { operation_id, .. }
            | Request::Update { operation_id, .. }
            | Request::Delete { operation_id, .. }
            | Request::Initialize { operation_id, .. }
            | Request::Save { operation_id, .. }
            | Request::Complete { operation_id, .. } => Some(*operation_id),
            _ => None,
        };
        self.run(user_id, request.profile_id(), move |inner| {
            inner.stores.common.database.transaction(|tx| {
                let auth = sessions::validate(&inner.stores.common, tx, &token)?;
                if auth.user.id != user_id || auth.user.must_change_password {
                    return Err(reject(
                        "password_change_required",
                        403,
                        "Unrestricted session required.",
                    ));
                }
                Ok(())
            })?;
            if let Some(id) = mutation_id {
                if inner.stores.streaming.mutation(id)?.is_some() {
                    return Err(reject(
                        "operation_id_conflict",
                        409,
                        "Operation ID is already in use.",
                    ));
                }
            }
            inner.dispatch(user_id, request, max_profiles)
        })
        .await
        .map_err(api_error)
    }
}

impl Inner {
    /// Executes one authorized request while retaining account and optional profile guards.
    fn dispatch(&self, user_id: Uuid, request: Request, max: usize) -> Result<Value> {
        let common = &self.stores.common;
        match request {
            Request::List => {
                let mut profiles = common.profiles(user_id)?;
                profiles.sort_by_key(|p| (p.order, p.id));
                Ok(
                    json!({"used_slots":profiles.len(), "max_profiles_per_user":max, "profiles":profiles.iter().map(profile_view).collect::<Vec<_>>()}),
                )
            }
            Request::Get { profile_id } => {
                let (profile, settings, onboarding, pending) =
                    common.database.transaction(|tx| {
                        let profile = common.active_profile(tx, user_id, profile_id)?;
                        let settings = tx
                            .get(&common.settings, &profile_id.to_string())?
                            .context("common settings missing")?;
                        let onboarding =
                            tx.find(&common.onboarding, &by_id("profile_id", profile_id))?;
                        let pending = tx
                            .find(&common.operations, &by_id("profile_id", profile_id))?
                            .into_iter()
                            .find(|op| op.state != OperationState::Completed);
                        Ok((profile, settings, onboarding, pending))
                    })?;
                let stream = self.stores.streaming.settings(profile_id)?;
                Ok(
                    json!({"profile":profile_view(&profile), "common":{"schema_version":settings.schema_version,"revision":settings.revision,"values":settings.values}, "streaming":stream.map(|s| json!({"schema_version":s.schema_version,"revision":s.revision,"values":s.values})), "onboarding":onboarding, "pending_operation":pending.as_ref().map(operation_view)}),
                )
            }
            Request::Create {
                operation_id,
                name,
                avatar_id,
                common_values,
            } => {
                let (name, normalized_name) = validation::name(&name)?;
                avatar(&avatar_id)?;
                validation::common(&common_values)?;
                let op = self.prepare_create(
                    user_id,
                    operation_id,
                    CreateProfile {
                        name,
                        normalized_name,
                        avatar_id,
                        common: common_values,
                    },
                    max,
                )?;
                Ok(operation_view(&self.finish(op)?))
            }
            Request::Update {
                operation_id,
                profile_id,
                expected_revision,
                name,
                avatar_id,
            } => {
                let (name, normalized_name) = validation::name(&name)?;
                avatar(&avatar_id)?;
                let body = json!({"kind":"update_profile", "expected_revision":expected_revision,"name":name,"avatar_id":avatar_id});
                common.database.transaction(|tx| {
                    let mut profile = common.active_profile(tx, user_id, profile_id)?;
                    if let Some(op) =
                        self.existing(tx, operation_id, user_id, Some(profile_id), &body)?
                    {
                        return Ok(op.result);
                    }
                    self.no_pending(tx, profile_id)?;
                    if profile.revision != expected_revision {
                        anyhow::bail!("revision_conflict");
                    }
                    if tx
                        .find(&common.profiles, &by_id("user_id", user_id))?
                        .iter()
                        .any(|p| p.id != profile_id && p.normalized_name == normalized_name)
                    {
                        anyhow::bail!("profile_name_taken");
                    }
                    profile.name = name;
                    profile.normalized_name = normalized_name;
                    profile.avatar_id = avatar_id;
                    profile.revision = prepare::next_revision(profile.revision)?;
                    profile.updated_at = now_ms()?;
                    tx.put(&common.profiles, &profile)?;
                    let value = profile_view(&profile);
                    self.local_receipt(
                        tx,
                        operation_id,
                        user_id,
                        profile_id,
                        "update_profile",
                        body,
                        value.clone(),
                    )?;
                    Ok(value)
                })
            }
            Request::Delete {
                operation_id,
                profile_id,
                expected_revision,
            } => {
                let op =
                    self.prepare_delete(user_id, profile_id, operation_id, expected_revision)?;
                Ok(operation_view(&self.finish(op)?))
            }
            Request::Initialize {
                operation_id,
                profile_id,
                values,
            } => {
                let values = validation::streaming(values, true)?;
                self.home_reference(user_id, profile_id, &values)?;
                let op = self.prepare_initialize(user_id, profile_id, operation_id, values)?;
                Ok(operation_view(&self.finish(op)?))
            }
            Request::Save {
                operation_id,
                profile_id,
                common: change,
                streaming,
            } => {
                if change.is_none() && streaming.is_none() {
                    return Err(reject(
                        "invalid_data",
                        422,
                        "At least one settings domain is required.",
                    ));
                }
                if let Some(change) = &change {
                    validation::common(&change.values)?;
                }
                let streaming = streaming
                    .map(|s| {
                        validation::streaming(s.values, false).map(|values| SettingsChange {
                            expected_revision: s.expected_revision,
                            values,
                        })
                    })
                    .transpose()?;
                if let Some(change) = &streaming {
                    self.home_reference(user_id, profile_id, &change.values)?;
                }
                let op = self.prepare_save(user_id, profile_id, operation_id, change, streaming)?;
                Ok(operation_view(&self.finish(op)?))
            }
            Request::Complete {
                operation_id,
                profile_id,
                scope,
                expected_revision,
            } => {
                if scope != crate::PROFILE_COMMON_SCOPE && scope != crate::PROFILE_STREAM_SCOPE {
                    return Err(reject("invalid_data", 422, "Unsupported onboarding scope."));
                }
                let body = json!({"kind":"complete_onboarding","scope":scope,"expected_revision":expected_revision});
                common.database.transaction(|tx| {
                    common.active_profile(tx, user_id, profile_id)?;
                    if let Some(op) =
                        self.existing(tx, operation_id, user_id, Some(profile_id), &body)?
                    {
                        return Ok(op.result);
                    }
                    self.no_pending(tx, profile_id)?;
                    let key = PairKey([profile_id.to_string(), scope]);
                    let mut guide = tx.get(&common.onboarding, &key)?.ok_or_else(|| {
                        reject("resource_not_found", 404, "Onboarding is not initialized.")
                    })?;
                    if guide.revision != expected_revision {
                        anyhow::bail!("revision_conflict");
                    }
                    guide.revision = prepare::next_revision(guide.revision)?;
                    guide.state = OnboardingState::Completed;
                    tx.put(&common.onboarding, &guide)?;
                    let value = serde_json::to_value(guide)?;
                    self.local_receipt(
                        tx,
                        operation_id,
                        user_id,
                        profile_id,
                        "complete_onboarding",
                        body,
                        value.clone(),
                    )?;
                    Ok(value)
                })
            }
            Request::Operation { operation_id } => {
                if common.operation(operation_id)?.is_none() {
                    if let Some(op) = self.stores.streaming.mutation(operation_id)? {
                        common
                            .database
                            .transaction(|tx| common.active_profile(tx, user_id, op.profile_id))?;
                        return Ok(
                            json!({"operation_id":op.operation_id,"kind":op.kind,"profile_id":op.profile_id,"state":"completed","step":"completed","retryable":false,"result":op.result}),
                        );
                    }
                }
                let op = common
                    .operation(operation_id)?
                    .filter(|op| op.user_id == user_id && op.profile_id.is_some())
                    .ok_or_else(|| {
                        reject("resource_not_found", 404, "Operation is inaccessible.")
                    })?;
                // Deletion receipts survive cleanup; other receipts require active ownership.
                if op.kind != "delete_profile" {
                    common.database.transaction(|tx| {
                        common.active_profile(tx, user_id, op.profile_id.unwrap())
                    })?;
                }
                Ok(operation_view(&op))
            }
        }
    }

    /// Refuses references to homes outside the active profile before preparing an intent.
    fn home_reference(&self, user_id: Uuid, profile_id: Uuid, values: &Value) -> Result<()> {
        self.stores
            .common
            .database
            .transaction(|tx| self.stores.common.active_profile(tx, user_id, profile_id))?;
        if let Some(id) = values["defaultHomeId"].as_str() {
            let streaming = &self.stores.streaming;
            let exists = streaming
                .database
                .transaction(|tx| {
                    tx.get(
                        &streaming.homes,
                        &PairKey([profile_id.to_string(), id.into()]),
                    )
                })?
                .is_some();
            if !exists {
                return Err(reject(
                    "invalid_data",
                    422,
                    "Default home must belong to this profile.",
                ));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    /// Stores a completed single-store mutation and result in the caller's transaction.
    fn local_receipt(
        &self,
        tx: &arachnea_core::persistence::SqliteTransaction<'_>,
        id: Uuid,
        user_id: Uuid,
        profile_id: Uuid,
        kind: &str,
        request: Value,
        result: Value,
    ) -> Result<()> {
        let mut op = Operation {
            operation_id: id,
            user_id,
            profile_id: Some(profile_id),
            schema_version: crate::PROFILE_DOCUMENT_VERSION,
            kind: kind.into(),
            state: OperationState::Pending,
            step: OperationStep::Prepared,
            request,
            payload: json!({}),
            result: Value::Null,
            last_error: String::new(),
        };
        self.complete(tx, &mut op, result)
    }
}

fn avatar(id: &str) -> Result<()> {
    if !crate::profiles::routes::avatar_exists(id) {
        return Err(crate::profiles::accounts::for_field(
            "avatar_id",
            reject("invalid_data", 422, "Unknown avatar."),
        ));
    }
    Ok(())
}

fn profile_view(p: &Profile) -> Value {
    json!({"profile_id":p.id,"user_id":p.user_id,"name":p.name,"avatar_id":p.avatar_id,"order":p.order,"state":p.state,"revision":p.revision,"created_at":p.created_at,"updated_at":p.updated_at})
}

fn operation_view(op: &Operation) -> Value {
    json!({"operation_id":op.operation_id,"kind":op.kind,"profile_id":op.profile_id,"state":op.state,"step":op.step,"retryable":op.state != OperationState::Completed,"result":op.result})
}

/// Maps known domain rejections without converting arbitrary storage failures to client errors.
///
/// # Arguments
/// * `error` - Contextual authorization/domain/storage failure.
pub(super) fn api_error(error: anyhow::Error) -> anyhow::Error {
    for cause in error.chain() {
        let message = cause.to_string();
        let code = if message.starts_with("revision_conflict") {
            "revision_conflict"
        } else if message == "profile is inaccessible" {
            "profile_inaccessible"
        } else if message == "streaming profile is not initialized" {
            "invalid_data"
        } else if message == "streaming profile already initialized" {
            "invalid_data"
        } else if [
            "operation_id_conflict",
            "operation_in_progress",
            "profile_quota_reached",
            "profile_name_taken",
        ]
        .contains(&message.as_str())
        {
            match message.as_str() {
                "operation_id_conflict" => "operation_id_conflict",
                "operation_in_progress" => "operation_in_progress",
                "profile_quota_reached" => "profile_quota_reached",
                _ => "profile_name_taken",
            }
        } else {
            continue;
        };
        return reject(
            code,
            match code {
                "profile_inaccessible" => 404,
                "invalid_data" => 422,
                _ => 409,
            },
            code,
        );
    }
    error
}
