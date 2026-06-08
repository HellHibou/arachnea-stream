use anyhow::{bail, Result};
use std::collections::HashMap;

use super::super::scraper_data_node::ScraperDataNode;
use super::node_helpers::{get_node, get_node_mut, lookup_node_scalar_value, max_positive_usize, parse_bool, set_node, split_path};

/// Validates one `derive_pagination` post-process definition.
///
/// # Arguments
///
/// * `owner` - Human-readable owner name used in error messages.
/// * `source` - Optional `>`-delimited path of the source group.
/// * `entries_field` - Field name used to expose the entries group.
/// * `current_page_field` - Field name used to expose the current page value.
/// * `total_pages_field` - Optional field name used to read the total page
///   count.
/// * `have_more_field` - Field name used to expose the "have more" boolean.
/// * `next_value_field` - Optional field name whose scalar value is reused as
///   the next page value.
/// * `next_param` - Parameter name used inside `source_params`.
/// * `source_params_target` - Field name used to expose `source_params`.
/// * `page_size_field` - Optional field name used to infer pagination from the
///   page size.
/// * `infer_from_full_page_field` - Optional field name whose boolean decides
///   whether to infer pagination from a full page.
///
/// # Errors
///
/// Returns an error when any of the field names resolve to an empty string.
#[allow(clippy::too_many_arguments)]
pub(super) fn validate(
    owner: &str,
    source: Option<&str>,
    entries_field: &str,
    current_page_field: &str,
    total_pages_field: Option<&str>,
    have_more_field: &str,
    next_value_field: Option<&str>,
    next_param: &str,
    source_params_target: &str,
    page_size_field: Option<&str>,
    infer_from_full_page_field: Option<&str>,
) -> Result<()> {
    for (field_name, value) in [
        ("source", source),
        ("entries_field", Some(entries_field)),
        ("current_page_field", Some(current_page_field)),
        ("total_pages_field", total_pages_field),
        ("have_more_field", Some(have_more_field)),
        ("next_value_field", next_value_field),
        ("next_param", Some(next_param)),
        ("source_params_target", Some(source_params_target)),
        ("page_size_field", page_size_field),
        (
            "infer_from_full_page_field",
            infer_from_full_page_field,
        ),
    ] {
        if value.map(str::trim).is_some_and(str::is_empty) {
            bail!(
                "derive_pagination for {} contains an empty {}",
                owner,
                field_name
            );
        }
    }

    Ok(())
}

/// Derives generic pagination metadata (`have_more`, `source_params`) from
/// fields already extracted on the root node (or each item of `source` when
/// provided).
///
/// # Arguments
///
/// * `root` - Root node that receives the pagination fields.
/// * `source` - Optional `>`-delimited path of a per-item source group.
/// * `entries_field` - Field name used to count produced entries when
///   inferring pagination from a full page.
/// * `current_page_field` - Field name used to read the current page number.
/// * `total_pages_field` - Optional field name used to read the total page
///   count.
/// * `have_more_field` - Field name written with the resulting boolean.
/// * `next_value_field` - Optional field name whose scalar value is reused as
///   the next page value.
/// * `next_param` - Parameter name used inside `source_params`.
/// * `source_params_target` - Field name written with the `source_params`
///   payload.
/// * `page_size_field` - Optional field name used to infer pagination from the
///   page size.
/// * `infer_from_full_page_field` - Optional field name whose boolean decides
///   whether to infer pagination from a full page.
/// * `remove_fields` - Field names removed from the node after derivation.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    root: &mut ScraperDataNode,
    source: Option<&str>,
    entries_field: &str,
    current_page_field: &str,
    total_pages_field: Option<&str>,
    have_more_field: &str,
    next_value_field: Option<&str>,
    next_param: &str,
    source_params_target: &str,
    page_size_field: Option<&str>,
    infer_from_full_page_field: Option<&str>,
    remove_fields: &[String],
) {
    if let Some(source) = source {
        let Some(source_node) = get_node_mut(root, source) else {
            return;
        };

        for item in &mut source_node.items {
            derive_pagination_for_node(
                item,
                entries_field,
                current_page_field,
                total_pages_field,
                have_more_field,
                next_value_field,
                next_param,
                source_params_target,
                page_size_field,
                infer_from_full_page_field,
                remove_fields,
            );
        }
        return;
    }

    derive_pagination_for_node(
        root,
        entries_field,
        current_page_field,
        total_pages_field,
        have_more_field,
        next_value_field,
        next_param,
        source_params_target,
        page_size_field,
        infer_from_full_page_field,
        remove_fields,
    );
}

/// Derives pagination metadata for a single node using the same heuristic as
/// [`apply`] but operating on a borrowed node reference.
///
/// # Arguments
///
/// * `root` - Node that receives the pagination fields and from which inputs
///   are read.
/// * `entries_field` - Field name used to count produced entries when
///   inferring pagination from a full page.
/// * `current_page_field` - Field name used to read the current page number.
/// * `total_pages_field` - Optional field name used to read the total page
///   count.
/// * `have_more_field` - Field name written with the resulting boolean.
/// * `next_value_field` - Optional field name whose scalar value is reused as
///   the next page value.
/// * `next_param` - Parameter name used inside `source_params`.
/// * `source_params_target` - Field name written with the `source_params`
///   payload.
/// * `page_size_field` - Optional field name used to infer pagination from the
///   page size.
/// * `infer_from_full_page_field` - Optional field name whose boolean decides
///   whether to infer pagination from a full page.
/// * `remove_fields` - Field names removed from the node after derivation.
#[allow(clippy::too_many_arguments)]
fn derive_pagination_for_node(
    root: &mut ScraperDataNode,
    entries_field: &str,
    current_page_field: &str,
    total_pages_field: Option<&str>,
    have_more_field: &str,
    next_value_field: Option<&str>,
    next_param: &str,
    source_params_target: &str,
    page_size_field: Option<&str>,
    infer_from_full_page_field: Option<&str>,
    remove_fields: &[String],
) {
    let current_page = max_positive_usize(get_node(root, current_page_field));
    let mut have_more = lookup_node_scalar_value(root, have_more_field).map(|value| parse_bool(&value));
    let mut next_param_value = None;

    if let (Some(current_page), Some(total_pages_field)) = (current_page, total_pages_field) {
        if let Some(total_pages) = max_positive_usize(get_node(root, total_pages_field)) {
            let has_next_page = current_page < total_pages;
            have_more = Some(has_next_page);
            if has_next_page {
                next_param_value = Some((current_page + 1).to_string());
            }
        }
    }

    if next_param_value.is_none() {
        if let Some(next_value) = next_value_field
            .and_then(|field| lookup_node_scalar_value(root, field))
            .filter(|value| !value.trim().is_empty())
        {
            have_more = Some(true);
            next_param_value = Some(next_value);
        }
    }

    if next_param_value.is_none() && have_more == Some(true) {
        if let Some(current_page) = current_page {
            next_param_value = Some((current_page + 1).to_string());
        }
    }

    if next_param_value.is_none()
        && infer_from_full_page_field
            .and_then(|field| lookup_node_scalar_value(root, field))
            .map(|value| parse_bool(&value))
            .unwrap_or(false)
    {
        if let Some(page_size) = page_size_field
            .and_then(|field| max_positive_usize(get_node(root, field)))
        {
            let entries_count = get_node(root, entries_field)
                .map(|entries| entries.items.len())
                .unwrap_or(0);
            let has_next_page = entries_count >= page_size;
            have_more = Some(has_next_page);

            if has_next_page {
                if let Some(current_page) = current_page {
                    next_param_value = Some((current_page + 1).to_string());
                }
            }
        }
    }

    if let Some(have_more) = have_more {
        set_node(
            root,
            &split_path(have_more_field),
            ScraperDataNode::from_values(vec![have_more.to_string()]),
        );
    }

    if have_more == Some(true) {
        if let Some(next_param_value) = next_param_value {
            let mut source_param_item = ScraperDataNode::default();
            source_param_item.push_value(&split_path(next_param), next_param_value);
            set_node(
                root,
                &split_path(source_params_target),
                ScraperDataNode {
                    values: Vec::new(),
                    children: HashMap::new(),
                    items: vec![source_param_item],
                },
            );
        }
    } else {
        root.children.remove(source_params_target);
    }

    for field in remove_fields {
        root.children.remove(field);
    }
}
