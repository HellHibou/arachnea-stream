use anyhow::{Context, Result};
use regex::Regex;

use super::super::scraper_data_node::ScraperDataNode;
use super::node_helpers::{get_node, get_node_mut, remove_node};
use super::regex_helpers::get_cached_regex;

/// Validates the condition regex and rejects empty removal paths.
///
/// # Arguments
///
/// * `owner` - Owning query or entry label used to contextualize validation errors.
/// * `pattern` - Regular-expression pattern used for matching or extraction.
/// * `remove` - `>`-delimited field paths removed from each matching item.
///
/// # Errors
///
/// Returns a contextual regex error or an error for an empty removal path.
pub(super) fn validate(owner: &str, pattern: &str, remove: &[String]) -> Result<()> {
    Regex::new(pattern).with_context(|| format!("Invalid post-process regex for {}", owner))?;
    for field in remove {
        if field.trim().is_empty() {
            anyhow::bail!(
                "Empty field path in filter_fields post-process for {}",
                owner
            );
        }
    }
    Ok(())
}

/// Removes configured fields from items selected by a regex condition; missing paths or invalid regex are ignored.
///
/// # Arguments
///
/// * `root` - Result tree updated in place by this operation.
/// * `source` - `>`-delimited path of the source node within the result tree.
/// * `field` - Field path relative to the item being inspected.
/// * `pattern` - Regular-expression pattern used for matching or extraction.
/// * `keep_matching` - Whether matching items, rather than nonmatching items, receive the field removal.
/// * `remove` - `>`-delimited field paths removed from each matching item.
pub(super) fn apply(
    root: &mut ScraperDataNode,
    source: &str,
    field: &str,
    pattern: &str,
    keep_matching: bool,
    remove: &[String],
) {
    let Some(regex) = get_cached_regex(pattern) else {
        return;
    };

    let Some(source_node) = get_node_mut(root, source) else {
        return;
    };

    for item in &mut source_node.items {
        let is_match = get_node(item, field)
            .map(|node| node.values.iter().any(|value| regex.is_match(value)))
            .unwrap_or(false);

        if is_match == keep_matching {
            for field_path in remove {
                remove_node(item, field_path);
            }
        }
    }
}
