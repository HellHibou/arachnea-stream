use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{bail, Result};

use super::super::query_helpers::{replace_template_placeholders, DynamicTemplateVariables};
use super::super::scraper_data_node::{ScraperDataNode, ScraperOutputType};
use super::node_helpers::{get_node, get_node_mut, lookup_node_scalar_value, set_node, split_path};
use super::types::{ScraperNestedFieldDefinition, ScraperNestedFieldLookup};

/// Validates one `set_nested_fields` post-process definition.
///
/// # Arguments
///
/// * `owner` - Human-readable owner name used in error messages.
/// * `source` - `>`-delimited path of the source group.
/// * `nested_source` - `>`-delimited path of the nested group inside each
///   source item.
/// * `fields` - Field definitions (each must have a target and exactly one of
///   value, copy_from, or host_from; an optional lookup may refine the copied
///   value).
///
/// # Errors
///
/// Returns an error when `source`, `nested_source`, or any field target is
/// empty, when `fields` is empty, when a field does not have exactly one
/// source kind set, or when a lookup definition is incomplete.
pub(super) fn validate(
    owner: &str,
    source: &str,
    nested_source: &str,
    fields: &[ScraperNestedFieldDefinition],
) -> Result<()> {
    if source.trim().is_empty() {
        bail!(
            "set_nested_fields for {} requires a non-empty source",
            owner
        );
    }
    if nested_source.trim().is_empty() {
        bail!(
            "set_nested_fields for {} requires a non-empty nested_source",
            owner
        );
    }
    if fields.is_empty() {
        bail!(
            "set_nested_fields for {} requires at least one field",
            owner
        );
    }
    for field in fields {
        if field.target.trim().is_empty() {
            bail!(
                "set_nested_fields for {} contains a field with empty target",
                owner
            );
        }
        // Count primary sources (value, copy_from, host_from)
        let primary_count = field.value.is_some() as u8
            + field.copy_from.is_some() as u8
            + field.host_from.is_some() as u8;
        // template is a modifier that requires a primary source
        let has_template = field.template.is_some();

        if primary_count == 0 {
            bail!(
                "set_nested_fields for {} field '{}' requires one of value, copy_from, or host_from",
                owner,
                field.target
            );
        }
        if primary_count > 1 {
            bail!(
                "set_nested_fields for {} field '{}' has more than one primary source (value, copy_from, host_from)",
                owner,
                field.target
            );
        }
        if has_template && primary_count == 0 {
            bail!(
                "set_nested_fields for {} field '{}' template requires a primary source (value or copy_from)",
                owner,
                field.target
            );
        }
        if let Some(lookup) = &field.lookup {
            if field.value.is_some() || field.template.is_some() {
                bail!(
                    "set_nested_fields for {} field '{}' cannot combine lookup with value or template",
                    owner,
                    field.target
                );
            }
            if lookup.source.trim().is_empty()
                || lookup.key.trim().is_empty()
                || lookup.value.trim().is_empty()
            {
                bail!(
                    "set_nested_fields for {} field '{}' lookup requires non-empty source, key, and value",
                    owner,
                    field.target
                );
            }
        }
    }
    Ok(())
}

/// Extracts the hostname from a URL string, removing the `www.` prefix.
///
/// # Arguments
///
/// * `url` - Absolute request URL used by the HTTP transport.
fn extract_host(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    Some(host.strip_prefix("www.").unwrap_or(host).to_string())
}

/// Applies a template to a source value, resolving dynamic variables and params.
///
/// The template supports:
/// - `{}` replaced with the source value
/// - `{@var}` replaced with dynamic variables (from `dynamic_variables`)
/// - `{param}` replaced with query parameters (from `params`)
/// - `{request_url}` replaced with the request URL
fn apply_template(
    template: &str,
    source_value: &str,
    params: &HashMap<String, String>,
    request_url: &str,
    dynamic_variables: &Mutex<DynamicTemplateVariables>,
) -> String {
    let text = template.replace("{}", source_value);
    let (text, _) = replace_template_placeholders(&text, params);
    let text = text.replace("{request_url}", request_url);
    // Dynamic variables are resolved by the action pipeline, but here we
    // can't run actions. We'll do a simple replacement for @-prefixed vars.
    let dyn_vars = dynamic_variables.lock().ok();
    if let Some(vars) = dyn_vars {
        let mut text = text;
        for (key, value) in vars.as_map() {
            text = text.replace(&format!("{{{}}}", key), value);
        }
        text
    } else {
        text
    }
}

/// Builds a key/value map from the items of one root group.
///
/// # Arguments
///
/// * `root` - Root node that owns the lookup group.
/// * `lookup` - Lookup definition naming the source group and its key/value
///   fields.
///
/// # Returns
///
/// A map of the scalar key/value pairs found in the lookup group. Missing
/// groups or items without both scalar fields produce an empty/partial map.
fn build_lookup_map(
    root: &ScraperDataNode,
    lookup: &ScraperNestedFieldLookup,
) -> HashMap<String, String> {
    let Some(source_node) = get_node(root, &lookup.source) else {
        return HashMap::new();
    };

    source_node
        .items
        .iter()
        .filter_map(|item| {
            let key = lookup_node_scalar_value(item, &lookup.key)?;
            let value = lookup_node_scalar_value(item, &lookup.value)?;
            Some((key, value))
        })
        .collect()
}

/// Sets one or more fields on every nested item inside every source item.
///
/// For each item in `source`, the `nested_source` sub-array is resolved.  For
/// every nested item, every field definition is applied: a static `value` is
/// written to the field path, a sibling field is copied via `copy_from`, or
/// the hostname from a URL in a sibling field is extracted via `host_from`,
/// or a `template` is applied to the `copy_from` value (or `value`).
/// When a field declares a `lookup`, the resulting value is translated through
/// the referenced root group before it is written.
///
/// # Arguments
///
/// * `root` - Root node that owns the source group.
/// * `source` - `>`-delimited path of the source group.
/// * `nested_source` - `>`-delimited path of the nested group inside each
///   source item.
/// * `fields` - Field definitions to apply on each nested item.
/// * `params` - Runtime template parameters.
/// * `request_url` - Request URL for `{request_url}` placeholder.
/// * `dynamic_variables` - Dynamic variables for `{@var}` placeholders.
pub(super) fn apply(
    root: &mut ScraperDataNode,
    source: &str,
    nested_source: &str,
    fields: &[ScraperNestedFieldDefinition],
    params: &HashMap<String, String>,
    request_url: &str,
    dynamic_variables: &Mutex<DynamicTemplateVariables>,
) {
    let lookup_maps: Vec<Option<HashMap<String, String>>> = fields
        .iter()
        .map(|field| {
            field
                .lookup
                .as_ref()
                .map(|lookup| build_lookup_map(root, lookup))
        })
        .collect();

    let Some(source_node) = get_node_mut(root, source) else {
        return;
    };

    if source_node.items.is_empty() {
        return;
    }

    for source_item in &mut source_node.items {
        let Some(nested_node) = get_node_mut(source_item, nested_source) else {
            continue;
        };

        if nested_node.items.is_empty() {
            continue;
        }

        for nested_item in &mut nested_node.items {
            for (field, lookup_map) in fields.iter().zip(lookup_maps.iter()) {
                // Determine the source value for this field
                let source_value = if let Some(value) = &field.value {
                    // Static value provided directly
                    Some(value.clone())
                } else if let Some(copy_from) = &field.copy_from {
                    // Copy from sibling field
                    get_node(nested_item, copy_from)
                        .and_then(|node| node.values.first())
                        .cloned()
                } else if let Some(host_from) = &field.host_from {
                    // Extract hostname from URL
                    get_node(nested_item, host_from)
                        .and_then(|node| node.values.first())
                        .and_then(|url| extract_host(url))
                } else {
                    None
                };

                let Some(mut source_value) = source_value else {
                    continue;
                };

                // Apply template if provided
                if let Some(template) = &field.template {
                    source_value = apply_template(
                        template,
                        &source_value,
                        params,
                        request_url,
                        dynamic_variables,
                    );
                }

                // Apply lookup if provided (only for copy_from/host_from, not template)
                let final_value = if field.template.is_none() {
                    if let Some(lookup_map) = lookup_map {
                        let key = &source_value;
                        lookup_map.get(key).cloned().unwrap_or_else(|| key.clone())
                    } else {
                        source_value
                    }
                } else {
                    source_value
                };

                set_node(
                    nested_item,
                    &split_path(&field.target),
                    ScraperDataNode::from_values_typed(
                        vec![final_value],
                        ScraperOutputType::String,
                    ),
                );
            }
        }
    }
}
