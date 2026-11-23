//! Revision-checked profile settings requests.

use serde::{Deserialize, Serialize};

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
