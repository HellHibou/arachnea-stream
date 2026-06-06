use anyhow::Result;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;

/// Tree-shaped scraper output node used for hierarchical JSON results.
///
/// When serialized, a node can become:
/// - `null` if it has neither values nor children,
/// - an array of strings if it only has values,
/// - an object made of child nodes,
/// - an explicit array of nested nodes when group entries are used,
/// - or an array of objects when several leaf children expose aligned lists.
///
/// If a node has both values and children, the values are emitted under the `_` key.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(try_from = "ScraperDataNodeRaw")]
pub struct ScraperDataNode {
    /// Scalar values stored at this node level.
    pub values: Vec<String>,
    /// Named child nodes for hierarchical fields.
    pub children: HashMap<String, ScraperDataNode>,
    /// Explicit array-like nested items.
    pub items: Vec<ScraperDataNode>,
}

impl ScraperDataNode {
    /// Creates a node containing only values and no children.
    ///
    /// # Arguments
    ///
    /// * `values` - Scalar values stored directly on the new node.
    pub fn from_values(values: Vec<String>) -> Self {
        Self {
            values,
            children: HashMap::new(),
            items: Vec::new(),
        }
    }

    /// Pushes one scalar value under the provided `>`-split path.
    ///
    /// Intermediate child nodes are created on demand.
    ///
    /// # Arguments
    ///
    /// * `path` - Ordered path segments where the value is stored.
    /// * `value` - Value appended to the resolved target node.
    pub fn push_value(&mut self, path: &[&str], value: String) {
        if path.is_empty() {
            return;
        }

        let mut current = self;
        for segment in path {
            current = current.children.entry((*segment).to_string()).or_default();
        }

        current.values.push(value);
    }

    /// Appends one nested node under the provided path as an explicit array item.
    ///
    /// Intermediate child nodes are created on demand.
    ///
    /// # Arguments
    ///
    /// * `path` - Ordered path segments where the node is stored.
    /// * `node` - Nested data node appended to the resolved target.
    pub fn push_node(&mut self, path: &[&str], node: ScraperDataNode) {
        if path.is_empty() {
            return;
        }

        let mut current = self;
        for segment in path {
            current = current.children.entry((*segment).to_string()).or_default();
        }

        current.items.push(node);
    }

    /// Merges another node into this one while preserving append order.
    ///
    /// Scalar values and explicit items are appended, while child nodes are
    /// merged recursively by key.
    ///
    /// # Arguments
    ///
    /// * `other` - Node whose values, children, and items are merged into `self`.
    pub fn merge(&mut self, other: ScraperDataNode) {
        self.values.extend(other.values);

        for (name, child) in other.children {
            self.children.entry(name).or_default().merge(child);
        }

        self.items.extend(other.items);
    }

    /// Returns the aligned array length when the node can be serialized as a
    /// flat array of objects, or `None` otherwise.
    ///
    /// A node qualifies when it has no explicit items, at least two children
    /// without nested children, and the maximum child value count exceeds one.
    fn array_len(&self) -> Option<usize> {
        if !self.items.is_empty() {
            return None;
        }

        if self.children.len() < 2 {
            return None;
        }

        if self
            .children
            .values()
            .any(|child| !child.children.is_empty())
        {
            return None;
        }

        let child_max_len = self
            .children
            .values()
            .map(|child| child.values.len())
            .max()
            .unwrap_or(0);

        if child_max_len <= 1 {
            return None;
        }

        Some(child_max_len.max(self.values.len()))
    }
}

impl Serialize for ScraperDataNode {
    /// Serializes the node through its raw representation, choosing the most
    /// compact JSON shape.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ScraperDataNodeRaw::from(self).serialize(serializer)
    }
}

/// Raw JSON-compatible representation of a [`ScraperDataNode`].
///
/// The `#[serde(untagged)]` attribute lets serde pick the first variant that
/// deserializes successfully, which mirrors the serialization shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum ScraperDataNodeRaw {
    Null,
    Values(Vec<String>),
    Object(HashMap<String, ScraperDataNodeRaw>),
    FlatArray(Vec<HashMap<String, String>>),
    Array(Vec<ScraperDataNodeRaw>),
}

impl From<&ScraperDataNode> for ScraperDataNodeRaw {
    /// Converts a runtime node into its raw JSON shape, selecting the most
    /// compact representation (null, values array, flat array of objects,
    /// nested array, or plain object).
    fn from(node: &ScraperDataNode) -> Self {
        if !node.items.is_empty() {
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
                    if let Some(value) = child.values.get(index) {
                        obj.insert(name.clone(), value.clone());
                    }
                }
                if let Some(value) = node.values.get(index) {
                    obj.insert("_".to_string(), value.clone());
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
    fn from(node: ScraperDataNode) -> Self {
        ScraperDataNodeRaw::from(&node)
    }
}

impl TryFrom<ScraperDataNodeRaw> for ScraperDataNode {
    type Error = String;

    /// Converts a raw JSON representation back into a runtime [`ScraperDataNode`].
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
                items: items
                    .into_iter()
                    .map(ScraperDataNode::try_from)
                    .collect::<Result<Vec<_>, _>>()?,
                ..Default::default()
            }),
        }
    }
}