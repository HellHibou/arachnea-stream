use anyhow::{Context, Result};
use regex::Regex;

use super::super::scraper_data_node::ScraperDataNode;
use super::node_helpers::{get_node, get_node_mut};
use super::regex_helpers::get_cached_regex;

/// Validates one `filter_items` post-process definition.
///
/// # Arguments
///
/// * `owner` - Human-readable owner name used in error messages.
/// * `pattern` - Regular expression source to compile.
///
/// # Errors
///
/// Returns an error when `pattern` does not compile.
pub(super) fn validate(owner: &str, pattern: &str) -> Result<()> {
    Regex::new(pattern).with_context(|| format!("Invalid post-process regex for {}", owner))?;
    Ok(())
}

/// Retains only the items of `source` whose `field` value matches `pattern`,
/// or removes them when `keep_matching` is `false`.
///
/// # Arguments
///
/// * `root` - Root node that owns the source group.
/// * `source` - `>`-delimited path of the group to filter.
/// * `field` - `>`-delimited path of the scalar field used for the match.
/// * `pattern` - Regular expression applied to each field value.
/// * `keep_matching` - When `true`, retain items whose value matches; when
///   `false`, retain items whose value does not match.
pub(super) fn apply(
    root: &mut ScraperDataNode,
    source: &str,
    field: &str,
    pattern: &str,
    keep_matching: bool,
) {
    let Some(regex) = get_cached_regex(pattern) else {
        return;
    };

    let Some(source_node) = get_node_mut(root, source) else {
        return;
    };

    source_node.items.retain(|item| {
        let is_match = get_node(item, field)
            .map(|node| node.values.iter().any(|value| regex.is_match(value)))
            .unwrap_or(false);

        is_match == keep_matching
    });
}
