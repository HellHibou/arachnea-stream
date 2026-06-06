use anyhow::{bail, Result};
use std::collections::HashMap;

use super::super::query_helpers;
use super::super::scraper_data_node::ScraperDataNode;
use super::node_helpers::{collect_existing_scalar_values, get_node, split_path};
use super::types::ScraperPostProcessContext;

/// Validates one `append_static_items` post-process definition.
///
/// # Arguments
///
/// * `owner` - Human-readable owner name used in error messages.
/// * `target` - `>`-delimited path that receives the produced items.
/// * `items` - Static items to append.
/// * `unique_field` - Optional field used to deduplicate items.
///
/// # Errors
///
/// Returns an error when `target` is empty, when `unique_field` is provided as
/// an empty string, or when one of the item field names is empty.
pub(super) fn validate(
    owner: &str,
    target: &str,
    items: &[HashMap<String, String>],
    unique_field: Option<&str>,
) -> Result<()> {
    if target.trim().is_empty() {
        bail!(
            "append_static_items for {} requires a non-empty target",
            owner
        );
    }

    if unique_field
        .map(str::trim)
        .is_some_and(str::is_empty)
    {
        bail!(
            "append_static_items for {} contains an empty unique_field",
            owner
        );
    }

    for item in items {
        for field_name in item.keys() {
            if field_name.trim().is_empty() {
                bail!(
                    "append_static_items for {} contains an empty item field",
                    owner
                );
            }
        }
    }

    Ok(())
}

/// Appends static items to the target group, optionally skipping values that
/// already exist in the group (matched by `unique_field`).
///
/// # Arguments
///
/// * `root` - Root node that owns the target group.
/// * `context` - Runtime context used to resolve placeholders and the
///   `{request_url}` template variable.
/// * `target` - `>`-delimited path of the target group.
/// * `items` - Static items to append.
/// * `unique_field` - Optional field used to deduplicate items.
pub(super) fn apply(
    root: &mut ScraperDataNode,
    context: &ScraperPostProcessContext<'_>,
    target: &str,
    items: &[HashMap<String, String>],
    unique_field: Option<&str>,
) {
    let mut existing_values = collect_existing_scalar_values(get_node(root, target), unique_field);

    let target_path = split_path(target);
    for static_item in items {
        if let Some(unique_field) = unique_field {
            if let Some(value) = static_item.get(unique_field) {
                if existing_values.contains(value) {
                    continue;
                }

                existing_values.insert(value.clone());
            }
        }

        let mut item = ScraperDataNode::default();
        for (field, value) in static_item {
            let (value, _missing_keys) =
                query_helpers::replace_template_placeholders(value, context.params);
            let value = value.replace("{request_url}", context.request_url);
            item.push_value(&split_path(field), value);
        }

        root.push_node(&target_path, item);
    }
}
