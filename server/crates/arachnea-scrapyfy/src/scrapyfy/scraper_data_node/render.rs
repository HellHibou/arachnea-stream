//! Typed JSON rendering and value conversion with diagnostic node paths.

use serde::{Serialize, Serializer};
use serde_json::{Map, Number, Value};

use super::{ScraperDataNode, ScraperOutputType};

impl ScraperDataNode {
    /// Renders this node into the final typed JSON value.
    ///
    /// # Errors
    ///
    /// Returns an error if a typed node cannot be cast to its requested JSON
    /// type, or when a scalar node receives multiple values.
    pub fn to_json_value(&self) -> std::result::Result<Value, String> {
        self.to_json_value_at("$")
    }

    /// Renders the declared node type as JSON, using the path to contextualize shape or value errors.
    ///
    /// # Arguments
    ///
    /// * `path` - Diagnostic JSON path identifying this node in conversion errors.
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

    /// Renders an untyped object/container or null; untyped scalar values return an error.
    ///
    /// # Arguments
    ///
    /// * `path` - Diagnostic JSON path identifying this node in conversion errors.
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

    /// Renders one string or null; multiple scalar values return a cardinality error.
    ///
    /// # Arguments
    ///
    /// * `path` - Diagnostic JSON path identifying this node in conversion errors.
    fn render_single_string(&self, path: &str) -> std::result::Result<Value, String> {
        let Some(value) = self.single_value(path, ScraperOutputType::String)? else {
            return Ok(Value::Null);
        };

        Ok(Value::String(value.to_string()))
    }

    /// Renders one finite JSON number, or null for an absent or blank value.
    ///
    /// # Arguments
    ///
    /// * `path` - Diagnostic JSON path identifying this node in conversion errors.
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

    /// Renders one accepted boolean spelling, or null when the scalar is absent.
    ///
    /// # Arguments
    ///
    /// * `path` - Diagnostic JSON path identifying this node in conversion errors.
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

    /// Renders the scalar strings as a JSON array, or null when no values exist.
    fn render_string_array(&self) -> std::result::Result<Value, String> {
        if self.values.is_empty() {
            return Ok(Value::Null);
        }

        Ok(Value::Array(
            self.values.iter().cloned().map(Value::String).collect(),
        ))
    }

    /// Parses scalar values into a finite-number array, preserving null for an empty node.
    ///
    /// # Arguments
    ///
    /// * `path` - Diagnostic JSON path identifying this node in conversion errors.
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

    /// Parses scalar values into a boolean array, preserving null for an empty node.
    ///
    /// # Arguments
    ///
    /// * `path` - Diagnostic JSON path identifying this node in conversion errors.
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

    /// Renders one object or null, rejecting more than one explicit object item.
    ///
    /// # Arguments
    ///
    /// * `path` - Diagnostic JSON path identifying this node in conversion errors.
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

    /// Renders explicit items or aligned child values as an object array, or null for an empty node.
    ///
    /// # Arguments
    ///
    /// * `path` - Diagnostic JSON path identifying this node in conversion errors.
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

    /// Renders children recursively into an object, retaining container values under the `_` key.
    ///
    /// # Arguments
    ///
    /// * `path` - Diagnostic JSON path identifying this node in conversion errors.
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

    /// Renders a child at an aligned object-array index with path-aware conversion errors.
    ///
    /// # Arguments
    ///
    /// * `index` - Zero-based item or scalar position to read.
    /// * `path` - Diagnostic JSON path identifying this node in conversion errors.
    fn render_indexed_value(&self, index: usize, path: &str) -> std::result::Result<Value, String> {
        if self.values.is_empty() {
            if !self.children.is_empty() && self.items.is_empty() {
                let mut object = Map::new();
                for (name, child) in &self.children {
                    let child_path = join_path(path, name);
                    object.insert(
                        name.clone(),
                        child.render_indexed_value(index, &child_path)?,
                    );
                }
                return Ok(Value::Object(object));
            }

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

    /// Returns at most one scalar; empty nodes yield `None` and multiple values return an error.
    ///
    /// # Arguments
    ///
    /// * `path` - Diagnostic JSON path identifying this node in conversion errors.
    /// * `output_type` - Expected output type included in cardinality or value-conversion diagnostics.
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
    ///
    /// # Arguments
    ///
    /// * `serializer` - Serde serializer receiving the raw or typed representation.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_json_value()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

/// Appends a child name to a dotted diagnostic JSON path.
///
/// # Arguments
///
/// * `parent` - Parent diagnostic path used when formatting a child path.
/// * `child` - Child field name appended to the diagnostic path.
fn join_path(parent: &str, child: &str) -> String {
    if parent == "$" {
        format!("$.{}", child)
    } else {
        format!("{}.{}", parent, child)
    }
}

/// Parses a trimmed signed/unsigned integer or finite float, rejecting blank and non-finite values.
///
/// # Arguments
///
/// * `value` - Input value interpreted by this operation.
/// * `path` - Diagnostic JSON path identifying this node in conversion errors.
/// * `output_type` - Expected output type included in cardinality or value-conversion diagnostics.
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

/// Parses case-insensitive true/false, 1/0, yes/no or y/n, rejecting other values.
///
/// # Arguments
///
/// * `value` - Input value interpreted by this operation.
/// * `path` - Diagnostic JSON path identifying this node in conversion errors.
/// * `output_type` - Expected output type included in cardinality or value-conversion diagnostics.
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
