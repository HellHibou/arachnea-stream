use anyhow::{Context, Result};
use regex::{Captures, Regex};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;
use urlencoding::encode;

use super::ScraperDataNode;
use crate::scrapyfy::actions::ScraperAction;
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;

/// Compiled regex matching `{placeholder}` tokens in template strings.
static PLACEHOLDER_REGEX: OnceLock<Regex> = OnceLock::new();

/// Prefix reserved for variables produced while a scraper request is being extracted.
pub const DYNAMIC_TEMPLATE_VARIABLE_PREFIX: &str = "@";

/// Request-scoped template variables produced by extraction actions.
///
/// Dynamic variables are intentionally separated from collection/runtime
/// parameters. Every dynamic name must start with [`DYNAMIC_TEMPLATE_VARIABLE_PREFIX`]
/// and duplicates are rejected by default so extraction pipelines cannot silently
/// shadow previous values.
#[derive(Debug, Clone, Default)]
pub struct DynamicTemplateVariables {
    values: HashMap<String, String>,
}

impl DynamicTemplateVariables {
    /// Creates an empty dynamic variable set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one dynamic variable after validating its reserved prefix and collisions.
    ///
    /// # Arguments
    ///
    /// * `standard_params` - Standard runtime parameters that must not be overwritten.
    /// * `name` - Dynamic variable name. Must start with `@` and contain at least one
    ///   character after the prefix.
    /// * `value` - Textual value stored for placeholder replacement.
    ///
    /// # Errors
    ///
    /// Returns an error when the name is empty, lacks the `@` prefix, collides with
    /// a standard parameter, or duplicates an existing dynamic variable.
    pub fn insert(
        &mut self,
        standard_params: &HashMap<String, String>,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<()> {
        let name = name.into();
        validate_dynamic_template_variable_name(&name)?;

        if standard_params.contains_key(&name) {
            anyhow::bail!(
                "Dynamic template variable {} cannot overwrite a standard parameter",
                name
            );
        }

        if self.values.contains_key(&name) {
            anyhow::bail!("Duplicate dynamic template variable {}", name);
        }

        self.values.insert(name, value.into());
        Ok(())
    }

    /// Adds or replaces one dynamic variable after validating its reserved prefix.
    ///
    /// This is intended for extraction pipelines that deliberately read the same
    /// request-scoped declarations more than once. Standard parameters remain
    /// protected from overwrite.
    pub fn insert_or_replace(
        &mut self,
        standard_params: &HashMap<String, String>,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<()> {
        let name = name.into();
        validate_dynamic_template_variable_name(&name)?;

        if standard_params.contains_key(&name) {
            anyhow::bail!(
                "Dynamic template variable {} cannot overwrite a standard parameter",
                name
            );
        }

        self.values.insert(name, value.into());
        Ok(())
    }

    /// Returns the stored dynamic variables.
    pub fn as_map(&self) -> &HashMap<String, String> {
        &self.values
    }

    /// Returns whether no dynamic variables are currently stored.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// Validates one dynamic template variable name.
///
/// # Errors
///
/// Returns an error when the name does not use the reserved `@` namespace.
pub fn validate_dynamic_template_variable_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        anyhow::bail!("Dynamic template variable name cannot be empty");
    }

    if !name.starts_with(DYNAMIC_TEMPLATE_VARIABLE_PREFIX) {
        anyhow::bail!(
            "Dynamic template variable {} must start with {}",
            name,
            DYNAMIC_TEMPLATE_VARIABLE_PREFIX
        );
    }

    if name == DYNAMIC_TEMPLATE_VARIABLE_PREFIX {
        anyhow::bail!("Dynamic template variable name must include a name after @");
    }

    Ok(())
}

/// Builds a template parameter map containing standard and dynamic variables.
///
/// Dynamic variables keep their `@` prefix and can therefore be referenced as
/// `{@name}`. Standard parameters are rejected if they use the reserved dynamic
/// namespace, preventing a standard value from overwriting an extracted value.
///
/// # Errors
///
/// Returns an error when a standard parameter uses the reserved `@` prefix or a
/// dynamic variable collides with a standard parameter.
pub fn build_template_params_with_dynamic_variables(
    standard_params: &HashMap<String, String>,
    dynamic_variables: &DynamicTemplateVariables,
) -> Result<HashMap<String, String>> {
    let mut merged = standard_params.clone();

    for key in standard_params.keys() {
        if key.starts_with(DYNAMIC_TEMPLATE_VARIABLE_PREFIX) {
            anyhow::bail!(
                "Standard template parameter {} cannot use reserved dynamic prefix {}",
                key,
                DYNAMIC_TEMPLATE_VARIABLE_PREFIX
            );
        }
    }

    for (key, value) in dynamic_variables.as_map() {
        validate_dynamic_template_variable_name(key)
            .with_context(|| format!("Invalid dynamic template variable {}", key))?;
        if merged.contains_key(key) {
            anyhow::bail!(
                "Dynamic template variable {} cannot overwrite a standard parameter",
                key
            );
        }
        merged.insert(key.clone(), value.clone());
    }

    Ok(merged)
}

/// Defines how one source parameter is mapped into a target parameter through
/// a value translation table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryTemplateParamMapping {
    /// Output parameter name receiving the mapped value.
    pub target_param: String,
    /// Input parameter name whose comma-separated values are translated.
    pub source_param: String,
    /// Translation table mapping each source value to its target replacement.
    /// A `{}` token in the replacement is substituted with the URL-encoded source value.
    #[serde(deserialize_with = "deserialize_mapping_values")]
    pub values: HashMap<String, String>,
    /// Suffix appended to every translated item before concatenation.
    #[serde(default)]
    pub item_suffix: String,
}

/// Intermediate deserialization helper that accepts either a single map or a list
/// of maps for the `values` field of [`QueryTemplateParamMapping`].
#[derive(Deserialize)]
#[serde(untagged)]
enum QueryTemplateParamMappingValues {
    Map(HashMap<String, String>),
    MapList(Vec<HashMap<String, String>>),
}

/// Deserializes the `values` field accepting both a single map and a list of maps.
///
/// When a list is provided, all entries are flattened into a single map with
/// later entries overriding earlier ones.
///
/// # Arguments
///
/// * `deserializer` - Serde deserializer reading the YAML/JSON value.
fn deserialize_mapping_values<'de, D>(deserializer: D) -> Result<HashMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(
        match QueryTemplateParamMappingValues::deserialize(deserializer)? {
            QueryTemplateParamMappingValues::Map(values) => values,
            QueryTemplateParamMappingValues::MapList(values) => {
                values.into_iter().flatten().collect()
            }
        },
    )
}

/// Applies parameter mappings to produce a resolved template parameter map.
///
/// Each mapping takes a comma-separated source parameter, translates each
/// fragment through the mapping's value table (URL-encoding it when `{}` is
/// present), appends the item suffix, and inserts the concatenated result
/// under the target parameter name.
///
/// # Arguments
///
/// * `params` - Runtime template parameters to read source values from.
/// * `mappings` - Ordered list of parameter mappings to apply.
pub fn build_template_params(
    params: &HashMap<String, String>,
    mappings: &[QueryTemplateParamMapping],
) -> HashMap<String, String> {
    let mut resolved = params.clone();

    for mapping in mappings {
        let fragments = params
            .get(&mapping.source_param)
            .map(|raw_value| {
                raw_value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .filter_map(|value| mapping.values.get(value).map(|mapped| (value, mapped)))
                    .map(|(value, mapped_value)| {
                        let mapped_value: Cow<'_, str> = if mapped_value.contains("{}") {
                            Cow::Owned(mapped_value.replace("{}", &encode(value)))
                        } else {
                            Cow::Borrowed(mapped_value.as_str())
                        };

                        format!("{}{}", mapped_value, mapping.item_suffix)
                    })
                    .collect::<String>()
            })
            .unwrap_or_default();

        resolved.insert(mapping.target_param.clone(), fragments);
    }

    resolved
}

/// Resolves one template field against collection-level parameters.
///
/// # Arguments
///
/// * `owner_kind` - Human-readable owner kind used in error messages.
/// * `owner_name` - Owner name used in error messages.
/// * `field_name` - Field name being resolved.
/// * `template` - Template string containing `{placeholders}`.
/// * `params` - Collection-level parameters available to the template.
///
/// # Errors
///
/// Returns an error if one or more placeholders are missing from `params`.
pub fn resolve_required_template(
    owner_kind: &str,
    owner_name: &str,
    field_name: &str,
    template: &str,
    params: &HashMap<String, String>,
) -> Result<String> {
    let (resolved, missing_keys) = replace_template_placeholders(template, params);

    if !missing_keys.is_empty() {
        anyhow::bail!(
            "Missing collection params for {} {} {}: {}",
            owner_kind,
            owner_name,
            field_name,
            missing_keys.join(", ")
        );
    }

    Ok(resolved)
}

/// Returns the resolved template when present, otherwise the original template.
///
/// # Arguments
///
/// * `template` - Original template string used as fallback.
/// * `resolved` - Optional pre-resolved value.
pub fn resolved_or_template(template: &str, resolved: Option<String>) -> String {
    resolved.unwrap_or_else(|| template.to_string())
}

/// Replaces `{placeholders}` in a template with runtime parameters.
///
/// Missing keys are preserved verbatim in the rendered output and returned as a
/// deduplicated list.
///
/// # Arguments
///
/// * `template` - Template string containing `{placeholder}` tokens.
/// * `params` - Runtime parameters used to replace placeholders.
pub fn replace_template_placeholders(
    template: &str,
    params: &HashMap<String, String>,
) -> (String, Vec<String>) {
    let placeholder_re = PLACEHOLDER_REGEX
        .get_or_init(|| Regex::new(r"\{(@?[A-Za-z0-9_]+)\}").expect("Invalid placeholder regex"));
    let mut missing_keys: Vec<String> = Vec::new();

    let rendered = placeholder_re
        .replace_all(template, |captures: &Captures| {
            let key = &captures[1];
            match params.get(key) {
                Some(value) => value.clone(),
                None => {
                    missing_keys.push(key.to_string());
                    captures[0].to_string()
                }
            }
        })
        .to_string();

    missing_keys.sort();
    missing_keys.dedup();

    (rendered, missing_keys)
}

/// Iteratively resolves nested template placeholders until no further expansion
/// occurs, preventing infinite loops by capping at `params.len() + 1` passes.
///
/// # Arguments
///
/// * `params` - Template parameters that may reference other parameters.
pub(crate) fn resolve_nested_template_params(
    params: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut resolved = params.clone();
    let max_iterations = resolved.len().saturating_add(1);

    for _ in 0..max_iterations {
        let snapshot = resolved.clone();
        let mut changed = false;

        for (key, value) in snapshot {
            let (rendered, _missing_keys) = replace_template_placeholders(&value, &resolved);

            if rendered != value {
                resolved.insert(key, rendered);
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }

    resolved
}

/// Replaces `{placeholders}` in a query template with runtime parameters.
///
/// `{base_url}` is reserved and always resolved from `base_url`.
///
/// # Arguments
///
/// * `base_url` - Base URL injected as `{base_url}` when absent from `params`.
/// * `template` - Query URL template containing `{placeholder}` tokens.
/// * `params` - Runtime parameters used to replace placeholders.
///
/// # Errors
///
/// Returns an error if any placeholder is missing from the resolved parameter set.
pub fn format_query_template(
    base_url: &str,
    template: &str,
    params: &HashMap<String, String>,
) -> Result<String> {
    let mut runtime_params = params.clone();
    runtime_params
        .entry("base_url".to_string())
        .or_insert_with(|| base_url.to_string());
    let runtime_params = resolve_nested_template_params(&runtime_params);
    let (query, missing_keys) = replace_template_placeholders(template, &runtime_params);

    if !missing_keys.is_empty() {
        anyhow::bail!("Missing query params: {}", missing_keys.join(", "));
    }

    Ok(query)
}

/// Builds the runtime template map used by query URLs and actions.
///
/// This applies query-specific param mappings and always exposes `{base_url}`.
///
/// # Arguments
///
/// * `base_url` - Base URL injected as `{base_url}` when absent from `params`.
/// * `params` - Runtime parameters to apply mappings against.
/// * `mappings` - Query-specific parameter mappings to apply.
pub fn build_query_execution_params(
    base_url: &str,
    params: &HashMap<String, String>,
    mappings: &[QueryTemplateParamMapping],
) -> HashMap<String, String> {
    let mut resolved = build_template_params(params, mappings);
    resolved
        .entry("base_url".to_string())
        .or_insert_with(|| base_url.to_string());
    resolve_nested_template_params(&resolved)
}

/// Converts runtime query parameters into a JSON object made of string values.
///
/// This helper is used when a root query needs to resolve request headers or a
/// request body before any remote payload exists.
///
/// # Arguments
///
/// * `params` - Runtime template parameters to convert.
pub fn build_params_json_value(params: &HashMap<String, String>) -> Value {
    let mut entries = serde_json::Map::new();

    for (key, value) in params {
        entries.insert(key.clone(), Value::String(value.clone()));
    }

    Value::Object(entries)
}

/// Returns whether one root node should be filtered out using root-level filters.
///
/// A root is considered filtered when any required key is missing or when no
/// value for a required key matches the expected list.
///
/// # Arguments
///
/// * `root` - Extracted data node to test.
/// * `fields_filters` - Map of field names to acceptable value lists.
pub fn is_root_filtered(
    root: &ScraperDataNode,
    fields_filters: &HashMap<String, Vec<String>>,
) -> bool {
    for (key, values) in fields_filters {
        let entry_value = match root.children.get(key) {
            Some(value) => value,
            None => return true,
        };

        if !values
            .iter()
            .any(|value| entry_value.values.contains(value))
        {
            return true;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Request validation and resolution helpers (shared by HTML/JSON scrapers)
// ---------------------------------------------------------------------------

/// Validates every action against the query-level contract.
///
/// # Arguments
///
/// * `context` - Context identifier used in diagnostic messages (e.g., "query" or "sub-query").
/// * `name` - Query name used in diagnostic messages.
/// * `actions` - Actions to validate.
///
/// # Errors
///
/// Returns an error if any action fails validation for the given `context`.
pub fn validate_request_actions(
    context: &str,
    name: &str,
    actions: &[ScraperAction],
) -> Result<()> {
    for action in actions {
        action.validate(name, context)?;
    }
    Ok(())
}

/// Resolves request headers from a context row and template parameters.
///
/// Selects header values from the context using each header's pointer,
/// applies its actions, and returns a map of header names to resolved values.
/// Headers without a resolved value are silently skipped.
///
/// # Arguments
///
/// * `headers` - List of header configurations to resolve.
/// * `context_row` - JSON value used for header pointer resolution.
/// * `params` - Runtime template parameters.
/// * `context_request_url` - URL of the parent request for action pipelines.
///
/// # Example
///
/// ```ignore
/// let headers = vec![ScraperRequestHeader {
///     name: "Authorization".to_string(),
///     pointer: Some("auth.token".to_string()),
///     select: HtmlScraperSelectMode::First,
///     actions: vec![],
/// }];
/// let resolved = resolve_request_headers(&headers, &context_json, &params, "https://example.com")?;
/// // resolved = {"Authorization": "Bearer xxx"}
/// ```
pub fn resolve_request_headers(
    headers: &[crate::scrapyfy::scraper::config::ScraperRequestHeader],
    context_row: &Value,
    params: &HashMap<String, String>,
    context_request_url: &str,
) -> HashMap<String, String> {
    let mut resolved = HashMap::new();

    for header in headers {
        if let Some((name, value)) = header.resolve(context_row, params, context_request_url) {
            resolved.insert(name, value);
        }
    }

    resolved
}

/// Resolves the request body by selecting a JSON pointer value from the context
/// and applying the configured body actions.
///
/// # Arguments
///
/// * `body_pointer` - Optional JSON pointer selecting the request body from the context.
/// * `body_select` - Selection mode for the body pointer.
/// * `body_actions` - Actions applied to the selected body values.
/// * `context_row` - JSON value used for body pointer resolution.
/// * `params` - Runtime template parameters.
/// * `context_request_url` - URL of the parent request for action pipelines.
///
/// # Returns
///
/// The first non-empty trimmed body value after applying all actions,
/// or `None` if no value was resolved.
pub fn resolve_request_body(
    body_pointer: Option<&str>,
    body_select: HtmlScraperSelectMode,
    body_actions: &[ScraperAction],
    context_row: &Value,
    params: &HashMap<String, String>,
    context_request_url: &str,
) -> Option<String> {
    use crate::scrapyfy::scraper_json::entry::{json_value_to_strings, select_json_values};

    let mut values = select_json_values(context_row, body_pointer, body_select)
        .into_iter()
        .flat_map(json_value_to_strings)
        .collect::<Vec<_>>();

    for action in body_actions {
        values = action.apply(&None, values, params, context_request_url, None, None);
    }

    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_nested_template_params_resolves_chained_collection_params() {
        let params = HashMap::from([
            ("catalog_code".to_string(), "fr-fr".to_string()),
            (
                "android_graphql_url".to_string(),
                "https://app-api.tf1.fr/graphql/{catalog_code}/android".to_string(),
            ),
            (
                "android_home_covers_url".to_string(),
                "{android_graphql_url}?id=abc".to_string(),
            ),
        ]);

        let resolved = resolve_nested_template_params(&params);

        assert_eq!(
            resolved.get("android_graphql_url").map(String::as_str),
            Some("https://app-api.tf1.fr/graphql/fr-fr/android")
        );
        assert_eq!(
            resolved.get("android_home_covers_url").map(String::as_str),
            Some("https://app-api.tf1.fr/graphql/fr-fr/android?id=abc")
        );
    }

    #[test]
    fn replace_template_placeholders_resolves_dynamic_prefixed_keys() {
        let params = HashMap::from([
            ("country".to_string(), "BE".to_string()),
            ("@PortPart".to_string(), "80".to_string()),
        ]);

        let (rendered, missing) = replace_template_placeholders("{country}:{@PortPart}", &params);

        assert_eq!(rendered, "BE:80");
        assert!(missing.is_empty());
    }

    #[test]
    fn dynamic_template_variables_reject_missing_prefix() {
        let params = HashMap::new();
        let mut dynamic = DynamicTemplateVariables::new();

        let error = dynamic.insert(&params, "PortPart", "80").unwrap_err();

        assert!(error.to_string().contains("must start with @"));
    }

    #[test]
    fn dynamic_template_variables_reject_duplicates() {
        let params = HashMap::new();
        let mut dynamic = DynamicTemplateVariables::new();

        dynamic.insert(&params, "@PortPart", "80").unwrap();
        let error = dynamic.insert(&params, "@PortPart", "81").unwrap_err();

        assert!(error
            .to_string()
            .contains("Duplicate dynamic template variable"));
    }

    #[test]
    fn build_template_params_with_dynamic_variables_keeps_namespaces_separate() {
        let params = HashMap::from([("country".to_string(), "BE".to_string())]);
        let mut dynamic = DynamicTemplateVariables::new();

        dynamic.insert(&params, "@PortPart", "80").unwrap();
        let merged = build_template_params_with_dynamic_variables(&params, &dynamic).unwrap();

        assert_eq!(merged.get("country").map(String::as_str), Some("BE"));
        assert_eq!(merged.get("@PortPart").map(String::as_str), Some("80"));
    }

    #[test]
    fn build_template_params_with_dynamic_variables_rejects_standard_dynamic_prefix() {
        let params = HashMap::from([("@PortPart".to_string(), "80".to_string())]);
        let dynamic = DynamicTemplateVariables::new();

        let error = build_template_params_with_dynamic_variables(&params, &dynamic).unwrap_err();

        assert!(error
            .to_string()
            .contains("cannot use reserved dynamic prefix"));
    }
}
