use anyhow::{bail, Context, Result};
use std::collections::HashMap;

use super::super::query_helpers;
use super::super::scraper_data_node::ScraperDataNode;
use super::math_helpers::{
    evaluate_math_expression, format_math_result, list_template_placeholders,
};
use super::node_helpers::{get_node, get_node_mut, lookup_node_scalar_value, set_node, split_path};
use super::types::{
    ScraperComputedFieldVariable, ScraperComputedFieldVariableScope, ScraperPostProcessContext,
};

/// Validates one `compute_items_field` post-process definition.
///
/// # Arguments
///
/// * `owner` - Human-readable owner name used in error messages.
/// * `source` - `>`-delimited path of the source group.
/// * `nested_source` - Optional `>`-delimited path of a nested group inside
///   each source item.
/// * `target` - `>`-delimited path that receives the produced scalar value.
/// * `expression` - Math expression source to validate.
/// * `variables` - Variable mappings used by the expression.
///
/// # Errors
///
/// Returns an error when `source`, `target`, `nested_source` (if provided),
/// `expression`, or any variable path is empty, when the `parent` scope is
/// requested without a `nested_source`, or when the math expression fails to
/// evaluate against placeholder values.
pub(super) fn validate(
    owner: &str,
    source: &str,
    nested_source: Option<&str>,
    target: &str,
    expression: &str,
    variables: &HashMap<String, ScraperComputedFieldVariable>,
) -> Result<()> {
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

    let placeholder_values = list_template_placeholders(expression)
        .into_iter()
        .map(|name| (name, "1".to_string()))
        .collect::<HashMap<_, _>>();
    let (resolved_expression, _missing) =
        query_helpers::replace_template_placeholders(expression, &placeholder_values);

    evaluate_math_expression(&resolved_expression)
        .with_context(|| format!("Invalid post-process math expression for {}", owner))?;

    Ok(())
}

/// Computes one scalar field from a math expression evaluated per item (and
/// optionally per nested item) and writes the result into the target path.
///
/// # Arguments
///
/// * `root` - Root node that owns the source group and receives the computed
///   values.
/// * `context` - Runtime context used to resolve `params` scope.
/// * `source` - `>`-delimited path of the source group.
/// * `nested_source` - Optional `>`-delimited path of a nested group inside
///   each source item.
/// * `target` - `>`-delimited path that receives the computed value.
/// * `expression` - Math expression source.
/// * `variables` - Variable mappings used by the expression.
///
/// # Errors
///
/// Returns an error when one of the math expression evaluations fails.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
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
        let Some(source_node) = get_node(root, source) else {
            return Ok(());
        };

        if source_node.items.is_empty() {
            return Ok(());
        }

        let mut computed_values = Vec::with_capacity(source_node.items.len());

        for source_item in &source_node.items {
            if let Some(nested_source) = nested_source {
                let nested_values = get_node(source_item, nested_source)
                    .map(|nested_node| {
                        if nested_node.items.is_empty() {
                            return compute_item_field_value(
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
                                compute_item_field_value(
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

            computed_values.push(ComputedTargetValues::Single(compute_item_field_value(
                expression,
                variables,
                source_item,
                None,
                root,
                context,
            )?));
        }

        computed_values
    };

    let target_path = split_path(target);
    if target_path.is_empty() {
        return Ok(());
    }

    let Some(source_node) = get_node_mut(root, source) else {
        return Ok(());
    };

    for (source_item, source_item_values) in source_node
        .items
        .iter_mut()
        .zip(computed_values.into_iter())
    {
        if let Some(nested_source) = nested_source {
            let Some(nested_node) = get_node_mut(source_item, nested_source) else {
                continue;
            };

            match source_item_values {
                ComputedTargetValues::Single(Some(computed_value)) => {
                    set_node(
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

                        set_node(
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
                set_node(
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

/// Computes the scalar value produced by `expression` for one (current, parent)
/// item triple, resolving every placeholder against the configured scope.
///
/// # Arguments
///
/// * `expression` - Math expression source.
/// * `variables` - Variable mappings used by the expression.
/// * `current_item` - Node the `current` scope reads from.
/// * `parent_item` - Optional parent node the `parent` scope reads from.
/// * `root` - Root node the `root` scope reads from.
/// * `context` - Runtime context the `params` scope reads from.
///
/// # Errors
///
/// Returns an error when the resolved math expression fails to evaluate.
fn compute_item_field_value(
    expression: &str,
    variables: &HashMap<String, ScraperComputedFieldVariable>,
    current_item: &ScraperDataNode,
    parent_item: Option<&ScraperDataNode>,
    root: &ScraperDataNode,
    context: &ScraperPostProcessContext<'_>,
) -> Result<Option<String>> {
    let placeholders = list_template_placeholders(expression);
    let mut resolved_variables = HashMap::with_capacity(placeholders.len());

    for placeholder in placeholders {
        let Some(value) = resolve_numeric_variable(
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

    let computed_value = evaluate_math_expression(&resolved_expression).with_context(|| {
        format!(
            "Failed to evaluate computed field expression `{}` resolved as `{}`",
            expression, resolved_expression
        )
    })?;

    Ok(Some(format_math_result(computed_value)))
}

/// Resolves one math expression variable into a finite numeric string, falling
/// back through the configured scope chain.
///
/// # Arguments
///
/// * `variable_name` - Placeholder name to resolve.
/// * `variable` - Optional variable configuration overriding the path or
///   scope.
/// * `current_item` - Node the `current` scope reads from.
/// * `parent_item` - Optional parent node the `parent` scope reads from.
/// * `root` - Root node the `root` scope reads from.
/// * `context` - Runtime context the `params` scope reads from.
///
/// # Returns
///
/// The formatted numeric value, or `None` when the variable cannot be
/// resolved to a finite number.
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
        ScraperComputedFieldVariableScope::Auto => lookup_node_scalar_value(current_item, path)
            .or_else(|| {
                parent_item.and_then(|parent_item| lookup_node_scalar_value(parent_item, path))
            })
            .or_else(|| lookup_node_scalar_value(root, path))
            .or_else(|| {
                context
                    .params
                    .get(path)
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            }),
        ScraperComputedFieldVariableScope::Current => lookup_node_scalar_value(current_item, path),
        ScraperComputedFieldVariableScope::Parent => {
            parent_item.and_then(|parent_item| lookup_node_scalar_value(parent_item, path))
        }
        ScraperComputedFieldVariableScope::Root => lookup_node_scalar_value(root, path),
        ScraperComputedFieldVariableScope::Params => context
            .params
            .get(path)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    }?;

    let parsed_value = raw_value.parse::<f64>().ok()?;
    parsed_value
        .is_finite()
        .then(|| format_math_result(parsed_value))
}
