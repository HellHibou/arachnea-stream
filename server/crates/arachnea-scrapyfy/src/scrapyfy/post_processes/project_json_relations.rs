use super::node_helpers::{set_node, split_path};
use super::types::ScraperPostProcessContext;
use super::{ScraperDataNode, ScraperOutputType};
use anyhow::{bail, Context, Result};
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;

/// Validates the projection paths, pattern, and output template.
#[allow(clippy::too_many_arguments)]
pub(super) fn validate(
    owner: &str,
    target: &str,
    pointer: &str,
    key: &str,
    relations: &str,
    pattern: &str,
    template: &Value,
    values_field: &str,
    pairs_field: &str,
    from_field: &str,
    to_field: &str,
) -> Result<()> {
    if [
        target,
        key,
        relations,
        values_field,
        pairs_field,
        from_field,
        to_field,
    ]
    .iter()
    .any(|s| s.trim().is_empty())
        || (!pointer.is_empty() && !pointer.starts_with('/'))
        || !template.is_object()
        || values_field == pairs_field
        || from_field == to_field
    {
        bail!("invalid project_json_relations configuration for {owner}");
    }
    Regex::new(pattern).context("invalid project_json_relations pattern")?;
    Ok(())
}

/// Projects unique identifiers and directed edges, failing closed on malformed input.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply(
    root: &mut ScraperDataNode,
    context: &ScraperPostProcessContext<'_>,
    target: &str,
    pointer: &str,
    key: &str,
    relations: &str,
    pattern: &str,
    template: &Value,
    values_field: &str,
    pairs_field: &str,
    from_field: &str,
    to_field: &str,
) -> Result<()> {
    let body = context
        .response_body
        .context("project_json_relations requires a response body")?;
    let input: Value = serde_json::from_str(body).context("invalid project_json_relations JSON")?;
    let rows = input
        .pointer(pointer)
        .and_then(Value::as_array)
        .context("project_json_relations requires an array")?;
    let regex = Regex::new(pattern)?;
    let mut identifiers = BTreeSet::new();
    let mut edges = BTreeSet::new();
    for row in rows {
        let source = row
            .get(key)
            .and_then(Value::as_str)
            .context("missing relation identifier")?;
        let targets = row
            .get(relations)
            .and_then(Value::as_array)
            .context("missing relation array")?;
        if regex.is_match(source) {
            identifiers.insert(source.to_owned());
        }
        for item in targets {
            let destination = item
                .as_str()
                .context("relation identifier must be a string")?;
            if regex.is_match(source) && regex.is_match(destination) && source != destination {
                edges.insert((source.to_owned(), destination.to_owned()));
            }
        }
    }
    edges.retain(|(source, destination)| {
        identifiers.contains(source) && identifiers.contains(destination)
    });
    if edges.is_empty() {
        bail!("project_json_relations found no usable relations");
    }
    let mut result = template.clone();
    let object = result
        .as_object_mut()
        .context("projection template must be an object")?;
    object.insert(values_field.to_owned(), serde_json::to_value(identifiers)?);
    object.insert(
        pairs_field.to_owned(),
        Value::Array(
            edges
                .into_iter()
                .map(|(from, to)| {
                    let mut pair = serde_json::Map::new();
                    pair.insert(from_field.to_owned(), Value::String(from));
                    pair.insert(to_field.to_owned(), Value::String(to));
                    Value::Object(pair)
                })
                .collect(),
        ),
    );
    set_node(root, &split_path(target), json_node(&result)?);
    Ok(())
}

/// Converts a JSON template to an explicitly typed scraper tree.
fn json_node(value: &Value) -> Result<ScraperDataNode> {
    let mut node = ScraperDataNode::default();
    match value {
        Value::Object(fields) => {
            node.output_type = Some(ScraperOutputType::Object);
            for (key, value) in fields {
                node.children.insert(key.clone(), json_node(value)?);
            }
        }
        Value::Array(items) if items.iter().all(Value::is_string) => {
            node.output_type = Some(ScraperOutputType::StringArray);
            node.values = items
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect();
        }
        Value::Array(items) if items.iter().all(Value::is_object) => {
            node.output_type = Some(ScraperOutputType::ObjectArray);
            node.items = items.iter().map(json_node).collect::<Result<_>>()?;
        }
        Value::String(value) => {
            node.output_type = Some(ScraperOutputType::String);
            node.values.push(value.clone());
        }
        Value::Number(value) => {
            node.output_type = Some(ScraperOutputType::Number);
            node.values.push(value.to_string());
        }
        Value::Bool(value) => {
            node.output_type = Some(ScraperOutputType::Boolean);
            node.values.push(value.to_string());
        }
        _ => bail!("unsupported value in project_json_relations template"),
    }
    Ok(node)
}
