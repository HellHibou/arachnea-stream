use anyhow::{Context, Result};
use regex::Regex;

use super::super::scraper_data_node::ScraperDataNode;
use super::node_helpers::{get_node, get_node_mut, remove_node};
use super::regex_helpers::get_cached_regex;

pub(super) fn validate(owner: &str, pattern: &str, remove: &[String]) -> Result<()> {
    Regex::new(pattern).with_context(|| format!("Invalid post-process regex for {}", owner))?;
    for field in remove {
        if field.trim().is_empty() {
            anyhow::bail!("Empty field path in filter_fields post-process for {}", owner);
        }
    }
    Ok(())
}

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