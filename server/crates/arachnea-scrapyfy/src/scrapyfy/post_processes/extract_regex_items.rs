use anyhow::{Context, Result};
use regex::Regex;

use super::super::query_helpers::replace_template_placeholders;
use super::super::scraper_data_node::ScraperDataNode;
use super::node_helpers::{get_node, get_node_mut, split_path};
use super::regex_helpers::build_regex_items;
use super::types::{ScraperPostProcessContext, ScraperRegexItemEntry};
use crate::scrapyfy::ScraperOutputType;

/// Validates one `extract_regex_items` post-process definition.
///
/// # Arguments
///
/// * `owner` - Human-readable owner name used in error messages.
/// * `target_field` - Optional field path on each target item.
/// * `pattern` - Regular-expression template source to compile.
/// * `entries` - Field definitions whose `actions` are recursively validated.
///
/// # Errors
///
/// Returns an error for an empty `target_field`, an invalid pattern or entry
/// type, or an invalid entry action.
pub(super) fn validate(
    owner: &str,
    target_field: Option<&str>,
    pattern: &str,
    entries: &[ScraperRegexItemEntry],
) -> Result<()> {
    if target_field.is_some_and(|path| split_path(path).is_empty()) {
        anyhow::bail!("extract_regex_items for {owner} requires a non-empty target_field");
    }
    // Runtime placeholders are substituted only during query execution. Replace
    // named placeholders here so structural validation checks the surrounding
    // regex without treating `{url_fragment}` as an invalid repetition syntax.
    let quantifier_placeholder = Regex::new(r"\{\{[A-Za-z_][A-Za-z0-9_]*\}\}")
        .expect("quantifier template placeholder regex must compile");
    let pattern = quantifier_placeholder.replace_all(pattern, "{0}");
    let placeholder = Regex::new(r"\{[A-Za-z_][A-Za-z0-9_]*\}")
        .expect("named template placeholder regex must compile");
    let validation_pattern = placeholder.replace_all(&pattern, "placeholder");
    Regex::new(&validation_pattern)
        .with_context(|| format!("Invalid post-process regex for {}", owner))?;
    for entry in entries {
        let Some(output_type) = entry.output_type else {
            anyhow::bail!(
                "extract_regex_items for {} entry {} must define type",
                owner,
                entry.name
            );
        };
        if matches!(
            output_type,
            crate::scrapyfy::ScraperOutputType::Object
                | crate::scrapyfy::ScraperOutputType::ObjectArray
        ) {
            anyhow::bail!(
                "extract_regex_items for {} entry {} cannot use {} without nested entries",
                owner,
                entry.name,
                output_type.as_str()
            );
        }
        for action in &entry.actions {
            action.validate(&entry.name, "post-process entry")?;
        }
    }
    Ok(())
}

/// Builds explicit group items from repeated regex matches extracted from one
/// text field. Appends them under `target` or to a field on every target item.
///
/// # Arguments
///
/// * `root` - Root node that receives the produced items.
/// * `context` - Runtime context exposed to entry actions.
/// * `source` - `>`-delimited path of the source node whose scalar values are
///   scanned.
/// * `target` - `>`-delimited path that receives the produced items.
/// * `target_field` - Optional `>`-delimited path inside each existing target
///   item that receives the produced items.
/// * `pattern` - Regular expression applied to each source value.
/// * `entries` - Field definitions for every regex match.
pub(super) fn apply(
    root: &mut ScraperDataNode,
    context: &ScraperPostProcessContext<'_>,
    source: &str,
    target: &str,
    target_field: Option<&str>,
    pattern: &str,
    entries: &[ScraperRegexItemEntry],
) {
    let (pattern, _) = replace_template_placeholders(pattern, context.params);
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
                    &pattern,
                    entries,
                    context.params,
                    context.request_url,
                    context.response_body,
                    context.dynamic_variables,
                )
            })
            .collect::<Vec<_>>()
    };

    if extracted_items.is_empty() {
        return;
    }

    if let Some(target_field) = target_field {
        let Some(target_node) = get_node_mut(root, target) else {
            return;
        };
        let field_path = split_path(target_field);
        for target_item in &mut target_node.items {
            for item in &extracted_items {
                target_item.push_node_typed(
                    &field_path,
                    item.clone(),
                    ScraperOutputType::ObjectArray,
                );
            }
        }
        return;
    }

    let target_path = split_path(target);
    root.set_output_type(&target_path, ScraperOutputType::ObjectArray);
    for item in extracted_items {
        root.push_node_typed(&target_path, item, ScraperOutputType::ObjectArray);
    }
}
