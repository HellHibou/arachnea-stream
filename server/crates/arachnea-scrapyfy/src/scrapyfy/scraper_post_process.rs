use anyhow::{bail, Context, Result};
use futures::stream::{self, StreamExt};
use mathexpr::Expression;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use super::*;
use crate::scrapyfy::query_helpers;

/// Runtime context exposed to query post-processors.
pub struct ScraperPostProcessContext<'a> {
    /// Runtime template values available to actions executed during post-processing.
    pub params: &'a HashMap<String, String>,
    /// URL used to fetch the parent response.
    pub request_url: &'a str,
    /// Optional parent response body available to actions.
    pub response_body: Option<&'a str>,
    /// Shared HTTP client used by follow-up requests.
    pub http_client: &'a HttpClient,
}

/// One extracted field used to build an item from a regex match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScraperRegexItemEntry {
    /// Output field name (supports nested `>` paths).
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Capture group index to extract from the regex match.
    pub capture_group: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Actions applied on extracted values before insertion.
    pub actions: Vec<ScraperAction>,
}

/// Copies one field from a source node to a target node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScraperFieldMapping {
    /// Source path read from the current source/root item.
    pub source: String,
    /// Target path written into the generated item.
    pub target: String,
}

/// Generates one scalar field from the current 1-based index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScraperGeneratedField {
    /// Output field name generated for each indexed target item.
    pub name: String,
    /// String template where `{}` is replaced by the 1-based index.
    pub format: String,
}

fn default_entries_field() -> String {
    "entries".to_string()
}

fn default_current_page_field() -> String {
    "current_page".to_string()
}

fn default_have_more_field() -> String {
    "have_more".to_string()
}

fn default_next_param() -> String {
    "page".to_string()
}

fn default_source_params_target() -> String {
    "source_params".to_string()
}

/// Scope used to resolve one math expression variable.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperComputedFieldVariableScope {
    /// Resolves the variable against the current item, then its parent item,
    /// then the root node, then query params.
    #[default]
    Auto,
    /// Resolves the variable against the item currently being updated.
    Current,
    /// Resolves the variable against the parent item that owns the nested source.
    Parent,
    /// Resolves the variable against the query root node.
    Root,
    /// Resolves the variable against runtime query params.
    Params,
}

impl ScraperComputedFieldVariableScope {
    fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }
}

/// Variable mapping used by one computed scalar field expression.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScraperComputedFieldVariable {
    /// Path or param name resolved for this variable. Defaults to the variable name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Scope used to resolve the path.
    #[serde(
        default,
        skip_serializing_if = "ScraperComputedFieldVariableScope::is_auto"
    )]
    pub scope: ScraperComputedFieldVariableScope,
}

/// Structured transformations applied after one query extracted its raw fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScraperPostProcess {
    /// Builds explicit group items from repeated regex matches extracted from one text field.
    ExtractRegexItems {
        source: String,
        target: String,
        pattern: String,
        entries: Vec<ScraperRegexItemEntry>,
    },

    /// Filters explicit group items by matching one nested scalar field against a regex.
    FilterItems {
        source: String,
        field: String,
        pattern: String,
        #[serde(default = "ScraperPostProcess::default_keep_matching")]
        keep_matching: bool,
    },

    /// Fetches one text payload per extracted item, applies a regex on each response body,
    /// then appends the extracted items to the requested target path.
    FetchRegexItemsFromItems {
        source: String,
        request_field: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        request_actions: Vec<ScraperAction>,
        target: String,
        pattern: String,
        entries: Vec<ScraperRegexItemEntry>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        copy_item_fields: Vec<ScraperFieldMapping>,
    },

    /// Pivots aligned value lists stored in group items into indexed parent items.
    PivotItemsByIndex {
        source: String,
        target: String,
        values_field: String,
        nested_field: String,
        nested_value_field: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sort_by: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        copy_item_fields: Vec<ScraperFieldMapping>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        copy_root_fields: Vec<ScraperFieldMapping>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        promote_first_nested_fields: Vec<ScraperFieldMapping>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        generated_fields: Vec<ScraperGeneratedField>,
    },

    /// Computes one scalar field from a math expression evaluated per item.
    ComputeItemsField {
        source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nested_source: Option<String>,
        target: String,
        expression: String,
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        variables: HashMap<String, ScraperComputedFieldVariable>,
    },

    /// Derives generic pagination metadata from fields extracted by YAML entries.
    DerivePagination {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        #[serde(default = "default_entries_field")]
        entries_field: String,
        #[serde(default = "default_current_page_field")]
        current_page_field: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total_pages_field: Option<String>,
        #[serde(default = "default_have_more_field")]
        have_more_field: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_value_field: Option<String>,
        #[serde(default = "default_next_param")]
        next_param: String,
        #[serde(default = "default_source_params_target")]
        source_params_target: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page_size_field: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        infer_from_full_page_field: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        remove_fields: Vec<String>,
    },

    /// Appends static items to a target group, optionally skipping existing values.
    AppendStaticItems {
        target: String,
        items: Vec<HashMap<String, String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unique_field: Option<String>,
    },
}

impl ScraperPostProcess {
    const POST_PROCESS_FETCH_CONCURRENCY: usize = 8;

    fn default_keep_matching() -> bool {
        true
    }

    /// Validates one post-process definition before scraper execution.
    pub fn validate(&self, owner: &str) -> Result<()> {
        match self {
            ScraperPostProcess::ExtractRegexItems {
                pattern, entries, ..
            }
            | ScraperPostProcess::FetchRegexItemsFromItems {
                pattern, entries, ..
            } => {
                Regex::new(pattern)
                    .with_context(|| format!("Invalid post-process regex for {}", owner))?;
                for entry in entries {
                    for action in &entry.actions {
                        action.validate(&entry.name, "post-process entry")?;
                    }
                }

                if let ScraperPostProcess::FetchRegexItemsFromItems {
                    request_actions, ..
                } = self
                {
                    for action in request_actions {
                        action.validate(owner, "post-process request")?;
                    }
                }
            }
            ScraperPostProcess::FilterItems { pattern, .. } => {
                Regex::new(pattern)
                    .with_context(|| format!("Invalid post-process regex for {}", owner))?;
            }
            ScraperPostProcess::PivotItemsByIndex { .. } => {}
            ScraperPostProcess::ComputeItemsField {
                source,
                nested_source,
                target,
                expression,
                variables,
            } => {
                if source.trim().is_empty() {
                    bail!(
                        "compute_items_field for {} requires a non-empty source",
                        owner
                    );
                }

                if target.trim().is_empty() {
                    bail!(
                        "compute_items_field for {} requires a non-empty target",
                        owner
                    );
                }

                if let Some(nested_source) = nested_source {
                    if nested_source.trim().is_empty() {
                        bail!(
                            "compute_items_field for {} requires a non-empty nested_source when provided",
                            owner
                        );
                    }
                }

                if expression.trim().is_empty() {
                    bail!(
                        "compute_items_field for {} requires a non-empty expression",
                        owner
                    );
                }

                for (name, variable) in variables {
                    if name.trim().is_empty() {
                        bail!(
                            "compute_items_field for {} contains an empty variable name",
                            owner
                        );
                    }

                    if let Some(path) = &variable.path {
                        if path.trim().is_empty() {
                            bail!(
                                "compute_items_field for {} contains an empty path for variable {}",
                                owner,
                                name
                            );
                        }
                    }

                    if matches!(variable.scope, ScraperComputedFieldVariableScope::Parent)
                        && nested_source.is_none()
                    {
                        bail!(
                            "compute_items_field for {} cannot use parent scope without nested_source",
                            owner
                        );
                    }
                }

                let placeholder_values = Self::list_template_placeholders(expression)
                    .into_iter()
                    .map(|name| (name, "1".to_string()))
                    .collect::<HashMap<_, _>>();
                let (resolved_expression, _missing) =
                    query_helpers::replace_template_placeholders(expression, &placeholder_values);

                Self::evaluate_math_expression(&resolved_expression).with_context(|| {
                    format!("Invalid post-process math expression for {}", owner)
                })?;
            }
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
            } => {
                for (field_name, value) in [
                    ("source", source.as_deref()),
                    ("entries_field", Some(entries_field.as_str())),
                    ("current_page_field", Some(current_page_field.as_str())),
                    ("total_pages_field", total_pages_field.as_deref()),
                    ("have_more_field", Some(have_more_field.as_str())),
                    ("next_value_field", next_value_field.as_deref()),
                    ("next_param", Some(next_param.as_str())),
                    ("source_params_target", Some(source_params_target.as_str())),
                    ("page_size_field", page_size_field.as_deref()),
                    (
                        "infer_from_full_page_field",
                        infer_from_full_page_field.as_deref(),
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
            }
            ScraperPostProcess::AppendStaticItems {
                target,
                items,
                unique_field,
            } => {
                if target.trim().is_empty() {
                    bail!(
                        "append_static_items for {} requires a non-empty target",
                        owner
                    );
                }

                if unique_field
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(str::is_empty)
                {
                    bail!(
                        "append_static_items for {} contains an empty unique_field",
                        owner
                    );
                }

                for item in items {
                    for field_name in item.keys() {
                        if field_name.trim().is_empty() {
                            bail!(
                                "append_static_items for {} contains an empty item field",
                                owner
                            );
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Applies this post-process to one extracted root node.
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
                Self::apply_extract_regex_items(root, context, source, target, pattern, entries);
                Ok(())
            }
            ScraperPostProcess::FilterItems {
                source,
                field,
                pattern,
                keep_matching,
            } => {
                Self::apply_filter_items(root, source, field, pattern, *keep_matching);
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
                Self::apply_fetch_regex_items_from_items(
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
                generated_fields,
            } => {
                Self::apply_pivot_items_by_index(
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
            } => Self::apply_compute_items_field(
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
                Self::apply_derive_pagination(
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
                Self::apply_append_static_items(
                    root,
                    context,
                    target,
                    items,
                    unique_field.as_deref(),
                );
                Ok(())
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_derive_pagination(
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
            let Some(source_node) = Self::get_node_mut(root, source) else {
                return;
            };

            for item in &mut source_node.items {
                Self::derive_pagination_for_node(
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

        Self::derive_pagination_for_node(
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
        let current_page = Self::max_positive_usize(Self::get_node(root, current_page_field));
        let mut have_more = Self::lookup_node_scalar_value(root, have_more_field)
            .map(|value| Self::parse_bool(&value));
        let mut next_param_value = None;

        if let (Some(current_page), Some(total_pages_field)) = (current_page, total_pages_field) {
            if let Some(total_pages) =
                Self::max_positive_usize(Self::get_node(root, total_pages_field))
            {
                let has_next_page = current_page < total_pages;
                have_more = Some(has_next_page);
                if has_next_page {
                    next_param_value = Some((current_page + 1).to_string());
                }
            }
        }

        if next_param_value.is_none() {
            if let Some(next_value) = next_value_field
                .and_then(|field| Self::lookup_node_scalar_value(root, field))
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
                .and_then(|field| Self::lookup_node_scalar_value(root, field))
                .map(|value| Self::parse_bool(&value))
                .unwrap_or(false)
        {
            if let Some(page_size) = page_size_field
                .and_then(|field| Self::max_positive_usize(Self::get_node(root, field)))
            {
                let entries_count = Self::get_node(root, entries_field)
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
            Self::set_node(
                root,
                &Self::split_path(have_more_field),
                ScraperDataNode::from_values(vec![have_more.to_string()]),
            );
        }

        if have_more == Some(true) {
            if let Some(next_param_value) = next_param_value {
                let mut source_param_item = ScraperDataNode::default();
                source_param_item.push_value(&Self::split_path(next_param), next_param_value);
                Self::set_node(
                    root,
                    &Self::split_path(source_params_target),
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

    fn apply_append_static_items(
        root: &mut ScraperDataNode,
        context: &ScraperPostProcessContext<'_>,
        target: &str,
        items: &[HashMap<String, String>],
        unique_field: Option<&str>,
    ) {
        let mut existing_values = unique_field
            .and_then(|field| Self::get_node(root, target).map(|node| (field, node)))
            .map(|(field, node)| {
                node.items
                    .iter()
                    .filter_map(|item| Self::lookup_node_scalar_value(item, field))
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();

        let target_path = Self::split_path(target);
        for static_item in items {
            if let Some(unique_field) = unique_field {
                if let Some(value) = static_item.get(unique_field) {
                    if existing_values.contains(value) {
                        continue;
                    }

                    existing_values.insert(value.clone());
                }
            }

            let mut item = ScraperDataNode::default();
            for (field, value) in static_item {
                let (value, _missing_keys) =
                    query_helpers::replace_template_placeholders(value, context.params);
                let value = value.replace("{request_url}", context.request_url);
                item.push_value(&Self::split_path(field), value);
            }

            root.push_node(&target_path, item);
        }
    }

    fn apply_filter_items(
        root: &mut ScraperDataNode,
        source: &str,
        field: &str,
        pattern: &str,
        keep_matching: bool,
    ) {
        let Some(regex) = Self::get_cached_regex(pattern) else {
            return;
        };

        let Some(source_node) = Self::get_node_mut(root, source) else {
            return;
        };

        source_node.items.retain(|item| {
            let is_match = Self::get_node(item, field)
                .map(|node| node.values.iter().any(|value| regex.is_match(value)))
                .unwrap_or(false);

            is_match == keep_matching
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_compute_items_field(
        root: &mut ScraperDataNode,
        context: &ScraperPostProcessContext<'_>,
        source: &str,
        nested_source: Option<&str>,
        target: &str,
        expression: &str,
        variables: &HashMap<String, ScraperComputedFieldVariable>,
    ) -> Result<()> {
        enum ComputedTargetValues {
            Single(Option<String>),
            Many(Vec<Option<String>>),
        }

        let computed_values = {
            let Some(source_node) = Self::get_node(root, source) else {
                return Ok(());
            };

            if source_node.items.is_empty() {
                return Ok(());
            }

            let mut computed_values = Vec::with_capacity(source_node.items.len());

            for source_item in &source_node.items {
                if let Some(nested_source) = nested_source {
                    let nested_values = Self::get_node(source_item, nested_source)
                        .map(|nested_node| {
                            if nested_node.items.is_empty() {
                                return Self::compute_item_field_value(
                                    expression,
                                    variables,
                                    nested_node,
                                    Some(source_item),
                                    root,
                                    context,
                                )
                                .map(ComputedTargetValues::Single);
                            }

                            nested_node
                                .items
                                .iter()
                                .map(|nested_item| {
                                    Self::compute_item_field_value(
                                        expression,
                                        variables,
                                        nested_item,
                                        Some(source_item),
                                        root,
                                        context,
                                    )
                                })
                                .collect::<Result<Vec<_>>>()
                                .map(ComputedTargetValues::Many)
                        })
                        .transpose()?
                        .unwrap_or(ComputedTargetValues::Single(None));

                    computed_values.push(nested_values);
                    continue;
                }

                computed_values.push(ComputedTargetValues::Single(
                    Self::compute_item_field_value(
                        expression,
                        variables,
                        source_item,
                        None,
                        root,
                        context,
                    )?,
                ));
            }

            computed_values
        };

        let target_path = Self::split_path(target);
        if target_path.is_empty() {
            return Ok(());
        }

        let Some(source_node) = Self::get_node_mut(root, source) else {
            return Ok(());
        };

        for (source_item, source_item_values) in source_node
            .items
            .iter_mut()
            .zip(computed_values.into_iter())
        {
            if let Some(nested_source) = nested_source {
                let Some(nested_node) = Self::get_node_mut(source_item, nested_source) else {
                    continue;
                };

                match source_item_values {
                    ComputedTargetValues::Single(Some(computed_value)) => {
                        Self::set_node(
                            nested_node,
                            &target_path,
                            ScraperDataNode::from_values(vec![computed_value]),
                        );
                    }
                    ComputedTargetValues::Single(None) => {}
                    ComputedTargetValues::Many(computed_values) => {
                        for (nested_item, computed_value) in nested_node
                            .items
                            .iter_mut()
                            .zip(computed_values.into_iter())
                        {
                            let Some(computed_value) = computed_value else {
                                continue;
                            };

                            Self::set_node(
                                nested_item,
                                &target_path,
                                ScraperDataNode::from_values(vec![computed_value]),
                            );
                        }
                    }
                }

                continue;
            }

            match source_item_values {
                ComputedTargetValues::Single(Some(computed_value)) => {
                    Self::set_node(
                        source_item,
                        &target_path,
                        ScraperDataNode::from_values(vec![computed_value]),
                    );
                }
                ComputedTargetValues::Single(None) | ComputedTargetValues::Many(_) => {}
            }
        }

        Ok(())
    }

    fn compute_item_field_value(
        expression: &str,
        variables: &HashMap<String, ScraperComputedFieldVariable>,
        current_item: &ScraperDataNode,
        parent_item: Option<&ScraperDataNode>,
        root: &ScraperDataNode,
        context: &ScraperPostProcessContext<'_>,
    ) -> Result<Option<String>> {
        let placeholders = Self::list_template_placeholders(expression);
        let mut resolved_variables = HashMap::with_capacity(placeholders.len());

        for placeholder in placeholders {
            let Some(value) = Self::resolve_numeric_variable(
                &placeholder,
                variables.get(&placeholder),
                current_item,
                parent_item,
                root,
                context,
            ) else {
                return Ok(None);
            };

            resolved_variables.insert(placeholder, value);
        }

        let (resolved_expression, missing_keys) =
            query_helpers::replace_template_placeholders(expression, &resolved_variables);

        if !missing_keys.is_empty() {
            return Ok(None);
        }

        let computed_value =
            Self::evaluate_math_expression(&resolved_expression).with_context(|| {
                format!(
                    "Failed to evaluate computed field expression `{}` resolved as `{}`",
                    expression, resolved_expression
                )
            })?;

        Ok(Some(Self::format_math_result(computed_value)))
    }

    fn resolve_numeric_variable(
        variable_name: &str,
        variable: Option<&ScraperComputedFieldVariable>,
        current_item: &ScraperDataNode,
        parent_item: Option<&ScraperDataNode>,
        root: &ScraperDataNode,
        context: &ScraperPostProcessContext<'_>,
    ) -> Option<String> {
        let path = variable
            .and_then(|variable| variable.path.as_deref())
            .unwrap_or(variable_name)
            .trim();

        if path.is_empty() {
            return None;
        }

        let raw_value = match variable.map(|variable| variable.scope).unwrap_or_default() {
            ScraperComputedFieldVariableScope::Auto => {
                Self::lookup_node_scalar_value(current_item, path)
                    .or_else(|| {
                        parent_item.and_then(|parent_item| {
                            Self::lookup_node_scalar_value(parent_item, path)
                        })
                    })
                    .or_else(|| Self::lookup_node_scalar_value(root, path))
                    .or_else(|| {
                        context
                            .params
                            .get(path)
                            .map(|value| value.trim().to_string())
                            .filter(|value| !value.is_empty())
                    })
            }
            ScraperComputedFieldVariableScope::Current => {
                Self::lookup_node_scalar_value(current_item, path)
            }
            ScraperComputedFieldVariableScope::Parent => parent_item
                .and_then(|parent_item| Self::lookup_node_scalar_value(parent_item, path)),
            ScraperComputedFieldVariableScope::Root => Self::lookup_node_scalar_value(root, path),
            ScraperComputedFieldVariableScope::Params => context
                .params
                .get(path)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        }?;

        let parsed_value = raw_value.parse::<f64>().ok()?;
        parsed_value
            .is_finite()
            .then(|| Self::format_math_result(parsed_value))
    }

    fn lookup_node_scalar_value(root: &ScraperDataNode, path: &str) -> Option<String> {
        Self::get_node(root, path).and_then(|node| {
            node.values.iter().find_map(|value| {
                let trimmed = value.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            })
        })
    }

    fn max_positive_usize(node: Option<&ScraperDataNode>) -> Option<usize> {
        node.and_then(|node| {
            node.values
                .iter()
                .filter_map(|value| value.parse::<usize>().ok())
                .filter(|value| *value > 0)
                .max()
        })
    }

    fn parse_bool(value: &str) -> bool {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes" | "y"
        )
    }

    fn list_template_placeholders(template: &str) -> Vec<String> {
        let params = HashMap::<String, String>::new();
        let (_resolved, missing_keys) =
            query_helpers::replace_template_placeholders(template, &params);
        missing_keys
    }

    fn evaluate_math_expression(expression: &str) -> Result<f64> {
        let compiled_expression = Expression::parse(expression)
            .with_context(|| format!("Failed to parse math expression `{}`", expression))?
            .compile(&[] as &[&str])
            .with_context(|| format!("Failed to compile math expression `{}`", expression))?;
        let evaluated_value = compiled_expression
            .eval(&[] as &[f64])
            .with_context(|| format!("Failed to evaluate math expression `{}`", expression))?;

        if !evaluated_value.is_finite() {
            bail!(
                "Math expression `{}` returned a non-finite value",
                expression
            );
        }

        Ok(evaluated_value)
    }

    fn format_math_result(value: f64) -> String {
        let normalized_value = if value.abs() < 1e-12 { 0.0 } else { value };
        let rounded_value = normalized_value.round();

        if (normalized_value - rounded_value).abs() < 1e-9 {
            return format!("{rounded_value:.0}");
        }

        let mut rendered = format!("{normalized_value:.12}");
        while rendered.contains('.') && rendered.ends_with('0') {
            rendered.pop();
        }

        if rendered.ends_with('.') {
            rendered.pop();
        }

        rendered
    }

    fn apply_extract_regex_items(
        root: &mut ScraperDataNode,
        context: &ScraperPostProcessContext<'_>,
        source: &str,
        target: &str,
        pattern: &str,
        entries: &[ScraperRegexItemEntry],
    ) {
        let extracted_items = {
            let Some(source_node) = Self::get_node(root, source) else {
                return;
            };

            source_node
                .values
                .iter()
                .flat_map(|source_value| {
                    Self::build_regex_items(
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
        };

        let target_path = Self::split_path(target);
        for item in extracted_items {
            root.push_node(&target_path, item);
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn apply_fetch_regex_items_from_items(
        root: &mut ScraperDataNode,
        context: &ScraperPostProcessContext<'_>,
        source: &str,
        request_field: &str,
        request_actions: &[ScraperAction],
        target: &str,
        pattern: &str,
        entries: &[ScraperRegexItemEntry],
        copy_item_fields: &[ScraperFieldMapping],
    ) -> Result<()> {
        struct FetchRegexItemsJob {
            request_url: String,
            prepared_copy_fields: Vec<(String, ScraperDataNode)>,
        }

        let mut extracted_items = Vec::new();
        let mut seen_request_urls = HashSet::new();
        let mut fetch_jobs = Vec::new();
        let target_path = Self::split_path(target);

        {
            let Some(source_node) = Self::get_node(root, source) else {
                return Ok(());
            };

            if source_node.items.is_empty() {
                return Ok(());
            }

            for source_item in &source_node.items {
                let Some(request_values) =
                    Self::get_node(source_item, request_field).map(|node| node.values.clone())
                else {
                    continue;
                };

                let mut resolved_request_urls = request_values;
                for action in request_actions {
                    resolved_request_urls = action.apply(
                        &None,
                        resolved_request_urls,
                        context.params,
                        context.request_url,
                        context.response_body,
                        None,
                    );
                }

                for request_url in resolved_request_urls
                    .into_iter()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                {
                    if !seen_request_urls.insert(request_url.clone()) {
                        continue;
                    }

                    fetch_jobs.push(FetchRegexItemsJob {
                        request_url,
                        prepared_copy_fields: Self::prepare_copy_fields(
                            source_item,
                            copy_item_fields,
                        ),
                    });
                }
            }
        }

        let fetch_results = stream::iter(fetch_jobs.into_iter().map(|job| async move {
            let response_body = context
                .http_client
                .query_http(http::Method::GET, &job.request_url)
                .await
                .with_context(|| {
                    format!("Failed to fetch post-process request {}", job.request_url)
                })?;

            Ok::<(FetchRegexItemsJob, String), anyhow::Error>((job, response_body))
        }))
        .buffer_unordered(Self::POST_PROCESS_FETCH_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

        for fetch_result in fetch_results {
            let (job, response_body) = fetch_result?;

            for mut item in Self::build_regex_items(
                &response_body,
                pattern,
                entries,
                context.params,
                &job.request_url,
                Some(&response_body),
            ) {
                for (target, node) in &job.prepared_copy_fields {
                    Self::set_node(&mut item, &Self::split_path(target), node.clone());
                }

                extracted_items.push(item);
            }
        }

        for item in extracted_items {
            root.push_node(&target_path, item);
        }

        Ok(())
    }

    fn build_regex_items(
        source_text: &str,
        pattern: &str,
        entries: &[ScraperRegexItemEntry],
        params: &HashMap<String, String>,
        request_url: &str,
        response_body: Option<&str>,
    ) -> Vec<ScraperDataNode> {
        let Some(regex) = Self::get_cached_regex(pattern) else {
            return Vec::new();
        };

        let entry_paths: Vec<Vec<&str>> = entries
            .iter()
            .map(|entry| Self::split_path(&entry.name))
            .collect();

        let mut items = Vec::new();

        for captures in regex.captures_iter(source_text) {
            let mut item = ScraperDataNode::default();

            for (entry, path) in entries.iter().zip(entry_paths.iter()) {
                let mut values = entry
                    .capture_group
                    .and_then(|group| {
                        captures
                            .get(group)
                            .map(|value| vec![value.as_str().to_string()])
                    })
                    .unwrap_or_default();

                for action in &entry.actions {
                    values = action.apply(&None, values, params, request_url, response_body, None);
                }

                for value in values {
                    item.push_value(path, value);
                }
            }

            items.push(item);
        }

        items
    }

    fn get_cached_regex(pattern: &str) -> Option<Regex> {
        static REGEX_CACHE: OnceLock<Mutex<HashMap<String, Regex>>> = OnceLock::new();

        let cache = REGEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

        if let Ok(cache_guard) = cache.lock() {
            if let Some(regex) = cache_guard.get(pattern) {
                return Some(regex.clone());
            }
        }

        let compiled = Regex::new(pattern).ok()?;

        if let Ok(mut cache_guard) = cache.lock() {
            cache_guard.insert(pattern.to_string(), compiled.clone());
        }

        Some(compiled)
    }

    fn prepare_copy_fields(
        source_root: &ScraperDataNode,
        copy_item_fields: &[ScraperFieldMapping],
    ) -> Vec<(String, ScraperDataNode)> {
        let mut prepared_fields = Vec::new();

        for field in copy_item_fields {
            let Some(node) = Self::get_node(source_root, &field.source).cloned() else {
                continue;
            };

            prepared_fields.push((field.target.clone(), node));
        }

        prepared_fields
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_pivot_items_by_index(
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
        generated_fields: &[ScraperGeneratedField],
    ) {
        let target_items = {
            let Some(source_node) = Self::get_node(root, source) else {
                return;
            };

            if source_node.items.is_empty() {
                return;
            }

            let mut source_items: Vec<&ScraperDataNode> = source_node.items.iter().collect();
            if let Some(sort_by) = sort_by {
                source_items.sort_by(|left, right| {
                    let left_value = Self::get_node(left, sort_by)
                        .and_then(|node| node.values.first())
                        .map(String::as_str)
                        .unwrap_or_default();
                    let right_value = Self::get_node(right, sort_by)
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
                    Self::get_node(item, values_field)
                        .map(|node| node.values.len())
                        .unwrap_or(0)
                })
                .max()
                .unwrap_or(0);

            if max_len == 0 {
                return;
            }

            let mut target_items = Vec::new();
            let nested_field_path = Self::split_path(nested_field);
            let nested_value_field_path = Self::split_path(nested_value_field);
            let generated_field_paths: Vec<Vec<&str>> = generated_fields
                .iter()
                .map(|field| Self::split_path(&field.name))
                .collect();

            for index in 0..max_len {
                let mut target_item = ScraperDataNode::default();
                let mut nested_items = Vec::new();

                for (generated_field, generated_field_path) in
                    generated_fields.iter().zip(generated_field_paths.iter())
                {
                    target_item.push_value(
                        generated_field_path,
                        generated_field
                            .format
                            .replace("{}", &(index + 1).to_string()),
                    );
                }

                for source_item in &source_items {
                    let Some(value_node) = Self::get_node(source_item, values_field) else {
                        continue;
                    };
                    let Some(value) = value_node.values.get(index) else {
                        continue;
                    };

                    let mut nested_item = ScraperDataNode::default();
                    nested_item.push_value(&nested_value_field_path, value.clone());

                    for field in copy_item_fields {
                        Self::copy_field(
                            source_item,
                            &field.source,
                            &mut nested_item,
                            &field.target,
                        );
                    }

                    for field in copy_root_fields {
                        Self::copy_field(root, &field.source, &mut nested_item, &field.target);
                    }

                    nested_items.push(nested_item);
                }

                if nested_items.is_empty() {
                    continue;
                }

                for field in promote_first_nested_fields {
                    if let Some(first_nested_item) = nested_items.first() {
                        Self::copy_field(
                            first_nested_item,
                            &field.source,
                            &mut target_item,
                            &field.target,
                        );
                    }
                }

                Self::set_node(
                    &mut target_item,
                    &nested_field_path,
                    ScraperDataNode {
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

        let target_path = Self::split_path(target);
        Self::set_node(
            root,
            &target_path,
            ScraperDataNode {
                values: Vec::new(),
                children: HashMap::new(),
                items: target_items,
            },
        );
    }

    fn copy_field(
        source_root: &ScraperDataNode,
        source: &str,
        target_root: &mut ScraperDataNode,
        target: &str,
    ) {
        let Some(node) = Self::get_node(source_root, source).cloned() else {
            return;
        };

        Self::set_node(target_root, &Self::split_path(target), node);
    }

    fn split_path(path: &str) -> Vec<&str> {
        path.split('>')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .collect()
    }

    fn get_node<'a>(root: &'a ScraperDataNode, path: &str) -> Option<&'a ScraperDataNode> {
        let mut current = root;
        for segment in Self::split_path(path) {
            current = current.children.get(segment)?;
        }
        Some(current)
    }

    fn get_node_mut<'a>(
        root: &'a mut ScraperDataNode,
        path: &str,
    ) -> Option<&'a mut ScraperDataNode> {
        let mut current = root;
        for segment in Self::split_path(path) {
            current = current.children.get_mut(segment)?;
        }
        Some(current)
    }

    fn set_node(root: &mut ScraperDataNode, path: &[&str], node: ScraperDataNode) {
        if path.is_empty() {
            return;
        }

        let mut current = root;
        for segment in &path[..path.len().saturating_sub(1)] {
            current = current.children.entry((*segment).to_string()).or_default();
        }

        current
            .children
            .insert(path[path.len().saturating_sub(1)].to_string(), node);
    }
}
