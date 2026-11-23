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
        name: String,
        avatar_id: String,
        common_values: CommonPreferences,
    },
    #[serde(rename = "profiles/update")]
    Update {
        profile_id: Uuid,
        expected_revision: i64,
        name: String,
        avatar_id: String,
    },
    #[serde(rename = "profiles/delete")]
    Delete {
        profile_id: Uuid,
        expected_revision: i64,
    },
    #[serde(rename = "profiles/initialize_streaming")]
    Initialize { profile_id: Uuid, values: Value },
    #[serde(rename = "profiles/save_preferences")]
    Save {
        profile_id: Uuid,
        common: Option<SettingsChange<CommonPreferences>>,
        streaming: Option<SettingsChange<Value>>,
    },
    #[serde(rename = "profiles/complete_onboarding")]
    Complete {
        profile_id: Uuid,
        scope: String,
        expected_revision: i64,
    },
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
        self.run(user_id, request.profile_id(), move |inner| {
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
                inner.dispatch(tx, user_id, request, max_profiles)
            })
        })
        .await
        .map_err(api_error)
    }
}

impl Inner {
    /// Executes one authorized request while retaining account and optional profile guards.
    fn dispatch(
        &self,
        tx: &SqliteTransaction<'_>,
        user_id: Uuid,
        request: Request,
        max: usize,
    ) -> Result<Value> {
        let common = &self.stores.common;
        match request {
            Request::List => {
                let mut profiles = tx.find(&common.profiles, &by_id("user_id", user_id))?;
                profiles.sort_by_key(|p| (p.order, p.id));
                Ok(
                    json!({"used_slots":profiles.len(), "max_profiles_per_user":max, "profiles":profiles.iter().map(profile_view).collect::<Vec<_>>()}),
                )
            }
            Request::Get { profile_id } => {
                let profile = common.active_profile(tx, user_id, profile_id)?;
                let settings = tx
                    .get(&common.settings, &profile_id.to_string())?
                    .context("common settings missing")?;
                let onboarding = tx.find(&common.onboarding, &by_id("profile_id", profile_id))?;
                let stream = tx.get(&self.stores.streaming.settings, &profile_id.to_string())?;
                Ok(
                    json!({"profile":profile_view(&profile), "common":{"schema_version":settings.schema_version,"revision":settings.revision,"values":settings.values}, "streaming":stream.map(|s| json!({"schema_version":s.schema_version,"revision":s.revision,"values":s.values})), "onboarding":onboarding}),
                )
            }
            Request::Create {
                name,
                avatar_id,
                common_values,
            } => {
                let (name, normalized_name) = validation::name(&name)?;
                avatar(&avatar_id)?;
                validation::common(&common_values)?;
                self.create(
                    tx,
                    user_id,
                    CreateProfile {
                        name,
                        normalized_name,
                        avatar_id,
                        common: common_values,
                    },
                    max,
                )
            }
            Request::Update {
                profile_id,
                expected_revision,
                name,
                avatar_id,
            } => {
                let (name, normalized_name) = validation::name(&name)?;
                avatar(&avatar_id)?;
                let mut profile = common.active_profile(tx, user_id, profile_id)?;
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
                profile.revision = next_revision(profile.revision)?;
                profile.updated_at = now_ms()?;
                tx.put(&common.profiles, &profile)?;
                Ok(profile_view(&profile))
            }
            Request::Delete {
                profile_id,
                expected_revision,
            } => {
                let profile = common.active_profile(tx, user_id, profile_id)?;
                anyhow::ensure!(profile.revision == expected_revision, "revision_conflict");
                self.purge_profile(tx, profile_id)?;
                Ok(json!({"profile_id":profile_id, "deleted":true}))
            }
            Request::Initialize { profile_id, values } => self.initialize(
                tx,
                user_id,
                profile_id,
                validation::streaming(values, true)?,
            ),
            Request::Save {
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
                self.save(tx, user_id, profile_id, change, streaming)
            }
            Request::Complete {
                profile_id,
                scope,
                expected_revision,
            } => {
                if scope != crate::PROFILE_COMMON_SCOPE && scope != crate::PROFILE_STREAM_SCOPE {
                    return Err(reject("invalid_data", 422, "Unsupported onboarding scope."));
                }
                common.active_profile(tx, user_id, profile_id)?;
                let key = PairKey([profile_id.to_string(), scope]);
                let mut guide = tx.get(&common.onboarding, &key)?.ok_or_else(|| {
                    reject("resource_not_found", 404, "Onboarding is not initialized.")
                })?;
                if guide.revision != expected_revision {
                    anyhow::bail!("revision_conflict");
                }
                guide.revision = next_revision(guide.revision)?;
                guide.state = OnboardingState::Completed;
                tx.put(&common.onboarding, &guide)?;
                Ok(serde_json::to_value(guide)?)
            }
        }
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
        } else if message == "profile_quota_reached" {
            "profile_quota_reached"
        } else if message == "profile_name_taken" {
            "profile_name_taken"
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
