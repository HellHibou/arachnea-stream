//! Transformations applied to an HTTP response body before parsing and validation.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A transformation applied to a response body before it is parsed.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PreProcessAction {
    /// Removes every literal block delimited by `start` and `end`, including both delimiters.
    RemoveTextBlocks {
        /// Literal marker that starts a removed block after `{base_url}` resolution.
        start: String,
        /// Literal marker that ends a removed block after `{base_url}` resolution.
        end: String,
    },
}

impl PreProcessAction {
    /// Validates the action configuration for a query.
    ///
    /// # Errors
    ///
    /// Returns an error when a delimiter is empty.
    pub fn validate(&self, query_name: &str) -> Result<()> {
        match self {
            Self::RemoveTextBlocks { start, end } => {
                if start.is_empty() || end.is_empty() {
                    anyhow::bail!(
                        "Invalid pre_process for query `{query_name}`: remove_text_blocks delimiters cannot be empty"
                    );
                }
            }
        }
        Ok(())
    }

    fn apply(
        &self,
        body: String,
        query_name: &str,
        base_url: &str,
        params: &HashMap<String, String>,
    ) -> Result<String> {
        match self {
            Self::RemoveTextBlocks { start, end } => {
                let start = resolve_base_url(start, base_url, params);
                let end = resolve_base_url(end, base_url, params);
                if start.is_empty() || end.is_empty() {
                    anyhow::bail!(
                        "Pre-process remove_text_blocks for query `{query_name}` resolved an empty delimiter"
                    );
                }
                remove_text_blocks(body, &start, &end, query_name)
            }
        }
    }
}

/// Applies pre-processing actions in declaration order.
///
/// The `start` and `end` delimiters may use `{base_url}`, resolved from the
/// runtime parameter when present and otherwise from the query base URL.
///
/// # Arguments
///
/// * `body` - HTTP response body to transform.
/// * `actions` - Ordered transformations declared by the query.
/// * `query_name` - Query name used in diagnostics.
/// * `base_url` - Source base URL injected as `{base_url}` when absent from `params`.
/// * `params` - Runtime parameters used to resolve `{base_url}`.
///
/// # Errors
///
/// Returns an error when a resolved delimiter is empty or a configured block
/// starts without a matching end delimiter.
pub fn apply_pre_processes(
    body: String,
    actions: &[PreProcessAction],
    query_name: &str,
    base_url: &str,
    params: &HashMap<String, String>,
) -> Result<String> {
    actions.iter().try_fold(body, |body, action| {
        action.apply(body, query_name, base_url, params)
    })
}

/// Resolves the supported `{base_url}` placeholder without interpreting other
/// literal brace-delimited text in block markers as templates.
fn resolve_base_url(value: &str, base_url: &str, params: &HashMap<String, String>) -> String {
    let base_url = params
        .get("base_url")
        .map(String::as_str)
        .unwrap_or(base_url);
    value.replace("{base_url}", base_url)
}

fn remove_text_blocks(body: String, start: &str, end: &str, query_name: &str) -> Result<String> {
    let mut result = String::with_capacity(body.len());
    let mut remaining = body.as_str();

    while let Some(start_index) = remaining.find(start) {
        result.push_str(&remaining[..start_index]);
        let after_start = &remaining[start_index + start.len()..];
        let Some(end_index) = after_start.find(end) else {
            anyhow::bail!(
                "Pre-process remove_text_blocks for query `{query_name}` found its start delimiter without a matching end delimiter"
            );
        };
        remaining = &after_start[end_index + end.len()..];
    }

    result.push_str(remaining);
    Ok(result)
}
