//! REST/Tauri account routes and existing-admin-authorized user management.

use super::{dto::*, failure, guard, reject, sessions, AccountService};
use anyhow::Result;
use arachnea_core::controler::*;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};

/// Registers public session operations and administrator-only account operations.
///
/// # Arguments
/// * `controller` - Underlying controller, outside the content decorator.
/// * `accounts` - Shared persistent account/session service.
/// * `admin` - Existing independent administrator authorization boundary.
pub fn register(
    controller: &mut dyn ControlerService,
    accounts: Arc<AccountService>,
    admin: Arc<arachnea_scrapyfy::admin::AdminState>,
) {
    for (name, write) in [
        ("accounts/login", true),
        ("accounts/session", false),
        ("accounts/change_password", true),
        ("accounts/logout", true),
        ("admin/users", false),
        ("admin/create_user", true),
        ("admin/delete_user", true),
        ("admin/reset_user_password", true),
        ("admin/set_user_password_change", true),
    ] {
        let accounts = accounts.clone();
        let admin = admin.clone();
        controller.register_json_function(
            name,
            Arc::new(move |input| {
                let accounts = accounts.clone();
                let admin = admin.clone();
                Box::pin(async move {
                    let mut headers = HashMap::from([("Cache-Control".into(), "no-store".into())]);
                    let response =
                        dispatch(name, write, &accounts, &admin, input, &mut headers).await;
                    let (status, value) = match response {
                        Ok((status, data)) => (status, json!({"data":data,"error":null})),
                        Err(error) => failure(&error),
                    };
                    Ok(ControlerJsonOutput {
                        value,
                        status,
                        headers,
                    })
                })
            }),
        );
    }
}

/// Handles one account operation after checking method/origin and authorization.
async fn dispatch(
    name: &str,
    write: bool,
    accounts: &AccountService,
    admin: &arachnea_scrapyfy::admin::AdminState,
    input: ControlerJsonInput,
    headers: &mut HashMap<String, String>,
) -> Result<(u16, Value)> {
    let context = &input.context;
    guard::method(context, write)?;
    if name.starts_with("admin/") {
        admin
            .authorize_application_operation(context, write)
            .await
            .map_err(|_| {
                reject(
                    "admin_required",
                    403,
                    "Administrator authorization is required.",
                )
            })?;
        let data = match name {
            "admin/users" => accounts.users().await?,
            "admin/create_user" => accounts.create_user(parse(input.payload)?).await?,
            "admin/reset_user_password" => accounts.reset_password(parse(input.payload)?).await?,
            "admin/set_user_password_change" => {
                accounts.set_password_change(parse(input.payload)?).await?
            }
            "admin/delete_user" => accounts.delete_user(parse(input.payload)?).await?,
            _ => unreachable!(),
        };
        return Ok((200, data));
    }
    let peer = context
        .remote_addr()
        .map(|address| address.ip())
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    if matches!(name, "accounts/login" | "accounts/change_password")
        && accounts.limiter.is_limited(peer)
    {
        return Err(reject(
            "login_rate_limited",
            429,
            "Too many failed login attempts. Try again later.",
        ));
    }
    let (current, persistent) = accounts.secret(context).await?;
    match name {
        "accounts/session" => {
            let auth = match current.clone() {
                Some(token) => Some(accounts.authenticate(token, true).await?),
                None => None,
            };
            if let Some(token) = current.filter(|_| {
                context.native_webview().is_none() && guard::browser_origin(context).is_some()
            }) {
                headers.insert(
                    "Set-Cookie".into(),
                    guard::session_cookie(&token, context, name),
                );
            }
            Ok((
                200,
                serde_json::to_value(sessions::view(auth.as_ref(), persistent))?,
            ))
        }
        "accounts/login" => {
            let result = accounts.login(parse(input.payload)?, current).await;
            let (token, auth) = match result {
                Ok(result) => {
                    accounts.limiter.clear(peer);
                    result
                }
                Err(error) => {
                    accounts.limiter.record_failure(peer);
                    return Err(error);
                }
            };
            let persistent = bind(accounts, context, name, headers, auth.user.id, token).await?;
            Ok((
                200,
                serde_json::to_value(sessions::view(Some(&auth), persistent))?,
            ))
        }
        "accounts/change_password" => {
            let token = current.ok_or_else(|| {
                reject(
                    "session_required",
                    401,
                    "Public account authentication is required.",
                )
            })?;
            let (token, auth) = match accounts.change_password(token, parse(input.payload)?).await {
                Ok(result) => {
                    accounts.limiter.clear(peer);
                    result
                }
                Err(error) => {
                    if error
                        .downcast_ref::<super::AccountError>()
                        .is_some_and(|error| error.code == "invalid_credentials")
                    {
                        accounts.limiter.record_failure(peer);
                    }
                    return Err(error);
                }
            };
            let persistent = bind(accounts, context, name, headers, auth.user.id, token).await?;
            Ok((
                200,
                serde_json::to_value(sessions::view(Some(&auth), persistent))?,
            ))
        }
        "accounts/logout" => {
            let _: Logout = parse(input.payload)?;
            if let Some(token) = current {
                accounts.revoke(token.clone()).await?;
                let native = accounts.native.clone();
                tokio::task::spawn_blocking(move || native.clear(&token)).await??;
                if context.native_webview().is_some() {
                    headers.insert(
                        arachnea_core::CONTROLLER_NATIVE_EVENT_HEADER.into(),
                        crate::PUBLIC_SESSION_CHANGED_EVENT.into(),
                    );
                }
            }
            if context.native_webview().is_none() {
                headers.insert(
                    "Set-Cookie".into(),
                    guard::session_cookie("", context, name),
                );
            }
            Ok((200, json!({"revoked":true})))
        }
        _ => unreachable!(),
    }
}

/// Parses a validated JSON/query payload without exposing serde internals.
fn parse<T: serde::de::DeserializeOwned>(input: ControlerFunctionInput) -> Result<T> {
    deserialize_input(input).map_err(|_| reject("invalid_data", 422, "Invalid account request."))
}

/// Delivers a fresh secret through HttpOnly cookies or the native broker only.
async fn bind(
    accounts: &AccountService,
    context: &RequestControlerContext,
    name: &str,
    headers: &mut HashMap<String, String>,
    user_id: uuid::Uuid,
    token: String,
) -> Result<bool> {
    if context.native_webview().is_some() {
        headers.insert(
            arachnea_core::CONTROLLER_NATIVE_EVENT_HEADER.into(),
            crate::PUBLIC_SESSION_CHANGED_EVENT.into(),
        );
        let native = accounts.native.clone();
        return tokio::task::spawn_blocking(move || native.set(user_id, token)).await?;
    }
    headers.insert(
        "Set-Cookie".into(),
        guard::session_cookie(&token, context, name),
    );
    Ok(true)
}
