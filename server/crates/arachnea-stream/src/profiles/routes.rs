//! Protected REST/Tauri profile APIs, embedded avatars and live administration quota.

use super::{
    accounts::{failure, guard, reject, AccountService},
    ProfileCoordinator,
};
use anyhow::Result;
use arachnea_core::controler::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};

static AVATARS: &[(&str, &str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/avatars.rs"));

/// Reports whether an avatar ID is selectable in this executable.
///
/// # Arguments
/// * `id` - Stable manifest identifier, never a filesystem path.
pub(crate) fn avatar_exists(id: &str) -> bool {
    AVATARS.iter().any(|a| a.0 == id)
}

/// Reads the quota from the authoritative successfully persisted configuration.
///
/// # Arguments
/// * `admin` - Shared configuration authority used by all administrator writes.
///
/// # Errors
/// Rejects corrupt persisted quotas rather than silently changing their meaning.
pub fn quota(admin: &arachnea_scrapyfy::admin::AdminState) -> Result<usize> {
    match admin.application_option(crate::MAX_PROFILES_PER_USER_KEY)? {
        None => Ok(crate::DEFAULT_MAX_PROFILES_PER_USER),
        Some(value) => {
            let value: u64 = value.parse()?;
            anyhow::ensure!(
                value > 0 && value <= crate::PROFILE_MAX_REVISION as u64,
                "invalid persisted profile quota"
            );
            Ok(usize::try_from(value)?)
        }
    }
}

/// Registers profile JSON operations, static avatar bytes and application quota operations.
///
/// # Arguments
/// * `controller` - Underlying REST/Tauri transport controller.
/// * `accounts` - Shared session service and native secret broker.
/// * `coordinator` - Shared profile lifecycle coordinator.
/// * `admin` - Independent administrator authorization/configuration boundary.
///
/// # Errors
/// Returns an invalid persisted quota error before starting controllers.
pub fn register(
    controller: &mut dyn ControlerService,
    accounts: Arc<AccountService>,
    coordinator: ProfileCoordinator,
    admin: Arc<arachnea_scrapyfy::admin::AdminState>,
) -> Result<()> {
    quota(&admin)?;
    let manifest: Value =
        serde_json::from_str(include_str!("../../resources/avatars/avatars.json"))?;
    let mut catalogue = json!({"schema_version":1,"fallback_avatar_id":manifest["fallback_avatar_id"],"avatars":[]});
    let mut entries = Vec::new();
    {
        let mut protected = guard::ContentController {
            inner: controller,
            accounts: accounts.clone(),
        };
        for &(id, mime, bytes) in AVATARS {
            let name = format!("profiles/avatar/{id}");
            let url = protected.stream_public_path(&name);
            entries.push(json!({"avatar_id":id,"url":url,"mime_type":mime}));
            protected.register_stream_function(
                &name,
                Arc::new(move |input| {
                    Box::pin(async move {
                        let status = if input.method == "GET" { 200 } else { 405 };
                        Ok(ControlerStreamOutput {
                            status,
                            body: ResponseBody::Buffered(if status == 200 {
                                bytes.to_vec()
                            } else {
                                Vec::new()
                            }),
                            content_type: mime.into(),
                            headers: HashMap::from([(
                                "X-Content-Type-Options".into(),
                                "nosniff".into(),
                            )]),
                        })
                    })
                }),
            );
        }
    }
    catalogue["avatars"] = json!(entries);
    for (name, write) in [
        ("profiles/list", false),
        ("profiles/get", false),
        ("profiles/create", true),
        ("profiles/update", true),
        ("profiles/delete", true),
        ("profiles/initialize_streaming", true),
        ("profiles/save_preferences", true),
        ("profiles/complete_onboarding", true),
        ("profiles/homes", false),
        ("profiles/sections", false),
        ("profiles/bookmarks", false),
        ("profiles/save_home", true),
        ("profiles/delete_home", true),
        ("profiles/reorder_homes", true),
        ("profiles/save_section", true),
        ("profiles/delete_section", true),
        ("profiles/reorder_sections", true),
        ("profiles/save_bookmark", true),
        ("profiles/delete_bookmark", true),
        ("profiles/avatars", false),
        ("admin/profile_settings", false),
        ("admin/save_profile_settings", true),
    ] {
        let accounts = accounts.clone();
        let coordinator = coordinator.clone();
        let admin = admin.clone();
        let catalogue = catalogue.clone();
        controller.register_json_function(
            name,
            Arc::new(move |input| {
                let accounts = accounts.clone();
                let coordinator = coordinator.clone();
                let admin = admin.clone();
                let catalogue = catalogue.clone();
                Box::pin(async move {
                    let result = dispatch(
                        name,
                        write,
                        input,
                        &accounts,
                        &coordinator,
                        &admin,
                        catalogue,
                    )
                    .await;
                    let (status, value) = match result {
                        Ok(data) => (200, json!({"data":data,"error":null})),
                        Err(error) => failure(&error),
                    };
                    Ok(ControlerJsonOutput {
                        status,
                        value,
                        headers: HashMap::from([("Cache-Control".into(), "no-store".into())]),
                    })
                })
            }),
        );
    }
    Ok(())
}

/// Authorizes one transport operation before invoking application or quota mutations.
async fn dispatch(
    name: &str,
    write: bool,
    input: ControlerJsonInput,
    accounts: &AccountService,
    coordinator: &ProfileCoordinator,
    admin: &arachnea_scrapyfy::admin::AdminState,
    catalogue: Value,
) -> Result<Value> {
    guard::method(&input.context, write)?;
    if name.starts_with("admin/") {
        admin
            .authorize_application_operation(&input.context, write)
            .await
            .map_err(|_| {
                reject(
                    "admin_required",
                    403,
                    "Administrator authorization is required.",
                )
            })?;
        if !write {
            return Ok(json!({"max_profiles_per_user":quota(admin)?}));
        }
        let request: QuotaUpdate = deserialize_input(input.payload)
            .map_err(|_| reject("invalid_data", 422, "Invalid profile quota request."))?;
        if request.max_profiles_per_user == 0
            || request.max_profiles_per_user > crate::PROFILE_MAX_REVISION as u64
        {
            return Err(reject(
                "invalid_data",
                422,
                "Profile quota must be a positive JSON-safe integer.",
            ));
        }
        return admin
            .update_application_options(move |config| {
                config.set_additional_option(
                    crate::MAX_PROFILES_PER_USER_KEY,
                    Some(request.max_profiles_per_user.to_string()),
                );
                config.set_additional_option("profile_quota_operations", None);
                Ok(json!({"max_profiles_per_user":request.max_profiles_per_user}))
            })
            .await;
    }
    let token = accounts.authorize(&input.context).await?;
    let auth = accounts.authenticate(token.clone(), false).await?;
    if name == "profiles/avatars" {
        return Ok(catalogue);
    }
    let payload: Value = deserialize_input(input.payload)
        .map_err(|_| reject("invalid_data", 422, "Invalid profile request."))?;
    coordinator
        .request(auth.user.id, token, name, payload, quota(admin)?)
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuotaUpdate {
    max_profiles_per_user: u64,
}
