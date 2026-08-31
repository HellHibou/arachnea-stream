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

use arachnea_core::application;
use arachnea_core::controler::RequestControlerContext;
use arachnea_core::persistence::credentials_store::StoredCredentials;
use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{
    load_service_catalog_detailed, ScraperQueryCollectionParameter, ScraperServiceCatalogEntry,
};

use super::auth::{
    cleared_session_cookie, constant_time_eq, session_cookie, session_token_from_cookie,
    verify_admin_password,
};
use super::dto::*;
use super::{AdminMode, AdminState};

/// Stable HTTP status error returned by administration operations.
#[derive(Debug)]
pub(crate) struct AdminError {
    /// HTTP status code to return.
    status: u16,
    /// Stable machine-readable error code.
    code: &'static str,
    /// Human-readable error message.
    message: String,
}

impl AdminError {
    /// Builds a `400 Bad Request` error.
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            code: "bad_request",
            message: message.into(),
        }
    }

    /// Builds a `401 Unauthorized` error.
    pub(crate) fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: 401,
            code: "unauthorized",
            message: message.into(),
        }
    }

    /// Builds a `403 Forbidden` error.
    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: 403,
            code: "forbidden",
            message: message.into(),
        }
    }

    /// Builds a `404 Not Found` error.
    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: 404,
            code: "not_found",
            message: message.into(),
        }
    }

    /// Builds a `405 Method Not Allowed` error.
    pub(crate) fn method_not_allowed(message: impl Into<String>) -> Self {
        Self {
            status: 405,
            code: "method_not_allowed",
            message: message.into(),
        }
    }

    /// Builds a `429 Too Many Requests` error.
    pub(crate) fn rate_limited(message: impl Into<String>) -> Self {
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
pub(crate) struct AdminReply {
    /// HTTP status code to return.
    pub(crate) status: u16,
    /// JSON payload to serialize.
    pub(crate) value: serde_json::Value,
    /// Additional response headers.
    pub(crate) headers: HashMap<String, String>,
}

impl AdminReply {
    /// Builds a `200 OK` reply from a serializable payload.
    pub(crate) fn ok<T: Serialize>(value: &T) -> Self {
        Self {
            status: 200,
            value: serde_json::to_value(value).expect("admin responses must serialize"),
            headers: HashMap::new(),
        }
    }

    /// Builds a `200 OK` reply with custom response headers.
    pub(crate) fn ok_with_headers<T: Serialize>(
        value: &T,
        headers: HashMap<String, String>,
    ) -> Self {
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
        AdminMode::Desktop => Ok(()),
        AdminMode::Server => {
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
    if state.mode() == AdminMode::Desktop {
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

/// Builds the `status` operation reply.
pub(crate) async fn op_status(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    _input: AdminEmptyRequest,
) -> Result<AdminReply, AdminError> {
    let peer_local = state.mode() == AdminMode::Desktop
        || context
            .remote_addr()
            .map(|addr| addr.ip().is_loopback())
            .unwrap_or(false);
    let authenticated = peer_local
        || session_token_from_cookie(context.get_header("cookie"))
            .map(|token| state.sessions().validate(&token))
            .unwrap_or(false);

    let response = StatusResponse {
        mode: match state.mode() {
            AdminMode::Desktop => "desktop",
            AdminMode::Server => "server",
        }
        .to_string(),
        auth_required: state.mode() == AdminMode::Server && !peer_local,
        authenticated,
        password_configured: state.configuration().password_hash.is_some(),
        capabilities: StatusCapabilities {
            services: true,
            credentials: true,
            settings: state.mode() == AdminMode::Server,
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
    if state.mode() == AdminMode::Desktop {
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

    let config = state.configuration();
    let valid = if let Some(hash) = config.password_hash.as_deref() {
        verify_admin_password(&input.password, hash)
    } else if let Some(temp) = &state.temp_password {
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

/// Builds the `services` catalog reply.
pub(crate) async fn op_services(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: ServicesRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;

    let catalog = load_service_catalog_detailed(
        &crate::stream_scraper::DEFAULT_SERVICES_CONFIG_PATH,
    )
    .map_err(admin_err)?;

    let policy = state.activation_policy();
    let requested_lang = input.lang.as_deref();
    let mut services = Vec::with_capacity(catalog.entries.len());
    for entry in catalog.entries {
        let id = entry.source.id.clone();
        let override_value = policy.override_for(&id).await.map_err(admin_err)?;
        let has_override = override_value.is_some();
        let enabled = override_value
            .map(|record| record.enabled)
            .unwrap_or(entry.source.default_enabled);

        services.push(AdminServiceEntry {
            id,
            title: entry.title.clone(),
            logo: resolve_logo(entry.logo.as_deref(), &entry.source.parameters),
            description: localized_description(&entry.description, requested_lang),
            credentials: build_credential_info(&entry, state.credentials_store().as_ref()),
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
        .collect();

    Ok(AdminReply::ok(&ServicesResponse {
        services,
        unavailable,
    }))
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

/// Builds the credential state for one catalog entry.
fn build_credential_info(
    entry: &ScraperServiceCatalogEntry,
    store: &dyn CredentialsStore,
) -> Option<CredentialInfo> {
    let declared = entry.credentials.as_ref()?;
    let stored = store.get_credentials(&entry.source.id).ok().flatten();
    Some(CredentialInfo {
        required: declared.required,
        signup_url: declared.signup_url.clone(),
        configured: stored.is_some(),
        login_masked: stored.map(|credentials| mask_login(&credentials.login)),
    })
}

/// Masks a stored login, keeping the first character.
fn mask_login(login: &str) -> String {
    let first = login.chars().next().unwrap_or('*');
    format!("{first}***")
}

/// Builds the `set-service-enabled` reply.
pub(crate) async fn op_set_service_enabled(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: SetServiceEnabledRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    verify_write(&state, &context)?;

    if !service_exists(&input.service_id).await.map_err(admin_err)? {
        return Err(AdminError::not_found(format!(
            "Unknown service {}.",
            input.service_id
        )));
    }
    state
        .activation_policy()
        .set_enabled(&input.service_id, input.enabled)
        .await
        .map_err(admin_err)?;

    Ok(AdminReply::ok(&ServiceEnabledResponse {
        service_id: input.service_id,
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

    let default_enabled = match service_default(&input.service_id).await.map_err(admin_err)? {
        Some(default_enabled) => default_enabled,
        None => {
            return Err(AdminError::not_found(format!(
                "Unknown service {}.",
                input.service_id
            )))
        }
    };
    state
        .activation_policy()
        .clear_enabled(&input.service_id)
        .await
        .map_err(admin_err)?;

    Ok(AdminReply::ok(&ServiceEnabledResponse {
        service_id: input.service_id,
        enabled: Some(default_enabled),
        reload_required: true,
    }))
}

/// Returns whether an identifier belongs to the declared catalog.
async fn service_exists(service_id: &str) -> Result<bool> {
    let catalog = load_service_catalog_detailed(
        &crate::stream_scraper::DEFAULT_SERVICES_CONFIG_PATH,
    )?;
    Ok(catalog
        .entries
        .iter()
        .any(|entry| entry.source.id == service_id))
}

/// Returns the manifest default activation of one service.
async fn service_default(service_id: &str) -> Result<Option<bool>> {
    let catalog = load_service_catalog_detailed(
        &crate::stream_scraper::DEFAULT_SERVICES_CONFIG_PATH,
    )?;
    Ok(catalog
        .entries
        .iter()
        .find(|entry| entry.source.id == service_id)
        .map(|entry| entry.source.default_enabled))
}

/// Builds the `credentials` operation reply.
pub(crate) async fn op_credentials(
    state: Arc<AdminState>,
    _context: RequestControlerContext,
    input: CredentialsRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &_context).await?;

    let catalog = load_service_catalog_detailed(
        &crate::stream_scraper::DEFAULT_SERVICES_CONFIG_PATH,
    )
    .map_err(admin_err)?;
    if let Some(ref requested) = input.service_id {
        if !catalog
            .entries
            .iter()
            .any(|entry| &entry.source.id == requested)
        {
            return Err(AdminError::not_found(format!(
                "Unknown service {}.",
                requested
            )));
        }
    }

    let store = state.credentials_store();
    let mut credentials = BTreeMap::new();
    for entry in catalog.entries {
        if let Some(ref requested) = input.service_id {
            if &entry.source.id != requested {
                continue;
            }
        }
        let stored = store.get_credentials(&entry.source.id).ok().flatten();
        credentials.insert(
            entry.source.id.clone(),
            CredentialInfo {
                required: entry
                    .credentials
                    .as_ref()
                    .map(|declared| declared.required)
                    .unwrap_or(false),
                signup_url: entry
                    .credentials
                    .as_ref()
                    .and_then(|declared| declared.signup_url.clone()),
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

    if !service_exists(&input.service_id).await.map_err(admin_err)? {
        return Err(AdminError::not_found(format!(
            "Unknown service {}.",
            input.service_id
        )));
    }
    if input.login.trim().is_empty() || input.password.trim().is_empty() {
        return Err(AdminError::bad_request(
            "Service login and password must not be empty.",
        ));
    }

    state
        .credentials_store()
        .set_credentials(
            &input.service_id,
            StoredCredentials {
                login: input.login,
                password: input.password,
            },
        )
        .map_err(admin_err)?;

    Ok(AdminReply::ok(&ServiceEnabledResponse {
        service_id: input.service_id,
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

    if !service_exists(&input.service_id).await.map_err(admin_err)? {
        return Err(AdminError::not_found(format!(
            "Unknown service {}.",
            input.service_id
        )));
    }
    state
        .credentials_store()
        .clear_credentials(&input.service_id)
        .map_err(admin_err)?;

    Ok(AdminReply::ok(&ServiceEnabledResponse {
        service_id: input.service_id,
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
    Ok(AdminReply::ok(&SettingsResponse {
        server_port: effective.server_port,
        server_port_source: effective.server_port_source,
        network_mode: effective.network_mode.clone(),
        network_mode_source: effective.network_mode_source,
        entrypoint_root: effective.entrypoint_root.clone(),
        entrypoint_root_source: effective.entrypoint_root_source,
        password_configured: state.configuration().password_hash.is_some(),
        public_http_warning: effective.network_mode == "public",
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
    let mut config = state.configuration();

    if let Some(port) = input.server_port {
        if port == 0 {
            return Err(AdminError::bad_request("Server port cannot be zero."));
        }
        config.server_port = Some(port);
    }
    if let Some(root) = input.entrypoint_root {
        let root = root.trim().to_string();
        config.entrypoint_root = if root.is_empty() { None } else { Some(root) };
    }
    if let Some(network) = input.network_mode {
        let network = network.trim().to_lowercase();
        if !matches!(network.as_str(), "local" | "private" | "public") {
            return Err(AdminError::bad_request(format!(
                "Invalid network mode {network}; expected local, private or public."
            )));
        }
        if network == "local" && effective_mode == AdminMode::Server {
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
        config.network_mode = Some(network);
    }

    config.save().map_err(admin_err)?;
    state.set_configuration(config);

    Ok(AdminReply::ok(&UpdateSettingsResponse {
        restart_required: true,
    }))
}

/// Builds the `set-admin-password` reply.
pub(crate) async fn op_set_admin_password(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    input: SetAdminPasswordRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    verify_write(&state, &context)?;

    let mut config = state.configuration();
    if let Some(existing) = config.password_hash.as_deref() {
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

    let hash = super::auth::hash_admin_password(&input.new_password)
        .map_err(admin_err)?;
    config.password_hash = Some(hash);
    config.save().map_err(admin_err)?;
    state.set_configuration(config);

    Ok(AdminReply::ok(&AdminEmptyRequest {}))
}

/// Builds the `reload` reply.
pub(crate) async fn op_reload(
    state: Arc<AdminState>,
    context: RequestControlerContext,
    _input: AdminEmptyRequest,
) -> Result<AdminReply, AdminError> {
    require_authorized(&state, &context).await?;
    verify_write(&state, &context)?;

    let report = state
        .reloadable()
        .reload()
        .await
        .map_err(admin_err)?;

    Ok(AdminReply::ok(&ReloadResponse {
        applied: report.applied,
        loaded: report.loaded,
        disabled: report.disabled,
        ignored: report
            .ignored
            .into_iter()
            .map(|source| AdminReloadSkippedSource {
                id: source.id,
                path: source.path,
            })
            .collect(),
        errors: report
            .errors
            .into_iter()
            .map(|error| AdminReloadSourceError {
                id: error.id,
                path: error.path,
                message: error.message,
            })
            .collect(),
        build_error: report.build_error,
    }))
}