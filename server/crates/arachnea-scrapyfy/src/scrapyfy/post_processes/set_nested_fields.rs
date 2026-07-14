use anyhow::{bail, Result};

use super::super::scraper_data_node::{ScraperDataNode, ScraperOutputType};
use super::node_helpers::{get_node, get_node_mut, set_node, split_path};
use super::types::ScraperNestedFieldDefinition;

/// Validates one `set_nested_fields` post-process definition.
///
/// # Arguments
///
/// * `owner` - Human-readable owner name used in error messages.
/// * `source` - `>`-delimited path of the source group.
/// * `nested_source` - `>`-delimited path of the nested group inside each
///   source item.
/// * `fields` - Field definitions (each must have a target and exactly one of
///   value, copy_from, or host_from).
///
/// # Errors
///
/// Returns an error when `source`, `nested_source`, or any field target is
/// empty, when `fields` is empty, or when a field does not have exactly one
/// source kind set.
pub(super) fn validate(
    owner: &str,
    source: &str,
    nested_source: &str,
    fields: &[ScraperNestedFieldDefinition],
) -> Result<()> {
    if source.trim().is_empty() {
        bail!(
            "set_nested_fields for {} requires a non-empty source",
            owner
        );
    }
    if nested_source.trim().is_empty() {
        bail!(
            "set_nested_fields for {} requires a non-empty nested_source",
            owner
        );
    }
    if fields.is_empty() {
        bail!(
            "set_nested_fields for {} requires at least one field",
            owner
        );
    }
    for field in fields {
        if field.target.trim().is_empty() {
            bail!(
                "set_nested_fields for {} contains a field with empty target",
                owner
            );
        }
        let source_count = field.value.is_some() as u8
            + field.copy_from.is_some() as u8
            + field.host_from.is_some() as u8;
        if source_count == 0 {
            bail!(
                "set_nested_fields for {} field '{}' requires one of value, copy_from, or host_from",
                owner,
                field.target
            );
        }
        if source_count > 1 {
            bail!(
                "set_nested_fields for {} field '{}' has more than one source (value, copy_from, host_from)",
                owner,
                field.target
            );
        }
    }
    Ok(())
}

/// Extracts the hostname from a URL string, removing the `www.` prefix.
fn extract_host(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    Some(host.strip_prefix("www.").unwrap_or(host).to_string())
}

/// Sets one or more fields on every nested item inside every source item.
///
/// For each item in `source`, the `nested_source` sub-array is resolved.  For
/// every nested item, every field definition is applied: a static `value` is
/// written to the field path, a sibling field is copied via `copy_from`, or
/// the hostname from a URL in a sibling field is extracted via `host_from`.
///
/// # Arguments
///
/// * `root` - Root node that owns the source group.
/// * `source` - `>`-delimited path of the source group.
/// * `nested_source` - `>`-delimited path of the nested group inside each
///   source item.
/// * `fields` - Field definitions to apply on each nested item.
pub(super) fn apply(
    root: &mut ScraperDataNode,
    source: &str,
    nested_source: &str,
    fields: &[ScraperNestedFieldDefinition],
) {
    let Some(source_node) = get_node_mut(root, source) else {
        return;
    };

    if source_node.items.is_empty() {
        return;
    }

    for source_item in &mut source_node.items {
        let Some(nested_node) = get_node_mut(source_item, nested_source) else {
            continue;
        };

        if nested_node.items.is_empty() {
            continue;
        }

        for nested_item in &mut nested_node.items {
            for field in fields {
                if let Some(value) = &field.value {
                    set_node(
                        nested_item,
                        &split_path(&field.target),
                        ScraperDataNode::from_values_typed(
                            vec![value.clone()],
                            ScraperOutputType::String,
                        ),
                    );
                } else if let Some(copy_from) = &field.copy_from {
                    let cloned = get_node(nested_item, copy_from).cloned();
                    if let Some(cloned) = cloned {
                        set_node(nested_item, &split_path(&field.target), cloned);
                    }
                } else if let Some(host_from) = &field.host_from {
                    let host = get_node(nested_item, host_from)
                        .and_then(|node| node.values.first())
                        .and_then(|url| extract_host(url));
                    if let Some(host) = host {
                        set_node(
                            nested_item,
                            &split_path(&field.target),
                            ScraperDataNode::from_values_typed(
                                vec![host],
                                ScraperOutputType::String,
                            ),
                        );
                    }
                }
            }
        }
    }
}
