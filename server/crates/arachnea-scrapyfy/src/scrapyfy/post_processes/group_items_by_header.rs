use super::super::scraper_data_node::{ScraperDataNode, ScraperOutputType};
use super::node_helpers::{
    get_node, lookup_node_scalar_value, parse_bool, remove_node, set_node, split_path,
};

/// Validates one `group_items_by_header` post-process definition.
///
/// `group_items_by_header` has no upfront semantic check, so this always
/// returns `Ok(())` and exists to mirror the public dispatch surface used by
/// the other post-process actions.
pub(super) fn validate() -> Result<(), anyhow::Error> {
    Ok(())
}

/// Groups flat items into labeled sections delimited by header rows.
///
/// Items are consumed in display order. An item flagged by `is_header_field`
/// closes the current section and opens a new one labeled by `label_field`.
/// Every other item is appended to the open section's entries. Items flagged
/// by any of `skip_fields` are dropped entirely. Items found before the first
/// header row either start a section labeled `first_label` (when provided) or
/// are dropped. Sections without any entry are not emitted.
///
/// # Arguments
///
/// * `root` - Root node that owns the source group and receives the target
///   group.
/// * `source` - `>`-delimited path of the flat source items.
/// * `target` - `>`-delimited path that receives the grouped sections.
/// * `is_header_field` - `>`-delimited path of the boolean header flag.
/// * `label_field` - `>`-delimited path of the section label on header rows.
/// * `entries_field` - `>`-delimited path of the entries array written inside
///   each section.
/// * `skip_fields` - Boolean field paths; items where any is true are dropped.
/// * `first_label` - Optional label for items preceding the first header row.
/// * `remove_item_fields` - Field paths stripped from every copied entry.
/// * `remove_source` - When true, the source field is removed after grouping.
pub(super) fn apply(
    root: &mut ScraperDataNode,
    source: &str,
    target: &str,
    is_header_field: &str,
    label_field: &str,
    entries_field: &str,
    skip_fields: &[String],
    first_label: Option<&str>,
    remove_item_fields: &[String],
    remove_source: bool,
) {
    let mut sections: Vec<ScraperDataNode> = Vec::new();
    let mut current_label: Option<String> = None;
    let mut current_entries: Vec<ScraperDataNode> = Vec::new();

    {
        let Some(source_node) = get_node(root, source) else {
            return;
        };

        for item in &source_node.items {
            if skip_fields.iter().any(|field| {
                lookup_node_scalar_value(item, field)
                    .map(|value| parse_bool(&value))
                    .unwrap_or(false)
            }) {
                continue;
            }

            let is_header = lookup_node_scalar_value(item, is_header_field)
                .map(|value| parse_bool(&value))
                .unwrap_or(false);

            if is_header {
                flush_section(
                    &mut sections,
                    &mut current_label,
                    &mut current_entries,
                    entries_field,
                );
                current_label = lookup_node_scalar_value(item, label_field);
                continue;
            }

            if current_label.is_none() {
                match first_label {
                    Some(label) => current_label = Some(label.to_string()),
                    // Drop items found before the first header row.
                    None => continue,
                }
            }

            let mut entry = item.clone();
            for field in remove_item_fields {
                remove_node(&mut entry, field);
            }
            current_entries.push(entry);
        }
    }

    flush_section(
        &mut sections,
        &mut current_label,
        &mut current_entries,
        entries_field,
    );

    if !sections.is_empty() {
        set_node(
            root,
            &split_path(target),
            ScraperDataNode {
                output_type: Some(ScraperOutputType::ObjectArray),
                values: Vec::new(),
                children: Default::default(),
                items: sections,
            },
        );
    }

    if remove_source {
        remove_node(root, source);
    }
}

/// Closes the currently open section: when it carries a label and at least one
/// entry, a section item is pushed into `sections`. Resets the open section.
fn flush_section(
    sections: &mut Vec<ScraperDataNode>,
    current_label: &mut Option<String>,
    current_entries: &mut Vec<ScraperDataNode>,
    entries_field: &str,
) {
    if let Some(label) = current_label.take() {
        if !current_entries.is_empty() {
            let mut section = ScraperDataNode::default();
            section.push_value_typed(&["label"], label, ScraperOutputType::String);
            set_node(
                &mut section,
                &split_path(entries_field),
                ScraperDataNode {
                    output_type: Some(ScraperOutputType::ObjectArray),
                    values: Vec::new(),
                    children: Default::default(),
                    items: std::mem::take(current_entries),
                },
            );
            sections.push(section);
        }
    }

    current_entries.clear();
}
