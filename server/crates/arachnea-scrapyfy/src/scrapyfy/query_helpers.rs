use anyhow::Result;
use regex::{Captures, Regex};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;
use urlencoding::encode;

use super::ScraperDataNode;

static PLACEHOLDER_REGEX: OnceLock<Regex> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryTemplateParamMapping {
    pub target_param: String,
    pub source_param: String,
    #[serde(deserialize_with = "deserialize_mapping_values")]
    pub values: HashMap<String, String>,
    #[serde(default)]
    pub item_suffix: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum QueryTemplateParamMappingValues {
    Map(HashMap<String, String>),
    MapList(Vec<HashMap<String, String>>),
}

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
pub fn resolved_or_template(template: &str, resolved: Option<String>) -> String {
    resolved.unwrap_or_else(|| template.to_string())
}

/// Replaces `{placeholders}` in a template with runtime parameters.
///
/// Missing keys are preserved verbatim in the rendered output and returned as a
/// deduplicated list.
pub fn replace_template_placeholders(
    template: &str,
    params: &HashMap<String, String>,
) -> (String, Vec<String>) {
    let placeholder_re = PLACEHOLDER_REGEX
        .get_or_init(|| Regex::new(r"\{([A-Za-z0-9_]+)\}").expect("Invalid placeholder regex"));
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

fn resolve_nested_template_params(params: &HashMap<String, String>) -> HashMap<String, String> {
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
pub fn build_params_json_value(params: &HashMap<String, String>) -> Value {
    let mut entries = serde_json::Map::new();

    for (key, value) in params {
        entries.insert(key.clone(), Value::String(value.clone()));
    }

    Value::Object(entries)
}

/// Returns whether one root node should be filtered out using root-level filters.
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
