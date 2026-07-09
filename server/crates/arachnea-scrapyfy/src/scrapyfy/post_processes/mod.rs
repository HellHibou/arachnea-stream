use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::actions::ScraperAction;
use super::scraper_data_node::ScraperDataNode;

mod append_static_items;
mod compute_items_field;
mod derive_pagination;
mod extract_regex_items;
mod fetch_regex_items_from_items;
mod filter_items;
mod math_helpers;
mod node_helpers;
mod pivot_items_by_index;
mod regex_helpers;
mod types;

// Re-export the shared building blocks so the rest of the crate keeps
// importing them from `crate::scrapyfy::post_processes` without having to
// know about the inner module split.
pub use types::{
    ScraperComputedFieldVariable, ScraperComputedFieldVariableScope, ScraperFieldMapping,
    ScraperGeneratedField, ScraperPostProcessContext, ScraperRegexItemEntry,
};

/// Structured transformations applied after one query extracted its raw fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScraperPostProcess {
    /// Builds explicit group items from repeated regex matches extracted from one text field.
    ExtractRegexItems {
        /// Source field containing the text to extract items from.
        source: String,
        /// Target field where extracted items will be stored.
        target: String,
        /// Regex pattern used to extract items.
        pattern: String,
        /// List of field mappings for extracted regex groups.
        entries: Vec<ScraperRegexItemEntry>,
    },

    /// Filters explicit group items by matching one nested scalar field against a regex.
    FilterItems {
        /// Source field containing the items to filter.
        source: String,
        /// Field name within each item to match against the pattern.
        field: String,
        /// Regex pattern used to filter items.
        pattern: String,
        /// Whether to keep items that match (true) or don't match (false) the pattern.
        #[serde(default = "ScraperPostProcess::default_keep_matching")]
        keep_matching: bool,
    },

    /// Fetches one text payload per extracted item, applies a regex on each response body,
    /// then appends the extracted items to the requested target path.
    FetchRegexItemsFromItems {
        /// Source field containing the items to process.
        source: String,
        /// Field name within each item containing the URL to fetch.
        request_field: String,
        /// Optional actions to apply to the request field before fetching.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        request_actions: Vec<ScraperAction>,
        /// Target field where extracted items will be stored.
        target: String,
        /// Regex pattern used to extract items from fetched content.
        pattern: String,
        /// List of field mappings for extracted regex groups.
        entries: Vec<ScraperRegexItemEntry>,
        /// Fields to copy from source items to extracted items.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        copy_item_fields: Vec<ScraperFieldMapping>,
    },

    /// Pivots aligned value lists stored in group items into indexed parent items.
    PivotItemsByIndex {
        /// Source field containing the items to pivot.
        source: String,
        /// Target field where pivoted items will be stored.
        target: String,
        /// Field name containing the values to pivot.
        values_field: String,
        /// Field name containing the nested items.
        nested_field: String,
        /// Field name within nested items containing the values.
        nested_value_field: String,
        /// Optional field name to sort results by.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sort_by: Option<String>,
        /// Fields to copy from source items to pivoted items.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        copy_item_fields: Vec<ScraperFieldMapping>,
        /// Fields to copy from root to pivoted items.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        copy_root_fields: Vec<ScraperFieldMapping>,
        /// Fields to promote from first nested item to pivoted items.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        promote_first_nested_fields: Vec<ScraperFieldMapping>,
        /// Fields to copy from target to pivoted items.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        copy_target_fields: Vec<ScraperFieldMapping>,
        /// Generated fields to add to pivoted items.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        generated_fields: Vec<ScraperGeneratedField>,
    },

    /// Computes one scalar field from a math expression evaluated per item.
    ComputeItemsField {
        /// Source field containing the items to process.
        source: String,
        /// Optional nested source field for nested computations.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nested_source: Option<String>,
        /// Target field where computed values will be stored.
        target: String,
        /// Math expression to evaluate for each item.
        expression: String,
        /// Variables available in the expression.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        variables: HashMap<String, ScraperComputedFieldVariable>,
    },

    /// Derives generic pagination metadata from fields extracted by YAML entries.
    DerivePagination {
        /// Optional source field containing pagination data.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        /// Field name containing the entries/list of items.
        #[serde(default = "types::default_entries_field")]
        entries_field: String,
        /// Field name containing the current page number.
        #[serde(default = "types::default_current_page_field")]
        current_page_field: String,
        /// Optional field name containing the total number of pages.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total_pages_field: Option<String>,
        /// Field name indicating if there are more pages available.
        #[serde(default = "types::default_have_more_field")]
        have_more_field: String,
        /// Optional field name containing the next page value.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_value_field: Option<String>,
        /// Query parameter name for the next page.
        #[serde(default = "types::default_next_param")]
        next_param: String,
        /// Target field where source parameters will be stored.
        #[serde(default = "types::default_source_params_target")]
        source_params_target: String,
        /// Optional field name containing the page size.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page_size_field: Option<String>,
        /// Optional field name to infer full page status from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        infer_from_full_page_field: Option<String>,
        /// Fields to remove from the pagination metadata.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        remove_fields: Vec<String>,
    },

    /// Appends static items to a target group, optionally skipping existing values.
    AppendStaticItems {
        /// Target field where static items will be appended.
        target: String,
        /// List of static items to append.
        items: Vec<HashMap<String, String>>,
        /// Optional field name to use for deduplication.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unique_field: Option<String>,
    },
}

impl ScraperPostProcess {
    /// Returns the serde default for `FilterItems::keep_matching`.
    fn default_keep_matching() -> bool {
        true
    }

    /// Validates one post-process definition before scraper execution.
    ///
    /// # Arguments
    ///
    /// * `owner` - Human-readable owner name used in error messages.
    ///
    /// # Errors
    ///
    /// Returns an error when the post-process configuration is invalid (bad
    /// regex, empty field name, invalid math expression, etc.).
    pub fn validate(&self, owner: &str) -> Result<()> {
        match self {
            ScraperPostProcess::ExtractRegexItems {
                pattern, entries, ..
            } => extract_regex_items::validate(owner, pattern, entries),
            ScraperPostProcess::FetchRegexItemsFromItems {
                pattern,
                entries,
                request_actions,
                ..
            } => fetch_regex_items_from_items::validate(owner, pattern, entries, request_actions),
            ScraperPostProcess::FilterItems { pattern, .. } => {
                filter_items::validate(owner, pattern)
            }
            ScraperPostProcess::PivotItemsByIndex { .. } => pivot_items_by_index::validate(),
            ScraperPostProcess::ComputeItemsField {
                source,
                nested_source,
                target,
                expression,
                variables,
            } => compute_items_field::validate(
                owner,
                source,
                nested_source.as_deref(),
                target,
                expression,
                variables,
            ),
            ScraperPostProcess::DerivePagination {
                source,
                entries_field,
                current_page_field,
                total_pages_field,
                have_more_field,
                next_value_field,
                next_param,
                source_params_target,
                page_size_field,
                infer_from_full_page_field,
                ..
            } => derive_pagination::validate(
                owner,
                source.as_deref(),
                entries_field,
                current_page_field,
                total_pages_field.as_deref(),
                have_more_field,
                next_value_field.as_deref(),
                next_param,
                source_params_target,
                page_size_field.as_deref(),
                infer_from_full_page_field.as_deref(),
            ),
            ScraperPostProcess::AppendStaticItems {
                target,
                items,
                unique_field,
            } => append_static_items::validate(owner, target, items, unique_field.as_deref()),
        }
    }

    /// Applies this post-process to one extracted root node.
    ///
    /// # Arguments
    ///
    /// * `root` - Root node that owns the produced/extracted data.
    /// * `context` - Runtime context exposed to the post-process actions.
    ///
    /// # Errors
    ///
    /// Returns an error when one of the post-process steps (typically an HTTP
    /// fetch or a math expression evaluation) fails.
    pub async fn apply(
        &self,
        root: &mut ScraperDataNode,
        context: &ScraperPostProcessContext<'_>,
    ) -> Result<()> {
        match self {
            ScraperPostProcess::ExtractRegexItems {
                source,
                target,
                pattern,
                entries,
            } => {
                extract_regex_items::apply(root, context, source, target, pattern, entries);
                Ok(())
            }
            ScraperPostProcess::FilterItems {
                source,
                field,
                pattern,
                keep_matching,
            } => {
                filter_items::apply(root, source, field, pattern, *keep_matching);
                Ok(())
            }
            ScraperPostProcess::FetchRegexItemsFromItems {
                source,
                request_field,
                request_actions,
                target,
                pattern,
                entries,
                copy_item_fields,
            } => {
                fetch_regex_items_from_items::apply(
                    root,
                    context,
                    source,
                    request_field,
                    request_actions,
                    target,
                    pattern,
                    entries,
                    copy_item_fields,
                )
                .await
            }
            ScraperPostProcess::PivotItemsByIndex {
                source,
                target,
                values_field,
                nested_field,
                nested_value_field,
                sort_by,
                copy_item_fields,
                copy_root_fields,
                promote_first_nested_fields,
                copy_target_fields,
                generated_fields,
            } => {
                pivot_items_by_index::apply(
                    root,
                    source,
                    target,
                    values_field,
                    nested_field,
                    nested_value_field,
                    sort_by.as_deref(),
                    copy_item_fields,
                    copy_root_fields,
                    promote_first_nested_fields,
                    copy_target_fields,
                    generated_fields,
                );
                Ok(())
            }
            ScraperPostProcess::ComputeItemsField {
                source,
                nested_source,
                target,
                expression,
                variables,
            } => compute_items_field::apply(
                root,
                context,
                source,
                nested_source.as_deref(),
                target,
                expression,
                variables,
            ),
            ScraperPostProcess::DerivePagination {
                source,
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
            } => {
                derive_pagination::apply(
                    root,
                    source.as_deref(),
                    entries_field,
                    current_page_field,
                    total_pages_field.as_deref(),
                    have_more_field,
                    next_value_field.as_deref(),
                    next_param,
                    source_params_target,
                    page_size_field.as_deref(),
                    infer_from_full_page_field.as_deref(),
                    remove_fields,
                );
                Ok(())
            }
            ScraperPostProcess::AppendStaticItems {
                target,
                items,
                unique_field,
            } => {
                append_static_items::apply(root, context, target, items, unique_field.as_deref());
                Ok(())
            }
        }
    }
}
