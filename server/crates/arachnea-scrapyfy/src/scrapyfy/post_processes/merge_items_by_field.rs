use std::collections::HashMap;

use anyhow::{bail, Result};

use super::super::scraper_data_node::ScraperDataNode;
use super::node_helpers::{get_node, get_node_mut, lookup_node_scalar_value, set_node, split_path};

/// Validates one `merge_items_by_field` post-process definition.
///
/// # Arguments
///
/// * `owner` - Human-readable owner name used in error messages.
/// * `source` - `>`-delimited path of the group whose items are merged.
/// * `field` - Scalar field used as the merge key.
/// * `merge_fields` - Field paths appended from duplicate items into the first
///   matching item.
///
/// # Errors
///
/// Returns an error when `source`, `field`, or any merged field path is empty.
pub(super) fn validate(
    owner: &str,
    source: &str,
    field: &str,
    merge_fields: &[String],
) -> Result<()> {
    if source.trim().is_empty() {
        bail!(
            "merge_items_by_field for {} requires a non-empty source",
            owner
        );
    }
    if field.trim().is_empty() {
        bail!(
            "merge_items_by_field for {} requires a non-empty field",
            owner
        );
    }
    if merge_fields.iter().any(|path| path.trim().is_empty()) {
        bail!(
            "merge_items_by_field for {} contains an empty merge field",
            owner
        );
    }
    Ok(())
}

/// Merges sibling items sharing the same scalar field value into one item.
///
/// Items are visited in extraction order. The first item for a key is kept as
/// the output item for that key; every later item with the same key is folded
/// into it by appending the nodes addressed by `merge_fields` (scalar values
/// are appended once, nested children are merged recursively, and nested item
/// lists are concatenated). Items without a key are kept unchanged so rows that
/// cannot be merged still reach the output.
///
/// # Arguments
///
/// * `root` - Root node that owns `source` and receives the merged group.
/// * `source` - `>`-delimited path of the group whose items are merged.
/// * `field` - Scalar field read from each item as the merge key.
/// * `merge_fields` - Field paths whose content is appended from duplicate
///   items into the first matching item.
pub(super) fn apply(
    root: &mut ScraperDataNode,
    source: &str,
    field: &str,
    merge_fields: &[String],
) {
    let Some(source_node) = get_node(root, source) else {
        return;
    };

    let output_type = source_node.output_type;
    let items = source_node.items.clone();
    let mut merged_items: Vec<ScraperDataNode> = Vec::with_capacity(items.len());
    let mut index_by_key: HashMap<String, usize> = HashMap::new();

    for item in items {
        let Some(key) = lookup_node_scalar_value(&item, field) else {
            merged_items.push(item);
            continue;
        };

        match index_by_key.get(&key) {
            Some(index) => {
                let target = &mut merged_items[*index];
                for path in merge_fields {
                    merge_path(target, &item, path);
                }
            }
            None => {
                index_by_key.insert(key, merged_items.len());
                merged_items.push(item);
            }
        }
    }

    set_node(
        root,
        &split_path(source),
        ScraperDataNode {
            output_type,
            values: Vec::new(),
            children: Default::default(),
            items: merged_items,
        },
    );
}

/// Appends the node addressed by `path` from `source` into `target`.
///
/// The node is created inside `target` when missing. Existing scalar values are
/// preserved, appended values are deduplicated, and nested item lists are
/// concatenated in visit order.
///
/// # Arguments
///
/// * `target` - Item that receives the merged content.
/// * `source` - Duplicate item whose content is appended.
/// * `path` - `>`-delimited path resolved against both items.
fn merge_path(target: &mut ScraperDataNode, source: &ScraperDataNode, path: &str) {
    let Some(source_node) = get_node(source, path).cloned() else {
        return;
    };

    let Some(target_node) = get_node_mut(target, path) else {
        set_node(target, &split_path(path), source_node);
        return;
    };

    for value in source_node.values {
        if !target_node.values.iter().any(|existing| existing == &value) {
            target_node.values.push(value);
        }
    }

    for (name, child) in source_node.children {
        target_node.children.entry(name).or_default().merge(child);
    }

    target_node.items.extend(source_node.items);
}
