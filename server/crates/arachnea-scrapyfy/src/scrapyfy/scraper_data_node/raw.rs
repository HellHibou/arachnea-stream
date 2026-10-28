//! Raw serde input shapes and conversion to scraper node trees.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::ScraperDataNode;

/// Raw JSON-compatible representation of a [`ScraperDataNode`].
///
/// The `#[serde(untagged)]` attribute lets serde pick the first variant that
/// deserializes successfully, which mirrors the serialization shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum ScraperDataNodeRaw {
    /// Empty node represented as JSON null.
    Null,
    /// Scalar strings stored directly on a node.
    Values(Vec<String>),
    /// Named child nodes, with `_` reserved for the parent's scalar values.
    Object(HashMap<String, ScraperDataNodeRaw>),
    /// Aligned rows of scalar fields.
    FlatArray(Vec<HashMap<String, String>>),
    /// Explicit nested node items.
    Array(Vec<ScraperDataNodeRaw>),
}

impl From<&ScraperDataNode> for ScraperDataNodeRaw {
    /// Converts a runtime node into its raw JSON shape, selecting the most
    /// compact representation (null, values array, flat array of objects,
    /// nested array, or plain object).
    ///
    /// # Arguments
    ///
    /// * `node` - Scraper node inspected or updated by this operation.
    fn from(node: &ScraperDataNode) -> Self {
        if !node.items.is_empty() {
            // When all items are simple scalars (no children, no nested
            // items), flatten them into a single Values array.  This is
            // used by entry-level sub-queries that store their results in
            // `items` to survive `keep_first_values` truncation.
            if node
                .items
                .iter()
                .all(|item| item.children.is_empty() && item.items.is_empty())
            {
                let flat: Vec<String> = node
                    .items
                    .iter()
                    .flat_map(|item| item.values.clone())
                    .collect();
                return ScraperDataNodeRaw::Values(flat);
            }
            return ScraperDataNodeRaw::Array(
                node.items.iter().map(ScraperDataNodeRaw::from).collect(),
            );
        }

        if node.children.is_empty() {
            if node.values.is_empty() {
                return ScraperDataNodeRaw::Null;
            }
            return ScraperDataNodeRaw::Values(node.values.clone());
        }

        if let Some(max_len) = node.array_len() {
            let mut items = Vec::with_capacity(max_len);
            for index in 0..max_len {
                let mut obj: HashMap<String, String> = HashMap::new();
                for (name, child) in &node.children {
                    if let Some(value) = child.scalar_at(index) {
                        obj.insert(name.clone(), value.to_string());
                    }
                }
                if let Some(value) = node.scalar_at(index) {
                    obj.insert("_".to_string(), value.to_string());
                }
                items.push(obj);
            }

            return ScraperDataNodeRaw::FlatArray(items);
        }

        let mut obj: HashMap<String, ScraperDataNodeRaw> = HashMap::new();
        for (name, child) in &node.children {
            obj.insert(name.clone(), ScraperDataNodeRaw::from(child));
        }

        if !node.values.is_empty() {
            obj.insert(
                "_".to_string(),
                ScraperDataNodeRaw::Values(node.values.clone()),
            );
        }

        ScraperDataNodeRaw::Object(obj)
    }
}

impl From<ScraperDataNode> for ScraperDataNodeRaw {
    /// Converts an owned node into its raw JSON shape.
    ///
    /// # Arguments
    ///
    /// * `node` - Scraper node inspected or updated by this operation.
    fn from(node: ScraperDataNode) -> Self {
        ScraperDataNodeRaw::from(&node)
    }
}

impl TryFrom<ScraperDataNodeRaw> for ScraperDataNode {
    type Error = String;

    /// Converts a raw JSON representation back into a runtime [`ScraperDataNode`].
    ///
    /// # Arguments
    ///
    /// * `raw` - Encoded component to decode, preserving the original text if decoding fails.
    ///
    /// # Errors
    ///
    /// Returns an error when a `_` key inside an object variant does not hold
    /// a string array.
    fn try_from(raw: ScraperDataNodeRaw) -> Result<Self, Self::Error> {
        match raw {
            ScraperDataNodeRaw::Null => Ok(ScraperDataNode::default()),
            ScraperDataNodeRaw::Values(values) => Ok(ScraperDataNode::from_values(values)),
            ScraperDataNodeRaw::Object(entries) => {
                let mut node = ScraperDataNode::default();
                for (key, value) in entries {
                    if key == "_" {
                        match value {
                            ScraperDataNodeRaw::Values(values) => {
                                node.values = values;
                            }
                            ScraperDataNodeRaw::Null => {}
                            _ => {
                                return Err(
                                    "Invalid '_' entry: expected an array of strings".to_string()
                                )
                            }
                        }
                    } else {
                        node.children.insert(key, ScraperDataNode::try_from(value)?);
                    }
                }
                Ok(node)
            }
            ScraperDataNodeRaw::FlatArray(items) => Ok(ScraperDataNode {
                output_type: None,
                items: items
                    .into_iter()
                    .map(|item| {
                        let mut child = ScraperDataNode::default();
                        for (key, value) in item {
                            if key == "_" {
                                child.values.push(value);
                            } else {
                                child.children.entry(key).or_default().values.push(value);
                            }
                        }
                        child
                    })
                    .collect(),
                ..Default::default()
            }),
            ScraperDataNodeRaw::Array(items) => Ok(ScraperDataNode {
                output_type: None,
                items: items
                    .into_iter()
                    .map(ScraperDataNode::try_from)
                    .collect::<Result<Vec<_>, _>>()?,
                ..Default::default()
            }),
        }
    }
}
