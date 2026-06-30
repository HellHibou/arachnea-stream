use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Number, Value};
use std::collections::HashMap;

/// JSON output type requested by scraper YAML for one output node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScraperOutputType {
    /// JSON string scalar.
    String,
    /// JSON number scalar.
    Number,
    /// JSON boolean scalar.
    Boolean,
    /// JSON object produced from a scraper node.
    Object,
    /// JSON string array.
    StringArray,
    /// JSON number array.
    NumberArray,
    /// JSON boolean array.
    BooleanArray,
    /// JSON object array produced from scraper node items.
    ObjectArray,
}

impl ScraperOutputType {
    /// Returns the YAML spelling of this output type.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Object => "object",
            Self::StringArray => "string[]",
            Self::NumberArray => "number[]",
            Self::BooleanArray => "boolean[]",
            Self::ObjectArray => "object[]",
        }
    }

    /// Returns `true` when this type describes one JSON scalar value.
    pub fn is_scalar(self) -> bool {
        matches!(self, Self::String | Self::Number | Self::Boolean)
    }

    /// Returns the scalar element type for one primitive array type.
    pub fn array_element_type(self) -> Option<Self> {
        match self {
            Self::StringArray => Some(Self::String),
            Self::NumberArray => Some(Self::Number),
            Self::BooleanArray => Some(Self::Boolean),
            _ => None,
        }
    }
}

impl Serialize for ScraperOutputType {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ScraperOutputType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.trim() {
            "string" => Ok(Self::String),
            "number" => Ok(Self::Number),
            "boolean" => Ok(Self::Boolean),
            "object" => Ok(Self::Object),
            "string[]" => Ok(Self::StringArray),
            "number[]" => Ok(Self::NumberArray),
            "boolean[]" => Ok(Self::BooleanArray),
            "object[]" => Ok(Self::ObjectArray),
            other => Err(serde::de::Error::custom(format!(
                "unknown scraper output type `{}`",
                other
            ))),
        }
    }
}

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
    fn scalar_at(&self, index: usize) -> Option<&str> {
        self.values.get(index).map(String::as_str)
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

    /// Renders this node into the final typed JSON value.
    ///
    /// # Errors
    ///
    /// Returns an error if a typed node cannot be cast to its requested JSON
    /// type, or when a scalar node receives multiple values.
    pub fn to_json_value(&self) -> std::result::Result<Value, String> {
        self.to_json_value_at("$")
    }

    fn to_json_value_at(&self, path: &str) -> std::result::Result<Value, String> {
        match self.output_type {
            Some(ScraperOutputType::String) => self.render_single_string(path),
            Some(ScraperOutputType::Number) => self.render_single_number(path),
            Some(ScraperOutputType::Boolean) => self.render_single_boolean(path),
            Some(ScraperOutputType::Object) => self.render_object(path),
            Some(ScraperOutputType::StringArray) => self.render_string_array(),
            Some(ScraperOutputType::NumberArray) => self.render_number_array(path),
            Some(ScraperOutputType::BooleanArray) => self.render_boolean_array(path),
            Some(ScraperOutputType::ObjectArray) => self.render_object_array(path),
            None => self.render_untyped_container(path),
        }
    }

    fn render_untyped_container(&self, path: &str) -> std::result::Result<Value, String> {
        if !self.items.is_empty() {
            return self.render_object_array(path);
        }

        if !self.children.is_empty() {
            if self.array_len().is_some() {
                return self.render_object_array(path);
            }

            return self.render_object_from_children(path);
        }

        if self.values.is_empty() {
            return Ok(Value::Null);
        }

        Err(format!(
            "Missing output type for scalar scraper node at `{}` with values {:?}",
            path, self.values
        ))
    }

    fn render_single_string(&self, path: &str) -> std::result::Result<Value, String> {
        let Some(value) = self.single_value(path, ScraperOutputType::String)? else {
            return Ok(Value::Null);
        };

        Ok(Value::String(value.to_string()))
    }

    fn render_single_number(&self, path: &str) -> std::result::Result<Value, String> {
        let Some(value) = self.single_value(path, ScraperOutputType::Number)? else {
            return Ok(Value::Null);
        };

        if value.trim().is_empty() {
            return Ok(Value::Null);
        }

        Ok(Value::Number(parse_json_number(
            value,
            path,
            ScraperOutputType::Number,
        )?))
    }

    fn render_single_boolean(&self, path: &str) -> std::result::Result<Value, String> {
        let Some(value) = self.single_value(path, ScraperOutputType::Boolean)? else {
            return Ok(Value::Null);
        };

        Ok(Value::Bool(parse_json_bool(
            value,
            path,
            ScraperOutputType::Boolean,
        )?))
    }

    fn render_string_array(&self) -> std::result::Result<Value, String> {
        if self.values.is_empty() {
            return Ok(Value::Null);
        }

        Ok(Value::Array(
            self.values.iter().cloned().map(Value::String).collect(),
        ))
    }

    fn render_number_array(&self, path: &str) -> std::result::Result<Value, String> {
        if self.values.is_empty() {
            return Ok(Value::Null);
        }

        self.values
            .iter()
            .map(|value| {
                parse_json_number(value, path, ScraperOutputType::NumberArray).map(Value::Number)
            })
            .collect::<std::result::Result<Vec<_>, _>>()
            .map(Value::Array)
    }

    fn render_boolean_array(&self, path: &str) -> std::result::Result<Value, String> {
        if self.values.is_empty() {
            return Ok(Value::Null);
        }

        self.values
            .iter()
            .map(|value| {
                parse_json_bool(value, path, ScraperOutputType::BooleanArray).map(Value::Bool)
            })
            .collect::<std::result::Result<Vec<_>, _>>()
            .map(Value::Array)
    }

    fn render_object(&self, path: &str) -> std::result::Result<Value, String> {
        if self.children.is_empty() && self.items.is_empty() && self.values.is_empty() {
            return Ok(Value::Null);
        }

        if !self.items.is_empty() {
            if self.items.len() != 1 {
                return Err(format!(
                    "Cannot serialize `{}` as {}: expected one object item, received {}",
                    path,
                    ScraperOutputType::Object.as_str(),
                    self.items.len()
                ));
            }

            return self.items[0].render_object_from_children(path);
        }

        self.render_object_from_children(path)
    }

    fn render_object_array(&self, path: &str) -> std::result::Result<Value, String> {
        if self.items.is_empty() && self.children.is_empty() {
            return Ok(Value::Null);
        }

        if !self.items.is_empty() {
            return self
                .items
                .iter()
                .enumerate()
                .map(|(index, item)| item.render_object_from_children(&format!("{path}[{index}]")))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map(Value::Array);
        }

        if let Some(max_len) = self.array_len() {
            let mut items = Vec::with_capacity(max_len);
            for index in 0..max_len {
                let mut object = Map::new();
                for (name, child) in &self.children {
                    let child_path = join_path(path, name);
                    object.insert(
                        name.clone(),
                        child.render_indexed_value(index, &child_path)?,
                    );
                }
                items.push(Value::Object(object));
            }

            return Ok(Value::Array(items));
        }

        self.render_object_from_children(path)
            .map(|value| Value::Array(vec![value]))
    }

    fn render_object_from_children(&self, path: &str) -> std::result::Result<Value, String> {
        if self.children.is_empty() && self.values.is_empty() {
            return Ok(Value::Object(Map::new()));
        }

        let mut object = Map::new();
        for (name, child) in &self.children {
            let child_path = join_path(path, name);
            object.insert(name.clone(), child.to_json_value_at(&child_path)?);
        }

        if !self.values.is_empty() {
            let value = if self.values.len() == 1 {
                Value::String(self.values[0].clone())
            } else {
                Value::Array(self.values.iter().cloned().map(Value::String).collect())
            };
            object.insert("_".to_string(), value);
        }

        Ok(Value::Object(object))
    }

    fn render_indexed_value(
        &self,
        index: usize,
        path: &str,
    ) -> std::result::Result<Value, String> {
        if self.values.is_empty() {
            return self.to_json_value_at(path);
        }

        let value = self
            .values
            .get(index)
            .or_else(|| self.values.first())
            .ok_or_else(|| format!("Missing indexed value for `{}` at {}", path, index))?;

        match self.output_type {
            Some(ScraperOutputType::String) | Some(ScraperOutputType::StringArray) | None => {
                Ok(Value::String(value.clone()))
            }
            Some(ScraperOutputType::Number) | Some(ScraperOutputType::NumberArray) => {
                parse_json_number(value, path, self.output_type.unwrap()).map(Value::Number)
            }
            Some(ScraperOutputType::Boolean) | Some(ScraperOutputType::BooleanArray) => {
                parse_json_bool(value, path, self.output_type.unwrap()).map(Value::Bool)
            }
            Some(ScraperOutputType::Object | ScraperOutputType::ObjectArray) => {
                self.to_json_value_at(path)
            }
        }
    }

    fn single_value(
        &self,
        path: &str,
        output_type: ScraperOutputType,
    ) -> std::result::Result<Option<&str>, String> {
        if self.values.is_empty() {
            return Ok(None);
        }

        if self.values.len() > 1 {
            return Err(format!(
                "Cannot serialize `{}` as {}: expected one value, received {:?}",
                path,
                output_type.as_str(),
                self.values
            ));
        }

        Ok(self.values.first().map(String::as_str))
    }
}

impl Serialize for ScraperDataNode {
    /// Serializes the node to its typed JSON representation.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_json_value()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

fn join_path(parent: &str, child: &str) -> String {
    if parent == "$" {
        format!("$.{}", child)
    } else {
        format!("{}.{}", parent, child)
    }
}

fn parse_json_number(
    value: &str,
    path: &str,
    output_type: ScraperOutputType,
) -> std::result::Result<Number, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!(
            "Cannot serialize `{}` value {:?} as {}: empty number",
            path,
            value,
            output_type.as_str()
        ));
    }

    if let Ok(value) = trimmed.parse::<i64>() {
        return Ok(Number::from(value));
    }

    if let Ok(value) = trimmed.parse::<u64>() {
        return Ok(Number::from(value));
    }

    let parsed = trimmed.parse::<f64>().map_err(|_| {
        format!(
            "Cannot serialize `{}` value {:?} as {}: invalid number",
            path,
            value,
            output_type.as_str()
        )
    })?;

    Number::from_f64(parsed).ok_or_else(|| {
        format!(
            "Cannot serialize `{}` value {:?} as {}: number is not finite",
            path,
            value,
            output_type.as_str()
        )
    })
}

fn parse_json_bool(
    value: &str,
    path: &str,
    output_type: ScraperOutputType,
) -> std::result::Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "y" => Ok(true),
        "false" | "0" | "no" | "n" => Ok(false),
        _ => Err(format!(
            "Cannot serialize `{}` value {:?} as {}: invalid boolean",
            path,
            value,
            output_type.as_str()
        )),
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
