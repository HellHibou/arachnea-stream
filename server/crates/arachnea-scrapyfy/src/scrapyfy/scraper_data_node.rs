mod output_type;
mod raw;
mod render;

pub use output_type::ScraperOutputType;
use raw::ScraperDataNodeRaw;

use serde::Deserialize;
use std::collections::HashMap;

/// Tree-shaped scraper output node used for hierarchical JSON results.
///
/// When serialized, a typed node becomes the JSON shape requested by
/// [`ScraperOutputType`]. Untyped container nodes still render their children
/// as JSON objects so intermediate paths such as `img/poster > link` do not
/// need their own YAML type.
///
/// If a node has both values and children, the values are emitted under the `_` key.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(try_from = "ScraperDataNodeRaw")]
pub struct ScraperDataNode {
    /// JSON output type requested by the YAML entry that owns this node.
    pub output_type: Option<ScraperOutputType>,
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
            output_type: None,
            values,
            children: HashMap::new(),
            items: Vec::new(),
        }
    }

    /// Creates a typed node containing only scalar values.
    ///
    /// # Arguments
    ///
    /// * `values` - Scalar values stored directly on the new node.
    /// * `output_type` - JSON type used when the node is serialized.
    pub fn from_values_typed(values: Vec<String>, output_type: ScraperOutputType) -> Self {
        Self {
            output_type: Some(output_type),
            values,
            children: HashMap::new(),
            items: Vec::new(),
        }
    }

    /// Returns the first scalar value as a `&str`, or `None` if the node is empty.
    pub fn value_as_string(&self) -> Option<&str> {
        self.values.first().map(String::as_str)
    }

    /// Returns the first scalar value parsed as `u16`, or `None`.
    pub fn value_as_u16(&self) -> Option<u16> {
        self.values.first().and_then(|v| v.parse().ok())
    }

    /// Returns the first scalar value parsed as `u32`, or `None`.
    pub fn value_as_u32(&self) -> Option<u32> {
        self.values.first().and_then(|v| v.parse().ok())
    }

    /// Returns the first scalar value parsed as `u64`, or `None`.
    pub fn value_as_u64(&self) -> Option<u64> {
        self.values.first().and_then(|v| v.parse().ok())
    }

    /// Returns the first scalar value parsed as `bool`, or `None`.
    pub fn value_as_bool(&self) -> Option<bool> {
        self.values.first().and_then(|v| v.parse().ok())
    }

    /// Stores a YAML-declared output type under the provided `>`-split path.
    ///
    /// Missing intermediate nodes are created on demand. Existing compatible
    /// types are preserved; incompatible type conflicts are detected later by
    /// YAML validation or serialization.
    ///
    /// # Arguments
    ///
    /// * `path` - Ordered path segments where the output type is stored.
    /// * `output_type` - JSON output type declared by the entry.
    pub fn set_output_type(&mut self, path: &[&str], output_type: ScraperOutputType) {
        if path.is_empty() {
            return;
        }

        let mut current = self;
        for segment in path {
            current = current.children.entry((*segment).to_string()).or_default();
        }

        current.output_type.get_or_insert(output_type);
    }

    /// Assigns an output type directly to this node.
    ///
    /// Existing compatible types are preserved.
    ///
    /// # Arguments
    ///
    /// * `output_type` - JSON output type declared by the owner entry.
    pub fn set_self_output_type(&mut self, output_type: ScraperOutputType) {
        self.output_type.get_or_insert(output_type);
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

    /// Pushes one scalar value and records the output type on the target node.
    ///
    /// # Arguments
    ///
    /// * `path` - Ordered path segments where the value is stored.
    /// * `value` - Value appended to the resolved target node.
    /// * `output_type` - JSON output type declared by the entry.
    pub fn push_value_typed(
        &mut self,
        path: &[&str],
        value: String,
        output_type: ScraperOutputType,
    ) {
        self.set_output_type(path, output_type);
        self.push_value(path, value);
    }

    /// Sets one scalar value under the provided `>`-split path, replacing
    /// any previously stored values. Used by entries with `select: first`.
    ///
    /// Intermediate child nodes are created on demand.
    ///
    /// # Arguments
    ///
    /// * `path` - Ordered path segments where the value is stored.
    /// * `value` - Value assigned to the resolved target node (replaces existing).
    pub fn set_value(&mut self, path: &[&str], value: String) {
        if path.is_empty() {
            return;
        }

        let mut current = self;
        for segment in path {
            current = current.children.entry((*segment).to_string()).or_default();
        }

        current.values = vec![value];
    }

    /// Sets one scalar value and records the output type on the target node.
    ///
    /// # Arguments
    ///
    /// * `path` - Ordered path segments where the value is stored.
    /// * `value` - Value assigned to the resolved target node.
    /// * `output_type` - JSON output type declared by the entry.
    pub fn set_value_typed(
        &mut self,
        path: &[&str],
        value: String,
        output_type: ScraperOutputType,
    ) {
        self.set_output_type(path, output_type);
        self.set_value(path, value);
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

    /// Appends one nested node and records the output type on the target node.
    ///
    /// # Arguments
    ///
    /// * `path` - Ordered path segments where the node is stored.
    /// * `node` - Nested data node appended to the resolved target.
    /// * `output_type` - JSON output type declared by the group entry.
    pub fn push_node_typed(
        &mut self,
        path: &[&str],
        node: ScraperDataNode,
        output_type: ScraperOutputType,
    ) {
        self.set_output_type(path, output_type);
        self.push_node(path, node);
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
        if self.output_type.is_none() {
            self.output_type = other.output_type;
        }

        self.values.extend(other.values);

        for (name, child) in other.children {
            self.children.entry(name).or_default().merge(child);
        }

        self.items.extend(other.items);
    }

    /// Merges another node into this one, keeping scalar values from `self`
    /// (the first item) and extending items (group entries). Child nodes
    /// are merged recursively with `merge_first` semantics so that repeated
    /// scalar fields (e.g. `current_page`, `have_more`) are not duplicated.
    ///
    /// This is used when row_pointer: /* produces multiple rows where each
    /// row carries the same scalar values (current_page, have_more) and
    /// different group entries (episodes items).
    ///
    /// # Arguments
    ///
    /// * `other` - Node whose children/items are merged; its scalar values
    ///   are discarded.
    pub fn merge_first(&mut self, other: &ScraperDataNode) {
        if self.output_type.is_none() {
            self.output_type = other.output_type;
        }

        for (name, child) in &other.children {
            let entry = self.children.entry(name.clone()).or_default();
            if entry.output_type.is_none() {
                entry.output_type = child.output_type;
            }
            if entry.values.is_empty() {
                entry.values = child.values.clone();
            }
            entry.items.extend(child.items.iter().cloned());
            entry.merge_children_first(child);
        }

        self.items.extend(other.items.iter().cloned());
    }

    /// Recursively merges child nodes using `merge_first` semantics (scalar
    /// values from the first occurrence are kept).
    ///
    /// # Arguments
    ///
    /// * `other` - Incoming node whose children are merged while preserving existing children.
    fn merge_children_first(&mut self, other: &ScraperDataNode) {
        for (name, child) in &other.children {
            let entry = self.children.entry(name.clone()).or_default();
            entry.merge_first(child);
        }
    }

    /// Keeps only the first value at every leaf node, discarding subsequent
    /// duplicates.  Used after merge to collapse repeated scalar fields
    /// (e.g. `current_page`, `have_more`) while preserving group entries.
    pub fn keep_first_values(&mut self) {
        if self.values.len() > 1
            && !matches!(
                self.output_type,
                Some(
                    ScraperOutputType::StringArray
                        | ScraperOutputType::NumberArray
                        | ScraperOutputType::BooleanArray
                )
            )
        {
            self.values.truncate(1);
        }
        for child in self.children.values_mut() {
            child.keep_first_values();
        }
        for item in &mut self.items {
            item.keep_first_values();
        }
    }

    /// Returns the scalar value at the given index in this node.
    ///
    /// # Arguments
    ///
    /// * `index` - Zero-based item or scalar position to read.
    fn scalar_at(&self, index: usize) -> Option<&str> {
        self.values.get(index).map(String::as_str)
    }

    /// Returns the aligned array length when the node can be serialized as a
    /// flat array of objects, or `None` otherwise.
    ///
    /// A node qualifies when it has no explicit items, at least two children
    /// without explicit items, and the maximum child value count exceeds one.
    /// Nested child fields are rendered at the same index, allowing aligned
    /// arrays to contain structured values such as player resolvers.
    fn array_len(&self) -> Option<usize> {
        if !self.items.is_empty() {
            return None;
        }

        if self.children.len() < 2 {
            return None;
        }

        if self.children.values().any(|child| !child.items.is_empty()) {
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
