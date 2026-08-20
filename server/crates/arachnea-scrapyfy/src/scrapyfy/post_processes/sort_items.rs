use serde::{Deserialize, Serialize};

use super::super::scraper_data_node::ScraperDataNode;
use super::node_helpers::{get_node, get_node_mut};

/// Sorting strategy used by the `sort_items` post-process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperSortMethod {
    /// Reverses the current item order without comparing field values.
    Reverse,
    /// Sorts items in ascending order on the target field value.
    Asc,
    /// Sorts items in descending order on the target field value.
    Desc,
}

/// Validates one `sort_items` post-process definition.
///
/// # Arguments
///
/// * `owner` - Human-readable owner name used in error messages.
/// * `source` - `>`-delimited path of the group whose items are sorted.
/// * `field` - Scalar field read from each item when `method` is `asc` or `desc`.
///
/// # Errors
///
/// Returns an error when `source` or `field` is empty.
pub(super) fn validate(owner: &str, source: &str, field: &str) -> Result<(), anyhow::Error> {
    if source.trim().is_empty() {
        anyhow::bail!("Empty source path in sort_items post-process for {}", owner);
    }
    if field.trim().is_empty() {
        anyhow::bail!("Empty field path in sort_items post-process for {}", owner);
    }
    Ok(())
}

/// Sorts the items of `source` according to `method`.
///
/// `reverse` simply inverts the current item order. `asc` and `desc` compare
/// the first scalar value stored at `field` inside each item, using numeric
/// comparison when both values parse as integers and lexicographic comparison
/// otherwise. Items without a value for `field` keep their relative position
/// at the start of the comparison.
///
/// # Arguments
///
/// * `root` - Root node that owns the source group.
/// * `source` - `>`-delimited path of the group whose items are sorted.
/// * `field` - `>`-delimited path of the scalar field used for comparison.
/// * `method` - Sorting strategy applied to the group items.
pub(super) fn apply(
    root: &mut ScraperDataNode,
    source: &str,
    field: &str,
    method: ScraperSortMethod,
) {
    let Some(source_node) = get_node_mut(root, source) else {
        return;
    };

    match method {
        ScraperSortMethod::Reverse => source_node.items.reverse(),
        ScraperSortMethod::Asc => {
            source_node
                .items
                .sort_by(|left, right| compare_items(left, right, field));
        }
        ScraperSortMethod::Desc => {
            source_node
                .items
                .sort_by(|left, right| compare_items(right, left, field));
        }
    }
}

/// Compares two items on the first scalar value stored at `field`.
///
/// Uses numeric comparison when both values parse as integers, otherwise
/// falls back to lexicographic comparison. Items with no value compare equal.
fn compare_items(
    left: &ScraperDataNode,
    right: &ScraperDataNode,
    field: &str,
) -> std::cmp::Ordering {
    let left_value = get_node(left, field)
        .and_then(|node| node.values.first())
        .map(String::as_str)
        .unwrap_or_default();
    let right_value = get_node(right, field)
        .and_then(|node| node.values.first())
        .map(String::as_str)
        .unwrap_or_default();

    match (left_value.parse::<i64>(), right_value.parse::<i64>()) {
        (Ok(left_value), Ok(right_value)) => left_value.cmp(&right_value),
        _ => left_value.cmp(right_value),
    }
}
