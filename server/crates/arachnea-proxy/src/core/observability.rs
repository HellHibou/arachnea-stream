use serde::{Deserialize, Serialize};

/// Sensitivity level for data emitted through logs and diagnostics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogSensitivity {
    /// Emit minimal route and error information only.
    Minimal,
    /// Emit operational metadata without secrets.
    Normal,
    /// Emit verbose diagnostics for local debugging.
    Debug,
}

impl Default for LogSensitivity {
    /// Returns redacted logging as the default sensitivity.
    fn default() -> Self {
        Self::Minimal
    }
}

/// Observability controls for adapters and applications.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    /// Log sensitivity level.
    pub log_sensitivity: LogSensitivity,
}

impl Default for ObservabilityConfig {
    /// Returns conservative observability defaults.
    fn default() -> Self {
        Self {
            log_sensitivity: LogSensitivity::Minimal,
        }
    }
}
