use anyhow::{Context, Result};
use regex::Regex;

use super::super::scraper_data_node::ScraperDataNode;
use super::node_helpers::{get_node, split_path};
use super::regex_helpers::build_regex_items;
use super::types::{ScraperPostProcessContext, ScraperRegexItemEntry};

/// Validates one `extract_regex_items` post-process definition.
///
/// # Arguments
///
/// * `owner` - Human-readable owner name used in error messages.
/// * `pattern` - Regular expression source to compile.
/// * `entries` - Field definitions whose `actions` are recursively validated.
///
/// # Errors
///
/// Returns an error when `pattern` does not compile, or when one of the
/// entry actions fails to validate.
pub(super) fn validate(
    owner: &str,
    pattern: &str,
    entries: &[ScraperRegexItemEntry],
) -> Result<()> {
    Regex::new(pattern).with_context(|| format!("Invalid post-process regex for {}", owner))?;
    for entry in entries {
        for action in &entry.actions {
            action.validate(&entry.name, "post-process entry")?;
        }
    }
    Ok(())
}

/// Builds explicit group items from repeated regex matches extracted from one
/// text field and appends them under `target`.
///
/// # Arguments
///
/// * `root` - Root node that receives the produced items.
/// * `context` - Runtime context exposed to entry actions.
/// * `source` - `>`-delimited path of the source node whose scalar values are
///   scanned.
/// * `target` - `>`-delimited path that receives the produced items.
/// * `pattern` - Regular expression applied to each source value.
/// * `entries` - Field definitions for every regex match.
pub(super) fn apply(
    root: &mut ScraperDataNode,
    context: &ScraperPostProcessContext<'_>,
    source: &str,
    target: &str,
    pattern: &str,
    entries: &[ScraperRegexItemEntry],
) {
    let extracted_items = {
        let Some(source_node) = get_node(root, source) else {
            return;
        };

        source_node
            .values
            .iter()
            .flat_map(|source_value| {
                build_regex_items(
                    source_value,
                    pattern,
                    entries,
                    context.params,
                    context.request_url,
                    context.response_body,
                )
            })
            .collect::<Vec<_>>()
    };

    if extracted_items.is_empty() {
        return;
    }

    let target_path = split_path(target);
    for item in extracted_items {
        root.push_node(&target_path, item);
    }
}
