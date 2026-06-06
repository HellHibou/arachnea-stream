use serde::{Deserialize, Serialize};

/// Privacy controls applied by the core and server adapters.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacyPolicy {
    /// Strip proxy-identifying forwarding headers when relaying HTTP.
    pub strip_proxy_headers: bool,
    /// Avoid reusing sensitive connections across unrelated destinations.
    pub isolate_connections_by_site: bool,
    /// Refuse direct fallback when a privacy route was selected.
    pub prevent_direct_fallback: bool,
    /// Minimize destination and context data in logs.
    pub minimize_logs: bool,
}

impl Default for PrivacyPolicy {
    /// Returns privacy controls that preserve default compatibility.
    fn default() -> Self {
        Self {
            strip_proxy_headers: true,
            isolate_connections_by_site: false,
            prevent_direct_fallback: false,
            minimize_logs: true,
        }
    }
}
