use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::super::actions::ScraperAction;

/// Runtime context exposed to query post-processors.
pub struct ScraperPostProcessContext<'a> {
    /// Runtime template values available to actions executed during post-processing.
    pub params: &'a HashMap<String, String>,
    /// URL used to fetch the parent response.
    pub request_url: &'a str,
    /// Optional parent response body available to actions.
    pub response_body: Option<&'a str>,
    /// Shared HTTP client used by follow-up requests.
    pub http_client: &'a super::super::http_client::HttpClient,
}

/// One extracted field used to build an item from a regex match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScraperRegexItemEntry {
    /// Output field name (supports nested `>` paths).
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Capture group index to extract from the regex match.
    pub capture_group: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Actions applied on extracted values before insertion.
    pub actions: Vec<ScraperAction>,
}

/// Copies one field from a source node to a target node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScraperFieldMapping {
    /// Source path read from the current source/root item.
    pub source: String,
    /// Target path written into the generated item.
    pub target: String,
}

/// Generates one scalar field from the current 1-based index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScraperGeneratedField {
    /// Output field name generated for each indexed target item.
    pub name: String,
    /// String template where `{}` is replaced by the 1-based index.
    pub format: String,
}

/// Returns the default field name used by `derive_pagination` to expose the
/// resolved entries group.
pub(crate) fn default_entries_field() -> String {
    "entries".to_string()
}

/// Returns the default field name used by `derive_pagination` to expose the
/// current page number.
pub(crate) fn default_current_page_field() -> String {
    "current_page".to_string()
}

/// Returns the default field name used by `derive_pagination` to expose the
/// "have more" boolean.
pub(crate) fn default_have_more_field() -> String {
    "have_more".to_string()
}

/// Returns the default parameter name used by `derive_pagination` to expose
/// the next page value inside `source_params`.
pub(crate) fn default_next_param() -> String {
    "page".to_string()
}

/// Returns the default target field name used by `derive_pagination` for the
/// `source_params` payload.
pub(crate) fn default_source_params_target() -> String {
    "source_params".to_string()
}

/// Scope used to resolve one math expression variable.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperComputedFieldVariableScope {
    /// Resolves the variable against the current item, then its parent item,
    /// then the root node, then query params.
    #[default]
    Auto,
    /// Resolves the variable against the item currently being updated.
    Current,
    /// Resolves the variable against the parent item that owns the nested source.
    Parent,
    /// Resolves the variable against the query root node.
    Root,
    /// Resolves the variable against runtime query params.
    Params,
}

impl ScraperComputedFieldVariableScope {
    /// Returns `true` when the scope is the implicit `Auto` variant.
    ///
    /// Used by serde to omit the field from serialized payloads when the
    /// default behavior is selected.
    pub(crate) fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }
}

/// Variable mapping used by one computed scalar field expression.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScraperComputedFieldVariable {
    /// Path or param name resolved for this variable. Defaults to the variable name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Scope used to resolve the path.
    #[serde(
        default,
        skip_serializing_if = "ScraperComputedFieldVariableScope::is_auto"
    )]
    pub scope: ScraperComputedFieldVariableScope,
}
