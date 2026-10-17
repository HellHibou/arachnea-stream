use std::collections::HashMap;

use anyhow::{bail, Result};

use super::super::scraper_data_node::{ScraperDataNode, ScraperOutputType};
use super::node_helpers::{get_node, get_node_mut, lookup_node_scalar_value, set_node, split_path};
use super::types::{ScraperNestedFieldDefinition, ScraperNestedFieldLookup};

/// Validates one `set_nested_fields` post-process definition.
///
/// # Arguments
///
/// * `owner` - Human-readable owner name used in error messages.
/// * `source` - `>`-delimited path of the source group.
/// * `nested_source` - `>`-delimited path of the nested group inside each
///   source item.
/// * `fields` - Field definitions (each must have a target and exactly one of
///   value, copy_from, or host_from; an optional lookup may refine the copied
///   value).
///
/// # Errors
///
/// Returns an error when `source`, `nested_source`, or any field target is
/// empty, when `fields` is empty, when a field does not have exactly one
/// source kind set, or when a lookup definition is incomplete.
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
        if let Some(lookup) = &field.lookup {
            if field.value.is_some() {
                bail!(
                    "set_nested_fields for {} field '{}' cannot combine lookup with value",
                    owner,
                    field.target
                );
            }
            if lookup.source.trim().is_empty()
                || lookup.key.trim().is_empty()
                || lookup.value.trim().is_empty()
            {
                bail!(
                    "set_nested_fields for {} field '{}' lookup requires non-empty source, key, and value",
                    owner,
                    field.target
                );
            }
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

/// Builds a key/value map from the items of one root group.
///
/// # Arguments
///
/// * `root` - Root node that owns the lookup group.
/// * `lookup` - Lookup definition naming the source group and its key/value
///   fields.
///
/// # Returns
///
/// A map of the scalar key/value pairs found in the lookup group. Missing
/// groups or items without both scalar fields produce an empty/partial map.
fn build_lookup_map(
    root: &ScraperDataNode,
    lookup: &ScraperNestedFieldLookup,
) -> HashMap<String, String> {
    let Some(source_node) = get_node(root, &lookup.source) else {
        return HashMap::new();
    };

    source_node
        .items
        .iter()
        .filter_map(|item| {
            let key = lookup_node_scalar_value(item, &lookup.key)?;
            let value = lookup_node_scalar_value(item, &lookup.value)?;
            Some((key, value))
        })
        .collect()
}

/// Looks up `host` in `map`, trying the full hostname first and then each
/// parent domain (for example `video.sibnet.ru` then `sibnet.ru`).
///
/// # Arguments
///
/// * `map` - Hostname-to-value directory.
/// * `host` - Hostname extracted from a URL.
///
/// # Returns
///
/// The mapped value for the first matching hostname or parent domain.
fn lookup_host<'a>(map: &'a HashMap<String, String>, host: &str) -> Option<&'a str> {
    let labels: Vec<&str> = host.split('.').collect();
    for start in 0..labels.len().saturating_sub(1) {
        let candidate = labels[start..].join(".");
        if let Some(value) = map.get(&candidate) {
            return Some(value);
        }
    }
    None
}

/// Sets one or more fields on every nested item inside every source item.
///
/// For each item in `source`, the `nested_source` sub-array is resolved.  For
/// every nested item, every field definition is applied: a static `value` is
/// written to the field path, a sibling field is copied via `copy_from`, or
/// the hostname from a URL in a sibling field is extracted via `host_from`.
/// When a field declares a `lookup`, the resulting value is translated through
/// the referenced root group before it is written.
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
    let lookup_maps: Vec<Option<HashMap<String, String>>> = fields
        .iter()
        .map(|field| {
            field
                .lookup
                .as_ref()
                .map(|lookup| build_lookup_map(root, lookup))
        })
        .collect();

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
            for (field, lookup_map) in fields.iter().zip(lookup_maps.iter()) {
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
                        let node = match lookup_map {
                            Some(lookup_map) => {
                                let key = cloned.values.first().cloned().unwrap_or_default();
                                let value = lookup_map.get(&key).cloned().unwrap_or(key);
                                ScraperDataNode::from_values_typed(
                                    vec![value],
                                    ScraperOutputType::String,
                                )
                            }
                            None => cloned,
                        };
                        set_node(nested_item, &split_path(&field.target), node);
                    }
                } else if let Some(host_from) = &field.host_from {
                    let host = get_node(nested_item, host_from)
                        .and_then(|node| node.values.first())
                        .and_then(|url| extract_host(url));
                    if let Some(host) = host {
                        let name = lookup_map
                            .as_ref()
                            .and_then(|map| lookup_host(map, &host))
                            .map(str::to_string)
                            .unwrap_or_else(|| host.clone());
                        set_node(
                            nested_item,
                            &split_path(&field.target),
                            ScraperDataNode::from_values_typed(
                                vec![name],
                                ScraperOutputType::String,
                            ),
                        );
                    }
                }
            }
        }
    }
}
