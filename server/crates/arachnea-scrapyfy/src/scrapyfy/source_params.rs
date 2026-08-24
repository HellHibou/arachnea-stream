//! Source-scoped request parameter parsing helpers.
//!
//! Controller request payloads carry optional per-source runtime parameters
//! as a flat list of entries (`source` plus arbitrary flattened JSON fields).
//! This module converts them into the engine-level [`ScraperSourceParams`]
//! map consumed by query execution.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use super::ScraperSourceParams;

/// One per-source runtime parameter entry as received in controller requests.
#[derive(Clone, Serialize, Deserialize)]
pub struct ScraperSourceParamsRequestEntry {
    /// Source name the remaining flattened fields apply to.
    pub source: String,
    /// Arbitrary runtime parameters for that source.
    #[serde(flatten)]
    pub params: HashMap<String, Value>,
}

/// Converts flat request entries into source-scoped runtime parameters.
///
/// Entries with a blank source name are skipped, and a nested `source`
/// field is dropped so it never leaks into execution parameters.
pub fn source_params_from_entries(
    entries: Vec<ScraperSourceParamsRequestEntry>,
) -> ScraperSourceParams {
    let mut source_params = ScraperSourceParams::new();

    for entry in entries {
        let source = entry.source.trim().to_string();
        if source.is_empty() {
            continue;
        }

        let params = source_params.entry(source).or_default();
        for (key, value) in entry.params {
            if key == "source" {
                continue;
            }

            if let Some(value) = request_param_value_to_string(value) {
                params.insert(key, value);
            }
        }
    }

    source_params
}

/// Converts one JSON request parameter into its string form for execution.
fn request_param_value_to_string(value: Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Array(_) | Value::Object(_) => Some(value.to_string()),
    }
}
