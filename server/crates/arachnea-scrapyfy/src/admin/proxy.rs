//! Application-independent administration contracts for proxy access.
use arachnea_core::controler::options::SettingSource;
use serde::{Deserialize, Serialize};

#[cfg(feature = "arachnea-proxy")]
mod service;
#[cfg(feature = "arachnea-proxy")]
mod transition;
#[cfg(feature = "arachnea-proxy")]
pub use transition::apply_proxy_runtime;

/// Public metadata of an API key, without its secret or verifier.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProxyKeyInfo {
    /// Opaque identifier for revocation.
    pub id: String,
    /// Administrator label.
    pub label: String,
    /// Creation time in Unix seconds.
    pub created_at: u64,
}

/// Requested proxy settings; absent fields remain unchanged.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxySettingsUpdate {
    /// Whether user access is enabled.
    pub enabled: Option<bool>,
    /// Fixed user listening port.
    pub server_port: Option<u16>,
    /// Network mode: local, private or public.
    pub network_mode: Option<String>,
}

/// Requested and effective proxy configuration, without secrets.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProxySettingsResponse {
    /// Requested user access activation.
    pub enabled: bool,
    /// Source of activation setting.
    pub enabled_source: SettingSource,
    /// Requested fixed port, retained while disabled.
    pub server_port: Option<u16>,
    /// Source of requested port.
    pub server_port_source: SettingSource,
    /// Requested network mode, retained while disabled.
    pub network_mode: String,
    /// Source of requested network mode.
    pub network_mode_source: SettingSource,
    /// Actual user access activation.
    pub effective_enabled: bool,
    /// Actual listener port, including OS-selected ephemeral ports.
    pub effective_port: u16,
    /// Actual exposure mode.
    pub effective_network_mode: String,
    /// Last application failure, if any.
    pub apply_error: Option<String>,
    /// Public user-key metadata.
    pub keys: Vec<ProxyKeyInfo>,
}

/// Request to create one proxy API key.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateProxyKeyRequest {
    /// Display label for the new key.
    pub label: String,
}

/// Request to revoke one proxy API key.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteProxyKeyRequest {
    /// Public key identifier.
    pub id: String,
}

/// One-time key creation response; deliberately without Debug.
#[derive(Serialize)]
pub struct CreatedProxyKey {
    /// Public metadata.
    pub key: ProxyKeyInfo,
    /// Secret returned only once.
    pub token: String,
}
