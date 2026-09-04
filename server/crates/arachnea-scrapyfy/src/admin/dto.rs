//! Request and response DTOs of the administration API.
//!
//! Sources are addressed by the composite pair
//! `(service_store_id, service_id)`; write operations accept an optional
//! `service_store_id` and resolve a bare identifier across the declared
//! service stores for backward compatibility with single-group clients.

use serde::{Deserialize, Serialize};

use super::reload::ReloadGroupReport;

use arachnea_core::controler::options::SettingSource;

/// Credential metadata exposed to administrators (never the secret itself).
#[derive(Clone, Debug, Serialize)]
pub struct CredentialInfo {
    /// Whether the service declares credentials as required.
    pub required: bool,
    /// Account creation URL advertised by the service, when declared.
    pub signup_url: Option<String>,
    /// Whether credentials are stored for the service.
    pub configured: bool,
    /// Masked stored login, when credentials exist.
    pub login_masked: Option<String>,
}

/// One service entry of the administration catalog.
#[derive(Clone, Debug, Serialize)]
pub struct AdminServiceEntry {
    /// Service store (group) identifier owning the service.
    pub service_store_id: String,
    /// Stable YAML identifier of the service.
    pub id: String,
    /// Human-readable title, when declared.
    pub title: Option<String>,
    /// Resolved logo URL, `None` when the template has unresolved placeholders.
    pub logo: Option<String>,
    /// Description in the requested language, when available.
    pub description: Option<String>,
    /// Default activation value declared by the manifest.
    pub default_enabled: bool,
    /// Effective activation state (override applied when present).
    pub enabled: bool,
    /// Whether the activation state comes from a persistent override.
    pub has_override: bool,
    /// Credential requirements advertised by the service.
    pub credentials: Option<CredentialInfo>,
    /// True when the YAML file is currently missing or invalid.
    pub unavailable: bool,
}

/// One source that failed to load, with its error context.
#[derive(Clone, Debug, Serialize)]
pub struct AdminUnavailableSource {
    /// Parsed identifier, when available.
    pub id: Option<String>,
    /// YAML file path involved.
    pub path: String,
    /// Human-readable error message.
    pub message: String,
}

/// One service store (group) section of the administration catalog.
#[derive(Debug, Serialize)]
pub struct AdminServiceStore {
    /// Service store (group) identifier.
    pub service_store_id: String,
    /// Declared services of this store, including disabled ones.
    pub services: Vec<AdminServiceEntry>,
    /// Sources of this store that could not be loaded, with their error context.
    pub unavailable: Vec<AdminUnavailableSource>,
}

/// Response of the `services` catalog operation.
#[derive(Debug, Serialize)]
pub struct ServicesResponse {
    /// Service store sections, in the declared group order.
    pub service_stores: Vec<AdminServiceStore>,
}
/// Response of the `status` operation.
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    /// Backend mode: `desktop` or `server`.
    pub mode: String,
    /// Whether the calling client must authenticate.
    pub auth_required: bool,
    /// Whether the calling client currently passes the access control.
    pub authenticated: bool,
    /// Whether a permanent administrator password hash is configured.
    pub password_configured: bool,
    /// Features enabled in this build.
    pub capabilities: StatusCapabilities,
}

/// Capability flags reported by the `status` operation.
#[derive(Debug, Serialize)]
pub struct StatusCapabilities {
    /// Service catalog and activation management.
    pub services: bool,
    /// Service credentials management.
    pub credentials: bool,
    /// Server settings management.
    pub settings: bool,
    /// Permanent administrator password management.
    pub admin_password: bool,
    /// Server configuration reload.
    pub reload: bool,
}

/// Response of the `settings` operation.
#[derive(Debug, Serialize)]
pub struct SettingsResponse {
    /// Effective REST server port.
    pub server_port: u16,
    /// Where the effective port comes from.
    pub server_port_source: SettingSource,
    /// Effective network mode: `local`, `private` or `public`.
    pub network_mode: String,
    /// Where the effective network mode comes from.
    pub network_mode_source: SettingSource,
    /// Effective public root path, when set.
    pub entrypoint_root: Option<String>,
    /// Where the effective root comes from.
    pub entrypoint_root_source: SettingSource,
    /// Whether a permanent administrator password hash is configured.
    pub password_configured: bool,
    /// Whether the network mode is public over plain HTTP.
    pub public_http_warning: bool,
}

/// Request body of `update-settings`.
///
/// Absent fields keep their current value. An empty `entrypoint_root` clears
/// the persisted root.
#[derive(Debug, Default, Deserialize)]
pub struct UpdateSettingsRequest {
    /// New REST server port.
    #[serde(default, alias = "serverPort")]
    pub server_port: Option<u16>,
    /// New public root path; an empty string clears it.
    #[serde(default, alias = "entrypointRoot")]
    pub entrypoint_root: Option<String>,
    /// New network mode: `local`, `private` or `public`.
    #[serde(default, alias = "networkMode")]
    pub network_mode: Option<String>,
}

/// Response of `update-settings`.
#[derive(Debug, Serialize)]
pub struct UpdateSettingsResponse {
    /// Whether a process restart is still required to apply the changes.
    ///
    /// Kept for backward compatibility: it stays `true` only when hot
    /// application is not available (desktop mode or missing REST handle).
    pub restart_required: bool,
    /// Whether hot application of the settings has been scheduled on the
    /// running server (the definitive outcome is observable via `settings`).
    pub applied: bool,
    /// Scheduling failure context, when hot application could not be planned.
    pub apply_error: Option<String>,
    /// Target administration URL when the port or the entrypoint root change,
    /// so the UI can redirect once the new listener is up.
    pub admin_url: Option<String>,
}

/// Request body of `set-admin-password`.
#[derive(Debug, Deserialize)]
pub struct SetAdminPasswordRequest {
    /// Current password, required when a password is already configured.
    #[serde(default, alias = "currentPassword")]
    pub current_password: Option<String>,
    /// New permanent password.
    #[serde(alias = "newPassword")]
    pub new_password: String,
}

/// Request body of `set-service-enabled`.
#[derive(Debug, Deserialize)]
pub struct SetServiceEnabledRequest {
    /// Service store (group) identifier; when absent, the service identifier
    /// is resolved across the declared service stores.
    #[serde(default, alias = "serviceStoreId")]
    pub service_store_id: Option<String>,
    /// Stable service identifier.
    #[serde(alias = "serviceId")]
    pub service_id: String,
    /// Desired activation state.
    pub enabled: bool,
}

/// Request body of `reset-service-enabled`.
#[derive(Debug, Deserialize)]
pub struct ResetServiceEnabledRequest {
    /// Service store (group) identifier; optional like
    /// [`SetServiceEnabledRequest::service_store_id`].
    #[serde(default, alias = "serviceStoreId")]
    pub service_store_id: Option<String>,
    /// Stable service identifier.
    #[serde(alias = "serviceId")]
    pub service_id: String,
}

/// Response of activation operations.
#[derive(Debug, Serialize)]
pub struct ServiceEnabledResponse {
    /// Service store (group) identifier owning the service.
    pub service_store_id: String,
    /// Stable service identifier.
    pub service_id: String,
    /// Effective activation state after the operation, when known.
    pub enabled: Option<bool>,
    /// Whether a reload is required to apply the change to the live scraper.
    pub reload_required: bool,
}

/// Request body of `set-credentials`.
#[derive(Debug, Deserialize)]
pub struct SetCredentialsRequest {
    /// Service store (group) identifier; optional like
    /// [`SetServiceEnabledRequest::service_store_id`].
    #[serde(default, alias = "serviceStoreId")]
    pub service_store_id: Option<String>,
    /// Stable service identifier.
    #[serde(alias = "serviceId")]
    pub service_id: String,
    /// Service login.
    pub login: String,
    /// Service password.
    pub password: String,
}

/// Request body of `clear-credentials`.
#[derive(Debug, Deserialize)]
pub struct ClearCredentialsRequest {
    /// Service store (group) identifier; optional like
    /// [`SetServiceEnabledRequest::service_store_id`].
    #[serde(default, alias = "serviceStoreId")]
    pub service_store_id: Option<String>,
    /// Stable service identifier.
    #[serde(alias = "serviceId")]
    pub service_id: String,
}

/// Request body of the `credentials` operation.
#[derive(Debug, Default, Deserialize)]
pub struct CredentialsRequest {
    /// Optional service store (group) identifier restricting the query.
    #[serde(default, alias = "serviceStoreId")]
    pub service_store_id: Option<String>,
    /// Optional service identifier; when absent, every service is returned.
    #[serde(default, alias = "serviceId")]
    pub service_id: Option<String>,
}

/// Response of the `credentials` operation.
#[derive(Debug, Serialize)]
pub struct CredentialsResponse {
    /// Credential state per service identifier.
    pub credentials: std::collections::BTreeMap<String, CredentialInfo>,
}

/// Request body of `login`.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    /// Administrator password.
    pub password: String,
}

/// Response of `login`.
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    /// Whether a session was created (always `true` on success).
    pub authenticated: bool,
}

/// Response of `reload`.
#[derive(Debug, Serialize)]
pub struct ReloadResponse {
    /// Whether the primary group replaced its runtime instance.
    pub applied: bool,
    /// Build failure message when the replacement could not be constructed.
    pub build_error: Option<String>,
    /// Result of every declared administrable service group, in display order.
    pub groups: Vec<ReloadGroupReport>,
}

/// Request body of the `services` operation.
#[derive(Debug, Default, Deserialize)]
pub struct ServicesRequest {
    /// Requested language code for localized descriptions.
    #[serde(default)]
    pub lang: Option<String>,
}

/// Empty body used by operations that take no specific input.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct AdminEmptyRequest {}
