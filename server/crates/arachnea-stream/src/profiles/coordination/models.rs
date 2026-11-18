//! Typed application intents for durable forward recovery.

use super::super::common::models::CommonPreferences;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use super::super::common::models::CreateProfile;

/// One complete document replacement guarded by a revision.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsChange<T> {
    /// Current confirmed revision; zero is reserved for initialization.
    pub expected_revision: i64,
    /// Complete validated settings document.
    pub values: T,
}

/// Application intent, stored opaquely by the common repository.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Intent {
    /// Finish publishing a reserved common identity.
    CreateProfile {
        /// Common preferences supplied with creation.
        common: CommonPreferences,
    },
    /// Initialize service settings once, retaining the common UUID.
    InitializeStreaming {
        /// Validated streaming defaults selected by the caller.
        values: Value,
    },
    /// Save one or both preference domains by forward recovery.
    SavePreferences {
        /// Optional common document replacement.
        common: Option<SettingsChange<CommonPreferences>>,
        /// Optional streaming document replacement.
        streaming: Option<SettingsChange<Value>>,
    },
    /// Delete an already blocked profile and all service data.
    DeleteProfile,
    /// Delete an already blocked account and all its profiles.
    DeleteUser,
}
