use std::collections::HashMap;

use super::super::scraper_data_node::{ScraperDataNode, ScraperOutputType};
use super::node_helpers::{copy_field, get_node, set_node, split_path};
use super::types::{ScraperFieldMapping, ScraperGeneratedField};

/// Validates one `pivot_items_by_index` post-process definition.
///
/// `pivot_items_by_index` has no upfront semantic check, so this always
/// returns `Ok(())` and exists to mirror the public dispatch surface used by
/// the other post-process actions.
pub(super) fn validate() -> Result<(), anyhow::Error> {
    Ok(())
}

/// Pivots aligned value lists stored in group items into indexed parent items.
///
/// For every index up to the longest aligned value list, a fresh target item
/// is produced with optional `generated_fields` plus a nested group containing
/// one entry per source item. `copy_item_fields` and `copy_root_fields` are
/// copied into each nested entry, `promote_first_nested_fields` is copied
/// from the first nested entry into the target item, and `copy_target_fields`
/// is copied directly from `root` into the target item. `sort_by` (when
/// provided) is used to order the source items before pivotting.
///
/// # Arguments
///
/// * `root` - Root node that owns the source group and receives the target
///   items.
/// * `source` - `>`-delimited path of the source group.
/// * `target` - `>`-delimited path that receives the produced target group.
/// * `values_field` - `>`-delimited path of the aligned value list inside each
///   source item.
/// * `nested_field` - `>`-delimited path of the nested group written into
///   each target item.
/// * `nested_value_field` - `>`-delimited path of the scalar value written
///   into each nested entry.
/// * `sort_by` - Optional `>`-delimited path used to sort the source items.
/// * `copy_item_fields` - Field mappings copied from each source item into the
///   corresponding nested entry.
/// * `copy_root_fields` - Field mappings copied from `root` into each nested
///   entry.
/// * `promote_first_nested_fields` - Field mappings copied from the first
///   nested entry into the target item.
/// * `copy_target_fields` - Field mappings copied from `root` directly into
///   each target item (without passing through nested entries).
/// * `generated_fields` - Scalar fields generated for each indexed target.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    root: &mut ScraperDataNode,
    source: &str,
    target: &str,
    values_field: &str,
    nested_field: &str,
    nested_value_field: &str,
    sort_by: Option<&str>,
    copy_item_fields: &[ScraperFieldMapping],
    copy_root_fields: &[ScraperFieldMapping],
    promote_first_nested_fields: &[ScraperFieldMapping],
    copy_target_fields: &[ScraperFieldMapping],
    generated_fields: &[ScraperGeneratedField],
) {
    let target_items = {
        let Some(source_node) = get_node(root, source) else {
            return;
        };

        if source_node.items.is_empty() {
            return;
        }

        let mut source_items: Vec<&ScraperDataNode> = source_node.items.iter().collect();
        if let Some(sort_by) = sort_by {
            source_items.sort_by(|left, right| {
                let left_value = get_node(left, sort_by)
                    .and_then(|node| node.values.first())
                    .map(String::as_str)
                    .unwrap_or_default();
                let right_value = get_node(right, sort_by)
                    .and_then(|node| node.values.first())
                    .map(String::as_str)
                    .unwrap_or_default();

                match (left_value.parse::<usize>(), right_value.parse::<usize>()) {
                    (Ok(left_value), Ok(right_value)) => left_value.cmp(&right_value),
                    _ => left_value.cmp(right_value),
                }
            });
        }

        let max_len = source_items
            .iter()
            .map(|item| {
                get_node(item, values_field)
                    .map(|node| node.values.len())
                    .unwrap_or(0)
            })
            .max()
            .unwrap_or(0);

        if max_len == 0 {
            return;
        }

        let mut target_items = Vec::new();
        let nested_field_path = split_path(nested_field);
        let nested_value_field_path = split_path(nested_value_field);
        let generated_field_paths: Vec<Vec<&str>> = generated_fields
            .iter()
            .map(|field| split_path(&field.name))
            .collect();

        for index in 0..max_len {
            let mut target_item = ScraperDataNode::default();
            let mut nested_items = Vec::new();

            for (generated_field, generated_field_path) in
                generated_fields.iter().zip(generated_field_paths.iter())
            {
                let generated_output_type =
                    generated_field.output_type.unwrap_or(ScraperOutputType::String);
                target_item.push_value_typed(
                    generated_field_path,
                    generated_field
                        .format
                        .replace("{}", &(index + 1).to_string()),
                    generated_output_type,
                );
            }

            for source_item in &source_items {
                let Some(value_node) = get_node(source_item, values_field) else {
                    continue;
                };
                let Some(value) = value_node.values.get(index) else {
                    continue;
                };
                let nested_value_output_type = value_node
                    .output_type
                    .and_then(ScraperOutputType::array_element_type)
                    .or(value_node.output_type)
                    .unwrap_or(ScraperOutputType::String);

                let mut nested_item = ScraperDataNode::default();
                nested_item.push_value_typed(
                    &nested_value_field_path,
                    value.clone(),
                    nested_value_output_type,
                );

                for field in copy_item_fields {
                    copy_field(source_item, &field.source, &mut nested_item, &field.target);
                }

                for field in copy_root_fields {
                    copy_field(root, &field.source, &mut nested_item, &field.target);
                }

                nested_items.push(nested_item);
            }

            if nested_items.is_empty() {
                continue;
            }

            for field in promote_first_nested_fields {
                if let Some(first_nested_item) = nested_items.first() {
                    copy_field(
                        first_nested_item,
                        &field.source,
                        &mut target_item,
                        &field.target,
                    );
                }
            }

            for field in copy_target_fields {
                copy_field(root, &field.source, &mut target_item, &field.target);
            }

            set_node(
                &mut target_item,
                &nested_field_path,
                ScraperDataNode {
                    output_type: Some(ScraperOutputType::ObjectArray),
                    values: Vec::new(),
                    children: HashMap::new(),
                    items: nested_items,
                },
            );

            target_items.push(target_item);
        }

        if target_items.is_empty() {
            return;
        }

        target_items
    };

    let target_path = split_path(target);
    set_node(
        root,
        &target_path,
        ScraperDataNode {
            output_type: Some(ScraperOutputType::ObjectArray),
            values: Vec::new(),
            children: HashMap::new(),
            items: target_items,
        },
    );
}
