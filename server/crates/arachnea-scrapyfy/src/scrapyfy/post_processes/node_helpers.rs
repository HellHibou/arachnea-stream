use std::collections::HashSet;

use super::super::scraper_data_node::ScraperDataNode;
use super::types::ScraperFieldMapping;

/// Splits a `>`-delimited node path into its individual segments, trimming
/// whitespace and dropping empty segments.
///
/// # Arguments
///
/// * `path` - Path expression such as `"entries > item > name"`.
pub(super) fn split_path(path: &str) -> Vec<&str> {
    path.split('>')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect()
}

/// Walks `root` following `path` and returns the matching node when it exists.
///
/// # Arguments
///
/// * `root` - Root node used as the traversal origin.
/// * `path` - `>`-delimited path relative to `root`.
pub(super) fn get_node<'a>(root: &'a ScraperDataNode, path: &str) -> Option<&'a ScraperDataNode> {
    let mut current = root;
    for segment in split_path(path) {
        current = current.children.get(segment)?;
    }
    Some(current)
}

/// Walks `root` following `path` and returns the matching mutable node when it
/// exists.
///
/// # Arguments
///
/// * `root` - Root node used as the traversal origin.
/// * `path` - `>`-delimited path relative to `root`.
pub(super) fn get_node_mut<'a>(
    root: &'a mut ScraperDataNode,
    path: &str,
) -> Option<&'a mut ScraperDataNode> {
    let mut current = root;
    for segment in split_path(path) {
        current = current.children.get_mut(segment)?;
    }
    Some(current)
}

/// Inserts `node` at the provided `path` inside `root`, creating missing parent
/// containers on the fly. A no-op when `path` is empty.
///
/// # Arguments
///
/// * `root` - Root node that receives the inserted value.
/// * `path` - Pre-split path segments resolved relative to `root`.
/// * `node` - Node written at the resolved target.
pub(super) fn set_node(root: &mut ScraperDataNode, path: &[&str], node: ScraperDataNode) {
    if path.is_empty() {
        return;
    }

    let mut current = root;
    for segment in &path[..path.len().saturating_sub(1)] {
        current = current.children.entry((*segment).to_string()).or_default();
    }

    current
        .children
        .insert(path[path.len().saturating_sub(1)].to_string(), node);
}

/// Copies the node addressed by `source` from `source_root` into the `target`
/// path of `target_root`.
///
/// # Arguments
///
/// * `source_root` - Node the source path is resolved against.
/// * `source` - `>`-delimited path read from `source_root`.
/// * `target_root` - Node that receives the cloned value.
/// * `target` - `>`-delimited path written inside `target_root`.
pub(super) fn copy_field(
    source_root: &ScraperDataNode,
    source: &str,
    target_root: &mut ScraperDataNode,
    target: &str,
) {
    let Some(node) = get_node(source_root, source).cloned() else {
        return;
    };

    set_node(target_root, &split_path(target), node);
}

/// Returns the first non-empty scalar value stored at `path` inside `root`.
///
/// # Arguments
///
/// * `root` - Node searched for the requested field.
/// * `path` - `>`-delimited path read from `root`.
pub(super) fn lookup_node_scalar_value(root: &ScraperDataNode, path: &str) -> Option<String> {
    get_node(root, path).and_then(|node| {
        node.values.iter().find_map(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
    })
}

/// Returns the largest positive integer stored in `node` scalar values.
///
/// # Arguments
///
/// * `node` - Optional node candidate. `None` returns `None`.
pub(super) fn max_positive_usize(node: Option<&ScraperDataNode>) -> Option<usize> {
    node.and_then(|node| {
        node.values
            .iter()
            .filter_map(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .max()
    })
}

/// Parses a YAML-style boolean value (`true`, `1`, `yes`, `y`, case
/// insensitive) into a `bool`. Returns `false` for any other input.
///
/// # Arguments
///
/// * `value` - Raw text to interpret.
pub(super) fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "y"
    )
}

/// Snapshots every `ScraperFieldMapping` value from `source_root` so the caller
/// can apply them later to a freshly built target node.
///
/// # Arguments
///
/// * `source_root` - Node the `source` paths are resolved against.
/// * `copy_item_fields` - Field mappings describing the values to clone.
pub(super) fn prepare_copy_fields(
    source_root: &ScraperDataNode,
    copy_item_fields: &[ScraperFieldMapping],
) -> Vec<(String, ScraperDataNode)> {
    let mut prepared_fields = Vec::new();

    for field in copy_item_fields {
        let Some(node) = get_node(source_root, &field.source).cloned() else {
            continue;
        };

        prepared_fields.push((field.target.clone(), node));
    }

    prepared_fields
}

/// Collects every value stored at `field` across the items of `node` into a
/// deduplicated set, or returns an empty set when `field` is `None`.
///
/// # Arguments
///
/// * `node` - Node whose items are scanned.
/// * `field` - Optional `>`-delimited path read from each item.
pub(super) fn collect_existing_scalar_values(
    node: Option<&ScraperDataNode>,
    field: Option<&str>,
) -> HashSet<String> {
    let Some(field) = field else {
        return HashSet::new();
    };
    let Some(node) = node else {
        return HashSet::new();
    };

    node.items
        .iter()
        .filter_map(|item| lookup_node_scalar_value(item, field))
        .collect()
}
