use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::scrapyfy::post_processes::{
    evaluate_math_expression, format_math_result, list_template_placeholders,
};
use crate::scrapyfy::query_helpers;
use crate::scrapyfy::{ScraperDataNode, ScraperOutputType};

/// Ordered transformations applied after an object entry has built all children.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScraperPostBuild {
    /// Calculates a numeric child from scalar sibling values referenced as `{path}`.
    MathFormula {
        /// `>`-separated path receiving the computed number.
        target: String,
        /// Math expression using sibling value placeholders such as `{total} / {per_page}`.
        expression: String,
    },
    /// Removes transient child fields after preceding post-build transformations consumed them.
    RemoveFields {
        /// `>`-separated child paths to remove.
        fields: Vec<String>,
    },
}

impl ScraperPostBuild {
    /// Validates one post-build configuration before scraper execution.
    pub fn validate(&self, owner: &str) -> Result<()> {
        match self {
            Self::MathFormula { target, expression } => {
                if path_segments(target).is_empty() {
                    bail!(
                        "post_build math_formula for {} requires a non-empty target",
                        owner
                    );
                }
                if expression.trim().is_empty() {
                    bail!(
                        "post_build math_formula for {} requires a non-empty expression",
                        owner
                    );
                }

                let values = list_template_placeholders(expression)
                    .into_iter()
                    .map(|name| (name, "1".to_string()))
                    .collect();
                let (resolved, _) =
                    query_helpers::replace_template_placeholders(expression, &values);
                evaluate_math_expression(&resolved).with_context(|| {
                    format!("Invalid post_build math_formula expression for {}", owner)
                })?;
            }
            Self::RemoveFields { fields } => {
                if fields.is_empty() || fields.iter().any(|field| path_segments(field).is_empty()) {
                    bail!(
                        "post_build remove_fields for {} requires non-empty fields",
                        owner
                    );
                }
            }
        }

        Ok(())
    }

    /// Applies one post-build transformation to an assembled object entry.
    pub fn apply(&self, node: &mut ScraperDataNode) {
        match self {
            Self::MathFormula { target, expression } => {
                let values = list_template_placeholders(expression)
                    .into_iter()
                    .map(|placeholder| {
                        lookup_scalar(node, &placeholder)
                            .map(|value| (placeholder, value.to_string()))
                    })
                    .collect::<Option<std::collections::HashMap<_, _>>>();
                let Some(values) = values else {
                    return;
                };
                let (resolved, missing) =
                    query_helpers::replace_template_placeholders(expression, &values);
                if !missing.is_empty() {
                    return;
                }
                let Ok(value) = evaluate_math_expression(&resolved) else {
                    return;
                };

                node.set_value_typed(
                    &path_segments(target),
                    format_math_result(value),
                    ScraperOutputType::Number,
                );
            }
            Self::RemoveFields { fields } => {
                for field in fields {
                    remove_path(node, &path_segments(field));
                }
            }
        }
    }
}

fn path_segments(path: &str) -> Vec<&str> {
    path.split('>')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn lookup_scalar<'a>(node: &'a ScraperDataNode, path: &str) -> Option<&'a str> {
    path_segments(path)
        .into_iter()
        .try_fold(node, |current, segment| current.children.get(segment))
        .and_then(ScraperDataNode::value_as_string)
}

fn remove_path(node: &mut ScraperDataNode, path: &[&str]) {
    let Some((last, parent_path)) = path.split_last() else {
        return;
    };
    let mut current = node;
    for segment in parent_path {
        let Some(child) = current.children.get_mut(*segment) else {
            return;
        };
        current = child;
    }
    current.children.remove(*last);
}
