//! Request handlers of the administration API.
//!
//! Every operation returns an [`AdminReply`] carrying an explicit status code
//! and response headers, or an [`AdminError`] with a stable `{code,message}`
//! error shape. Access control is applied per operation in this module.

use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use arachnea_core::application;
use arachnea_core::controler::{
    options::{ApplicationMode, CoreApplicationOptions, ServerNetworkMode},
    ControlerService, RequestControlerContext,
};
use arachnea_core::persistence::{credentials_store::StoredCredentials, TypedEntityStore};

use super::auth::{
    cleared_session_cookie, constant_time_eq, hash_admin_password, session_cookie,
    session_token_from_cookie, verify_admin_password,
};
use super::dto::*;
use super::register_admin_operation;
use super::{
    AdminGroupReload, AdminRuntimeAdapter, AdminServiceGroupConfig, AdminState, AdminStateBuild,
};
use crate::scrapyfy::{
    load_service_catalog_detailed, ScraperAgregator, ScraperQueryCollectionParameter,
    ScraperServiceCatalogEntry, ScraperServiceCredentials, ScraperSourceEnabled,
    ServiceCatalogFailureReason, SourceServiceRecord,
};

/// Delay between the `update-settings` response and the hot application of
/// the new server settings, so the response flows on the listener still alive
/// before it is stopped and re-bound.
const APPLY_SETTINGS_DELAY: Duration = Duration::from_millis(1000);

/// Stable HTTP status error returned by administration operations.
#[derive(Debug)]
pub struct AdminError {
    /// HTTP status code to return.
    status: u16,
    /// Stable machine-readable error code.
    code: &'static str,
    /// Human-readable error message.
    message: String,
}

impl AdminError {
    /// Builds a `400 Bad Request` error.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            code: "bad_request",
            message: message.into(),
        }
    }

    /// Builds a `401 Unauthorized` error.
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: 401,
            code: "unauthorized",
            message: message.into(),
        }
    }

    /// Builds a `403 Forbidden` error.
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: 403,
            code: "forbidden",
            message: message.into(),
        }
    }

    /// Builds a `404 Not Found` error.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: 404,
            code: "not_found",
            message: message.into(),
        }
    }

    /// Builds a `405 Method Not Allowed` error.
    pub fn method_not_allowed(message: impl Into<String>) -> Self {
        Self {
            status: 405,
            code: "method_not_allowed",
            message: message.into(),
        }
    }

    /// Builds a `429 Too Many Requests` error.
    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self {
            status: 429,
            code: "rate_limited",
            message: message.into(),
        }
    }

    /// Returns the HTTP status code of this error.
    pub(crate) fn status(&self) -> u16 {
        self.status
    }

    /// Builds the JSON error body.
    pub(crate) fn value(&self) -> serde_json::Value {
        serde_json::json!({
            "error": {
                "code": self.code,
                "message": self.message,
            }
        })
    }
}

/// One success reply of an administration operation.
pub struct AdminReply {
    /// HTTP status code to return.
    pub(crate) status: u16,
    /// JSON payload to serialize.
    pub(crate) value: serde_json::Value,
    /// Additional response headers.
    pub(crate) headers: HashMap<String, String>,
}

impl AdminReply {
    /// Builds a `200 OK` reply from a serializable payload.
    pub fn ok<T: Serialize>(value: &T) -> Self {
        Self {
            status: 200,
            value: serde_json::to_value(value).expect("admin responses must serialize"),
            headers: HashMap::new(),
        }
    }

    /// Builds a `200 OK` reply with custom response headers.
    pub fn ok_with_headers<T: Serialize>(value: &T, headers: HashMap<String, String>) -> Self {
        Self {
            status: 200,
            value: serde_json::to_value(value).expect("admin responses must serialize"),
            headers,
        }
    }
}

/// Converts an `anyhow` error into a contextualized bad-request error.
fn admin_err(error: anyhow::Error) -> AdminError {
    AdminError::bad_request(format!("{error:#}"))
}

/// Returns whether the caller is allowed to reach an administration operation.
///
/// Desktop clients are always authorized. In server mode, loopback clients are
/// authorized; any other caller must hold a valid session.
pub(crate) async fn require_authorized(
    state: &AdminState,
    context: &RequestControlerContext,
) -> Result<(), AdminError> {
    match state.mode() {
        ApplicationMode::Desktop => Ok(()),
        ApplicationMode::Server => {
            let is_loopback = context
                .remote_addr()
                .map(|addr| addr.ip().is_loopback())
                .unwrap_or(false);
            if is_loopback {
                return Ok(());
            }
            if let Some(token) = session_token_from_cookie(context.get_header("cookie")) {
                if state.sessions().validate(&token) {
                    return Ok(());
                }
            }
            Err(AdminError::unauthorized(
                "Administrator authentication required.",
            ))
        }
    }
}

/// Validates the transport context of a write operation.
///
/// Writes must arrive over `POST` and, when a browser `Origin` or `Referer`
/// header is present, must originate from the same host as the `Host` header.
/// Requests without an origin header (CLI tooling) are accepted.
pub(crate) fn verify_write(
    state: &AdminState,
    context: &RequestControlerContext,
) -> Result<(), AdminError> {
    if state.mode() == ApplicationMode::Desktop {
        return Ok(());
    }
    if let Some(method) = context.method() {
        if !method.eq_ignore_ascii_case("POST") {
            return Err(AdminError::method_not_allowed(
                "Administration writes require the POST method.",
            ));
        }
    }

    let origin = context
        .get_header("origin")
        .or_else(|| context.get_header("referer"));
    if let Some(origin) = origin {
        let host = context.get_header("host").map(str::to_string);
        if !origin_matches_host(origin, host.as_deref()) {
            return Err(AdminError::forbidden(
                "Cross-site administration requests are refused.",
            ));
        }
    }
    Ok(())
}

/// Returns whether the origin host matches the request `Host` header.
///
/// Ports equal to the scheme default (`80`/`443`) are treated as implicit.
fn origin_matches_host(origin: &str, host_header: Option<&str>) -> bool {
    let Some(host) = host_header else {
        return true;
    };
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    let Some(origin_host) = url.host_str() else {
        return false;
    };
    let (host_only, host_port) = split_host(host);
    if !host_only.eq_ignore_ascii_case(origin_host) {
        return false;
    }
    match (host_port, url.port_or_known_default()) {
        (Some(actual), Some(expected)) => actual == expected,
        (None, None) => true,
        (Some(actual), None) => actual == 80 || actual == 443,
        (None, Some(expected)) => expected == 80 || expected == 443,
    }
}

/// Splits a `Host` header value into its host name and optional port.
fn split_host(host: &str) -> (String, Option<u16>) {
    match host.rsplit_once(':') {
        Some((name, port)) => match port.parse::<u16>() {
            Ok(port) => (name.to_string(), Some(port)),
            Err(_) => (host.to_string(), None),
        },
        None => (host.to_string(), None),
    }
}

/// One source resolved against the declared service stores.
struct ResolvedSource {
    /// Service store (group) owning the source.
    service_store_id: String,
    /// Catalog entry of the source.
    entry: ScraperServiceCatalogEntry,
}

/// Resolves a source across the declared service stores.
///
/// When `service_store_id` is given, only that group is searched; otherwise
/// every group is searched in declaration order and an identifier present in
/// several groups is rejected with a bad request asking for qualification.
async fn resolve_source(
    state: &AdminState,
    service_store_id: Option<&str>,
    service_id: &str,
) -> Result<ResolvedSource, AdminError> {
    let mut found: Option<ResolvedSource> = None;
    for group in state.groups() {
        if let Some(requested) = service_store_id {
            if group.service_store_id != requested {
                continue;
            }
        }
        let catalog = load_service_catalog_detailed(&group.manifest_path, &group.service_store_id)
            .map_err(admin_err)?;
        if let Some(entry) = catalog
            .entries
            .into_iter()
            .find(|entry| entry.source.id == service_id)
        {
            if found.is_some() {
                return Err(AdminError::bad_request(format!(
                    "Service identifier {service_id} exists in several service stores; \
                     specify service_store_id."
                )));
            }
            found = Some(ResolvedSource {
                service_store_id: group.service_store_id.clone(),
                entry,
            });
        }
    }
    found.ok_or_else(|| AdminError::not_found(format!("Unknown service {service_id}.")))
}

/// Builds the `status` operation reply.
pub(crate) async fn op_status(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    _input: AdminEmptyRequest,
) -> Result<AdminReply, AdminError> {
    let peer_local = state.mode() == ApplicationMode::Desktop
        || context
            .remote_addr()
            .map(|addr| addr.ip().is_loopback())
            .unwrap_or(false);
    let authenticated = peer_local
        || session_token_from_cookie(context.get_header("cookie"))
            .map(|token| state.sessions().validate(&token))
            .unwrap_or(false);
    let password_configured = state
        .adapter()
        .persisted_settings()
        .map_err(admin_err)?
        .password_hash
        .is_some();

    let response = StatusResponse {
        mode: match state.mode() {
            ApplicationMode::Desktop => "desktop",
            ApplicationMode::Server => "server",
        }
        .to_string(),
        auth_required: state.mode() == ApplicationMode::Server && !peer_local,
        authenticated,
        password_configured,
        capabilities: StatusCapabilities {
            services: true,
            credentials: true,
            settings: state.mode() == ApplicationMode::Server,
            admin_password: true,
            reload: true,
        },
    };
    Ok(AdminReply::ok(&response))
}

/// Builds the `login` operation reply.
pub(crate) async fn op_login(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: LoginRequest,
) -> Result<AdminReply, AdminError> {
    if state.mode() == ApplicationMode::Desktop {
        // No authentication is required from a desktop client.
        return Ok(AdminReply::ok(&LoginResponse {
            authenticated: true,
        }));
    }

    let peer_ip: Option<IpAddr> = context.remote_addr().map(|addr| addr.ip());
    if let Some(ip) = peer_ip {
        if state.login_limiter().is_limited(ip) {
            return Err(AdminError::rate_limited(
                "Too many login attempts. Try again later.",
            ));
        }
    }

    let stored_hash = state
        .adapter()
        .persisted_settings()
        .map_err(admin_err)?
        .password_hash;
    let valid = if let Some(hash) = stored_hash.as_deref() {
        verify_admin_password(&input.password, hash)
    } else if let Some(temp) = state.temp_password() {
        constant_time_eq(&input.password, temp)
    } else {
        false
    };

    if !valid {
        if let Some(ip) = peer_ip {
            state.login_limiter().record_failure(ip);
        }
        return Err(AdminError::unauthorized("Invalid administrator password."));
    }
    if let Some(ip) = peer_ip {
        state.login_limiter().clear(ip);
    }

    let token = state.sessions().create();
    let mut headers = HashMap::new();
    headers.insert("Set-Cookie".to_string(), session_cookie(&token));
    Ok(AdminReply::ok_with_headers(
        &LoginResponse {
            authenticated: true,
        },
        headers,
    ))
}

/// Builds the `logout` operation reply.
pub(crate) async fn op_logout(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    _input: AdminEmptyRequest,
) -> Result<AdminReply, AdminError> {
    if let Some(token) = session_token_from_cookie(context.get_header("cookie")) {
        state.sessions().remove(&token);
    }
    let mut headers = HashMap::new();
    headers.insert("Set-Cookie".to_string(), cleared_session_cookie());
    Ok(AdminReply::ok_with_headers(
        &LoginResponse {
            authenticated: false,
        },
        headers,
    ))
}

/// Selects the description for the requested language.
///
/// Prefers the requested language, then English, then French, then the first
/// available declaration.
fn localized_description(
    descriptions: &HashMap<String, String>,
    requested_lang: Option<&str>,
) -> Option<String> {
    let requested = requested_lang.and_then(|lang| descriptions.get(lang));
    requested
        .or_else(|| descriptions.get("en"))
        .or_else(|| descriptions.get("fr"))
        .or_else(|| descriptions.values().next())
        .cloned()
}

/// Resolves the service logo URL from the template and known parameters.
///
/// Returns `None` when the template contains a placeholder that cannot be
/// resolved from the available parameters.
fn resolve_logo(
    template: Option<&str>,
    parameters: &[ScraperQueryCollectionParameter],
) -> Option<String> {
    let template = template?;
    let mut resolved = template.to_string();
    for parameter in parameters {
        resolved = resolved.replace(&format!("{{{}}}", parameter.name), &parameter.value);
    }
    if resolved.contains('{') {
        None
    } else {
        Some(resolved)
    }
}

/// Masks a stored login, keeping the first character.
fn mask_login(login: &str) -> String {
    let first = login.chars().next().unwrap_or('*');
    format!("{first}***")
}

/// Builds the `services` catalog reply across every declared service store.
pub(crate) async fn op_services(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: ServicesRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;

    let policy = state.activation_policy();
    let adapter = Arc::clone(state.adapter());
    let requested_lang = input.lang.as_deref();
    let mut service_stores = Vec::with_capacity(state.groups().len());

    for group in state.groups() {
        let catalog = load_service_catalog_detailed(&group.manifest_path, &group.service_store_id)
            .map_err(admin_err)?;
        let mut services = Vec::with_capacity(catalog.entries.len());
        for entry in catalog.entries {
            let key = state.source_key(&group.service_store_id, &entry.source.id);
            let override_value = policy.override_for(&key).await.map_err(admin_err)?;
            let has_override = override_value.is_some();
            let enabled = override_value
                .map(|record| record.enabled)
                .unwrap_or(entry.source.default_enabled);
            let stored = adapter
                .stored_credentials(&group.service_store_id, &entry.source.id)
                .ok()
                .flatten();

            services.push(AdminServiceEntry {
                service_store_id: group.service_store_id.clone(),
                id: entry.source.id.clone(),
                title: entry.title.clone(),
                logo: resolve_logo(entry.logo.as_deref(), &entry.source.parameters),
                description: localized_description(&entry.description, requested_lang),
                credentials: entry.credentials.as_ref().map(|declared| CredentialInfo {
                    required: declared.required,
                    signup_url: declared.signup_url.clone(),
                    configured: stored.is_some(),
                    login_masked: stored.as_ref().map(|value| mask_login(&value.login)),
                }),
                default_enabled: entry.source.default_enabled,
                enabled,
                has_override,
                unavailable: false,
            });
        }

        let unavailable = catalog
            .failures
            .into_iter()
            .map(|failure| AdminUnavailableSource {
                id: failure.id,
                path: failure.path.display().to_string(),
                message: failure.message,
            })
            .collect::<Vec<_>>();

        service_stores.push(AdminServiceStore {
            service_store_id: group.service_store_id.clone(),
            services,
            unavailable,
        });
    }

    Ok(AdminReply::ok(&ServicesResponse { service_stores }))
}

/// Builds the `set-service-enabled` reply.
pub(crate) async fn op_set_service_enabled(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: SetServiceEnabledRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    verify_write(&state, &context)?;

    let resolved =
        resolve_source(&state, input.service_store_id.as_deref(), &input.service_id).await?;
    state
        .activation_policy()
        .set_enabled(
            &state.source_key(&resolved.service_store_id, &resolved.entry.source.id),
            input.enabled,
        )
        .await
        .map_err(admin_err)?;

    Ok(AdminReply::ok(&ServiceEnabledResponse {
        service_store_id: resolved.service_store_id,
        service_id: resolved.entry.source.id,
        enabled: Some(input.enabled),
        reload_required: true,
    }))
}

/// Builds the `reset-service-enabled` reply.
pub(crate) async fn op_reset_service_enabled(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: ResetServiceEnabledRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    verify_write(&state, &context)?;

    let resolved =
        resolve_source(&state, input.service_store_id.as_deref(), &input.service_id).await?;
    state
        .activation_policy()
        .clear_enabled(&state.source_key(&resolved.service_store_id, &resolved.entry.source.id))
        .await
        .map_err(admin_err)?;

    Ok(AdminReply::ok(&ServiceEnabledResponse {
        service_store_id: resolved.service_store_id,
        service_id: resolved.entry.source.id,
        enabled: Some(resolved.entry.source.default_enabled),
        reload_required: true,
    }))
}

/// Builds the `credentials` operation reply.
pub(crate) async fn op_credentials(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: CredentialsRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;

    let adapter = Arc::clone(state.adapter());
    let mut credentials = BTreeMap::new();
    let mut key_counts: HashMap<String, usize> = HashMap::new();
    let mut pairs: Vec<(
        String,
        String,
        Option<ScraperServiceCredentials>,
        Option<StoredCredentials>,
    )> = Vec::new();

    for group in state.groups() {
        if let Some(requested_store) = input.service_store_id.as_deref() {
            if group.service_store_id != requested_store {
                continue;
            }
        }
        let catalog = load_service_catalog_detailed(&group.manifest_path, &group.service_store_id)
            .map_err(admin_err)?;
        for entry in catalog.entries {
            if let Some(requested) = input.service_id.as_deref() {
                if entry.source.id != requested {
                    continue;
                }
            }
            let stored = adapter
                .stored_credentials(&group.service_store_id, &entry.source.id)
                .ok()
                .flatten();
            key_counts
                .entry(entry.source.id.clone())
                .and_modify(|count| *count += 1)
                .or_insert(1);
            pairs.push((
                group.service_store_id.clone(),
                entry.source.id,
                entry.credentials,
                stored,
            ));
        }
    }

    if let Some(requested) = input.service_id.as_deref() {
        if pairs.is_empty() {
            return Err(AdminError::not_found(format!(
                "Unknown service {requested}."
            )));
        }
    }

    for (store, id, declared, stored) in pairs {
        let key = if key_counts.get(&id).copied().unwrap_or(1) > 1 {
            format!("{store}/{id}")
        } else {
            id.clone()
        };
        credentials.insert(
            key,
            CredentialInfo {
                required: declared.as_ref().map(|d| d.required).unwrap_or(false),
                signup_url: declared.and_then(|d| d.signup_url),
                configured: stored.is_some(),
                login_masked: stored.map(|value| mask_login(&value.login)),
            },
        );
    }

    Ok(AdminReply::ok(&CredentialsResponse { credentials }))
}

/// Builds the `set-credentials` reply.
pub(crate) async fn op_set_credentials(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: SetCredentialsRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    verify_write(&state, &context)?;

    let resolved =
        resolve_source(&state, input.service_store_id.as_deref(), &input.service_id).await?;
    if input.login.trim().is_empty() || input.password.trim().is_empty() {
        return Err(AdminError::bad_request(
            "Service login and password must not be empty.",
        ));
    }

    state
        .adapter()
        .set_stored_credentials(
            &resolved.service_store_id,
            &resolved.entry.source.id,
            StoredCredentials {
                login: input.login,
                password: input.password,
            },
        )
        .map_err(admin_err)?;

    Ok(AdminReply::ok(&ServiceEnabledResponse {
        service_store_id: resolved.service_store_id,
        service_id: resolved.entry.source.id,
        enabled: None,
        reload_required: false,
    }))
}

/// Builds the `clear-credentials` reply.
pub(crate) async fn op_clear_credentials(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: ClearCredentialsRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    verify_write(&state, &context)?;

    let resolved =
        resolve_source(&state, input.service_store_id.as_deref(), &input.service_id).await?;
    state
        .adapter()
        .clear_stored_credentials(&resolved.service_store_id, &resolved.entry.source.id)
        .map_err(admin_err)?;

    Ok(AdminReply::ok(&ServiceEnabledResponse {
        service_store_id: resolved.service_store_id,
        service_id: resolved.entry.source.id,
        enabled: None,
        reload_required: false,
    }))
}

/// Builds the `settings` reply.
pub(crate) async fn op_settings(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    _input: AdminEmptyRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;

    let effective = state.settings();
    let password_configured = state
        .adapter()
        .persisted_settings()
        .map_err(admin_err)?
        .password_hash
        .is_some();
    Ok(AdminReply::ok(&SettingsResponse {
        server_port: effective
            .server_port
            .expect("runtime administration settings must have an effective server port"),
        server_port_source: effective.server_port_source,
        network_mode: effective.network_mode.to_string(),
        network_mode_source: effective.network_mode_source,
        entrypoint_root: effective.entrypoint_root.clone(),
        entrypoint_root_source: effective.entrypoint_root_source,
        password_configured,
        public_http_warning: effective.network_mode == ServerNetworkMode::Public,
    }))
}

/// Builds the `update-settings` reply.
pub(crate) async fn op_update_settings(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: UpdateSettingsRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    verify_write(&state, &context)?;

    let effective_mode = state.mode();
    let mut persisted = state.adapter().persisted_settings().map_err(admin_err)?;

    if let Some(port) = input.server_port {
        if port == 0 {
            return Err(AdminError::bad_request("Server port cannot be zero."));
        }
        persisted.server_port = Some(port);
    }
    if let Some(root) = input.entrypoint_root {
        let root = root.trim().to_string();
        persisted.entrypoint_root = if root.is_empty() { None } else { Some(root) };
    }
    if let Some(network) = input.network_mode {
        let network = network.trim().to_lowercase();
        if !matches!(network.as_str(), "local" | "private" | "public") {
            return Err(AdminError::bad_request(format!(
                "Invalid network mode {network}; expected local, private or public."
            )));
        }
        if network == "local" && effective_mode == ApplicationMode::Server {
            let loopback = context
                .remote_addr()
                .map(|addr| addr.ip().is_loopback())
                .unwrap_or(false);
            if !loopback || !application::is_running_elevated() {
                return Err(AdminError::forbidden(
                    "Network mode local requires a loopback client and elevated privileges.",
                ));
            }
        }
        persisted.network_mode = network.parse().expect("validated network mode must parse");
    }

    state
        .adapter()
        .save_persisted_settings(&persisted)
        .map_err(admin_err)?;

    // Hot application path: in server mode with a REST handle, the port,
    // network mode and entrypoint root are applied to the running server
    // without restarting the process. The response is sent first; the
    // application runs APPLY_SETTINGS_DELAY later so the response flows on
    // the listener still alive. The definitive outcome is observable via the
    // `settings` operation and the application log.
    if effective_mode == ApplicationMode::Server {
        if let Some(target) = state.adapter().target_server_settings() {
            // Only advertise an URL change when the port or the entrypoint
            // root will actually move, so the UI never redirects to a URL that
            // stays pinned by a command-line override.
            let current = state.settings();
            let target_port = target
                .server_port
                .expect("REST server target settings must have an effective server port");
            let port_changes = target_port
                != current
                    .server_port
                    .expect("runtime administration settings must have an effective server port");
            let current_root = current
                .entrypoint_root
                .as_deref()
                .filter(|root| !root.is_empty());
            let root_changes = target.entrypoint_root.as_deref() != current_root;
            let admin_url = if port_changes || root_changes {
                hot_apply_admin_url(&context, target.entrypoint_root.as_deref(), target_port)
            } else {
                None
            };
            let adapter = Arc::clone(state.adapter());
            let state_for_apply = Arc::clone(&state);
            std::thread::spawn(move || {
                std::thread::sleep(APPLY_SETTINGS_DELAY);
                let report = adapter.apply_server_settings();
                state_for_apply.update_effective_server_settings(
                    report.server_port,
                    report
                        .network_mode
                        .parse()
                        .expect("REST server report must contain a valid network mode"),
                    report.entrypoint_root.clone(),
                );
                match &report.apply_error {
                    Some(error) => tracing::error!(
                        error = %error,
                        "hot application of server settings failed"
                    ),
                    None => tracing::info!(
                        port = report.server_port,
                        network = %report.network_mode,
                        root = ?report.entrypoint_root,
                        "server settings applied to the running server"
                    ),
                }
            });

            return Ok(AdminReply::ok(&UpdateSettingsResponse {
                restart_required: false,
                applied: true,
                apply_error: None,
                admin_url,
            }));
        }
    }

    Ok(AdminReply::ok(&UpdateSettingsResponse {
        restart_required: true,
        applied: false,
        apply_error: None,
        admin_url: None,
    }))
}

/// Builds the target administration URL of a hot settings application.
///
/// The host name comes from the request `Host` header so the URL keeps the
/// origin the administrator is talking to; the target port replaces the port
/// of the header, and the target root prefixes the admin path. Returns `None`
/// when the header is missing.
fn hot_apply_admin_url(
    context: &RequestControlerContext,
    entrypoint_root: Option<&str>,
    target_port: u16,
) -> Option<String> {
    let host = context.header("host")?;
    let hostname = match host.rsplit_once(':') {
        // Host header forms: `hostname:port`, `[ipv6]:port` or bare `ipv6`.
        Some((lhs, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => lhs,
        _ => host,
    };
    let root = entrypoint_root.unwrap_or("").trim_matches('/');
    if root.is_empty() {
        Some(format!("http://{hostname}:{target_port}/admin/"))
    } else {
        Some(format!("http://{hostname}:{target_port}/{root}/admin/"))
    }
}

/// Builds the `set-admin-password` reply.
pub(crate) async fn op_set_admin_password(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: SetAdminPasswordRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    verify_write(&state, &context)?;

    let mut persisted = state.adapter().persisted_settings().map_err(admin_err)?;
    if let Some(existing) = persisted.password_hash.as_deref() {
        let current = input.current_password.as_deref().unwrap_or("");
        if !verify_admin_password(current, existing) {
            return Err(AdminError::unauthorized("Current password is incorrect."));
        }
    }
    if input.new_password.chars().count() < 8 {
        return Err(AdminError::bad_request(
            "New administrator password must be at least 8 characters.",
        ));
    }

    persisted.password_hash = Some(hash_admin_password(&input.new_password).map_err(admin_err)?);
    state
        .adapter()
        .save_persisted_settings(&persisted)
        .map_err(admin_err)?;

    Ok(AdminReply::ok(&AdminEmptyRequest {}))
}

/// Builds the generic validation reload of one service store.
///
/// Groups without an application-specific runtime are validated by rebuilding
/// their catalog and activation view; no live runtime is replaced.
async fn generic_group_reload(
    state: &AdminState,
    group: &AdminServiceGroupConfig,
) -> AdminGroupReload {
    let mut report = AdminGroupReload {
        applied: false,
        ..Default::default()
    };
    let catalog = match load_service_catalog_detailed(&group.manifest_path, &group.service_store_id)
    {
        Ok(catalog) => catalog,
        Err(error) => {
            report.build_error = Some(format!("{error:#}"));
            return report;
        }
    };

    let policy = state.activation_policy();
    let mut defaults = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for entry in &catalog.entries {
        defaults.push(entry.source.clone());
        if let Err(error) = policy.is_enabled(&entry.source).await {
            failures.push(format!(
                "{}: failed to read activation state: {error:#}",
                entry.source.path.display()
            ));
        }
    }
    if let Err(error) = policy.register_defaults(&defaults).await {
        failures.push(format!(
            "Failed to synchronize service activation defaults: {error:#}"
        ));
    }
    for failure in &catalog.failures {
        if matches!(
            failure.reason,
            ServiceCatalogFailureReason::Invalid | ServiceCatalogFailureReason::Duplicate
        ) {
            failures.push(format!("{}: {}", failure.path.display(), failure.message));
        }
    }
    if !failures.is_empty() {
        report.build_error = Some(failures.join("; "));
    }
    report.applied = report.build_error.is_none();
    report
}

/// Builds the `reload` reply.
///
/// Every declared group is reloaded: groups with an application-specific
/// runtime go through the adapter hook, the others are validated generically.
/// A failing group keeps its runtime untouched and is reported with its
/// identifier.
pub(crate) async fn op_reload(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    _input: AdminEmptyRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    verify_write(&state, &context)?;

    let adapter = Arc::clone(state.adapter());
    let mut reloads: Vec<(String, AdminGroupReload)> = Vec::with_capacity(state.groups().len());
    for group in state.groups() {
        let reload = match adapter.rebuild_group(&group.service_store_id).await {
            Ok(Some(reload)) => reload,
            Ok(None) => generic_group_reload(&state, group).await,
            Err(error) => AdminGroupReload {
                applied: false,
                build_error: Some(format!("{error:#}")),
                ..Default::default()
            },
        };
        reloads.push((group.service_store_id.clone(), reload));
    }

    // Legacy flat view: report the first declared group so single-group
    // clients keep their contract.
    let first = reloads.into_iter().next();
    let response = ReloadResponse {
        applied: first
            .as_ref()
            .map(|(_, reload)| reload.applied)
            .unwrap_or(false),
        build_error: first.and_then(|(_, reload)| reload.build_error),
    };

    Ok(AdminReply::ok(&response))
}

/// Registers every administration operation against the controller.
///
/// # Arguments
/// * `settings` - Effective runtime settings of the running server.
/// * `scraper_agregator` - Aggregator supplying the administrable service groups.
/// * `source_enabled` - Typed store holding source activation overrides.
/// * `adapter` - Application adapter for settings, credentials, and reloads.
/// * `controler` - Controller receiving the `admin/*` routes.
///
/// # Errors
///
/// Returns an error when the administration state cannot be built.
pub fn register_admin_service(
    settings: CoreApplicationOptions,
    scraper_agregator: &ScraperAgregator,
    source_enabled: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
    adapter: Arc<dyn AdminRuntimeAdapter>,
    controler: &mut dyn ControlerService,
) -> Result<AdminStateBuild> {
    let groups = scraper_agregator
        .query_services()
        .map(|service| AdminServiceGroupConfig {
            service_store_id: service.name.clone(),
            manifest_path: service.json_path.clone(),
        })
        .collect();
    let state = AdminState::build(settings, groups, source_enabled, adapter)?;

    register_admin_operation(controler, &state.state, "status", op_status);
    register_admin_operation(controler, &state.state, "login", op_login);
    register_admin_operation(controler, &state.state, "logout", op_logout);
    register_admin_operation(controler, &state.state, "services", op_services);
    register_admin_operation(
        controler,
        &state.state,
        "set-service-enabled",
        op_set_service_enabled,
    );
    register_admin_operation(
        controler,
        &state.state,
        "reset-service-enabled",
        op_reset_service_enabled,
    );
    register_admin_operation(controler, &state.state, "credentials", op_credentials);
    register_admin_operation(
        controler,
        &state.state,
        "set-credentials",
        op_set_credentials,
    );
    register_admin_operation(
        controler,
        &state.state,
        "clear-credentials",
        op_clear_credentials,
    );
    register_admin_operation(controler, &state.state, "settings", op_settings);
    register_admin_operation(
        controler,
        &state.state,
        "update-settings",
        op_update_settings,
    );
    register_admin_operation(
        controler,
        &state.state,
        "set-admin-password",
        op_set_admin_password,
    );
    register_admin_operation(controler, &state.state, "reload", op_reload);

    if let Some(password) = &state.temporary_password {
        println!("Temporary administrator password for remote administration: {password}");
    }
    
    Ok(state)
}
