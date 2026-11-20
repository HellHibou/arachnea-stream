use anyhow::{bail, Result};
use serde_json::Value;

use super::super::scraper_data_node::{ScraperDataNode, ScraperOutputType};
use super::node_helpers::{get_node, get_node_mut, set_node, split_path};

/// Validates paths and fallback options for one `offset_items_field` step.
///
/// # Arguments
///
/// * `owner` - Query or entry name for validation errors.
/// * `source` - Path to the root group.
/// * `nested_source` - Optional nested group path relative to each parent item.
/// * `field` - Field path relative to the offset item.
/// * `target` - Field path relative to the current item.
/// * `default` - Optional literal fallback value.
/// * `default_field` - Optional root field supplying the fallback.
///
/// # Errors
///
/// Returns an error for empty paths, a non-scalar fallback, or conflicting fallbacks.
pub(super) fn validate(
    owner: &str,
    source: &str,
    nested_source: Option<&str>,
    field: &str,
    target: &str,
    default: Option<&Value>,
    default_field: Option<&str>,
) -> Result<()> {
    for (name, path) in [
        ("source", Some(source)),
        ("nested_source", nested_source),
        ("field", Some(field)),
        ("target", Some(target)),
        ("default_field", default_field),
    ] {
        if path.is_some_and(|path| split_path(path).is_empty()) {
            bail!("offset_items_field for {owner} requires a non-empty {name}");
        }
    }
    if default.is_some() && default_field.is_some() {
        bail!("offset_items_field for {owner} accepts either default or default_field");
    }
    if default
        .is_some_and(|value| !matches!(value, Value::String(_) | Value::Number(_) | Value::Bool(_)))
    {
        bail!("offset_items_field for {owner} requires a string, number, or boolean default");
    }
    Ok(())
}

/// Copies fields within each selected group, preserving their output types.
/// Items without a relative field or a fallback keep their current target.
///
/// # Arguments
///
/// * `root` - Result tree to update and origin for `default_field`.
/// * `source` - Path to the root group.
/// * `nested_source` - Optional nested group path relative to each parent item.
/// * `field` - Field path relative to the offset item.
/// * `target` - Field path relative to the current item.
/// * `offset` - Signed index difference within the group.
/// * `default` - Optional literal fallback value.
/// * `default_field` - Optional root field supplying the fallback.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    root: &mut ScraperDataNode,
    source: &str,
    nested_source: Option<&str>,
    field: &str,
    target: &str,
    offset: isize,
    default: Option<&Value>,
    default_field: Option<&str>,
) {
    let fallback = default_field
        .and_then(|path| get_node(root, path))
        .cloned()
        .or_else(|| default.and_then(scalar_node));
    let Some(source_node) = get_node_mut(root, source) else {
        return;
    };

    let target_path = split_path(target);
    if let Some(nested_source) = nested_source {
        for parent in &mut source_node.items {
            if let Some(group) = get_node_mut(parent, nested_source) {
                copy_fields(
                    &mut group.items,
                    field,
                    &target_path,
                    offset,
                    fallback.as_ref(),
                );
            }
        }
    } else {
        copy_fields(
            &mut source_node.items,
            field,
            &target_path,
            offset,
            fallback.as_ref(),
        );
    }
}

/// Copies from a snapshot of the group so overlapping source and target fields
/// read the original values.
///
/// # Arguments
///
/// * `items` - Ordered group items to update.
/// * `field` - Path to read from the offset item.
/// * `target_path` - Pre-split path to write inside the current item.
/// * `offset` - Signed index difference within this group.
/// * `fallback` - Value to use for missing items or fields.
fn copy_fields(
    items: &mut [ScraperDataNode],
    field: &str,
    target_path: &[&str],
    offset: isize,
    fallback: Option<&ScraperDataNode>,
) {
    let values: Vec<_> = (0..items.len())
        .map(|index| {
            index
                .checked_add_signed(offset)
                .and_then(|index| items.get(index))
                .and_then(|item| get_node(item, field))
                .or(fallback)
                .cloned()
        })
        .collect();

    for (item, value) in items.iter_mut().zip(values) {
        if let Some(value) = value {
            set_node(item, target_path, value);
        }
    }
}

/// Converts a YAML scalar fallback into a node with the corresponding JSON type.
///
/// # Arguments
///
/// * `value` - Literal fallback supplied by the configuration.
fn scalar_node(value: &Value) -> Option<ScraperDataNode> {
    let (text, output_type) = match value {
        Value::String(value) => (value.clone(), ScraperOutputType::String),
        Value::Number(value) => (value.to_string(), ScraperOutputType::Number),
        Value::Bool(value) => (value.to_string(), ScraperOutputType::Boolean),
        _ => return None,
    };
    Some(ScraperDataNode::from_values_typed(vec![text], output_type))
}
