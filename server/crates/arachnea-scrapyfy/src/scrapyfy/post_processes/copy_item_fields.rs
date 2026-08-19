use anyhow::{bail, Result};

use super::super::scraper_data_node::ScraperDataNode;
use super::node_helpers::{get_node, get_node_mut, set_node, split_path};
use super::types::ScraperFieldMapping;

/// Validates one `copy_item_fields` post-process definition.
pub(super) fn validate(owner: &str, source: &str, fields: &[ScraperFieldMapping]) -> Result<()> {
    if source.trim().is_empty() {
        bail!("copy_item_fields for {} requires a non-empty source", owner);
    }
    if fields.is_empty() {
        bail!("copy_item_fields for {} requires at least one field", owner);
    }
    for field in fields {
        if field.source.trim().is_empty() || field.target.trim().is_empty() {
            bail!(
                "copy_item_fields for {} requires non-empty source and target paths",
                owner
            );
        }
    }
    Ok(())
}

/// Copies fields between items of a root group while preserving their node types.
pub(super) fn apply(root: &mut ScraperDataNode, source: &str, fields: &[ScraperFieldMapping]) {
    let Some(source_node) = get_node_mut(root, source) else {
        return;
    };

    for item in &mut source_node.items {
        for field in fields {
            let Some(value) = get_node(item, &field.source).cloned() else {
                continue;
            };
            set_node(item, &split_path(&field.target), value);
        }
    }
}
