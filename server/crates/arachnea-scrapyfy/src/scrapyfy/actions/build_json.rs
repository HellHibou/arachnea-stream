use anyhow::{bail, Context, Result};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::collections::HashMap;

/// One value source used by the `build_json` request action.
///
/// The YAML syntax is a single-key mapping:
///
/// ```yaml
/// source: {string_parameter: translation_source_language}
/// blocks: {json_parameter: translation_blocks_json}
/// texts:
///   json_array_projection:
///     parameter: translation_blocks_json
///     path: /text
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum BuildJsonField {
    /// Inserts the runtime parameter as a JSON string, escaping its content.
    StringParameter(String),
    /// Decodes the runtime parameter as validated JSON and inserts the value.
    JsonParameter(String),
    /// Projects one JSON pointer from every item of a JSON array parameter.
    JsonArrayProjection(JsonArrayProjection),
}

/// Projects one field from every item of a JSON array runtime parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonArrayProjection {
    /// Runtime parameter holding the source JSON array.
    pub parameter: String,
    /// JSON pointer applied to every source item. Empty selects the item itself.
    #[serde(default)]
    pub path: String,
}

/// Source of the `build_json_array` action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildJsonArraySource {
    /// Runtime parameter holding the source JSON array.
    pub json_parameter: String,
}

impl<'de> Deserialize<'de> for BuildJsonField {
    /// Parses the single-key `string_parameter` or `json_parameter` mapping.
    ///
    /// # Errors
    ///
    /// Returns an error when the value is not a one-entry mapping, when the
    /// selected parameter name is not a string, or when the key is unsupported.
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| D::Error::custom("build_json field must be a single-key mapping"))?;
        if object.len() != 1 {
            return Err(D::Error::custom(
                "build_json field must contain exactly one source",
            ));
        }
        let (source, raw_name) = object.iter().next().expect("one entry");
        match source.as_str() {
            "string_parameter" => raw_name
                .as_str()
                .map(|parameter| Self::StringParameter(parameter.to_string()))
                .ok_or_else(|| D::Error::custom("build_json parameter name must be a string")),
            "json_parameter" => raw_name
                .as_str()
                .map(|parameter| Self::JsonParameter(parameter.to_string()))
                .ok_or_else(|| D::Error::custom("build_json parameter name must be a string")),
            "json_array_projection" => {
                serde_json::from_value::<JsonArrayProjection>(raw_name.clone())
                    .map(Self::JsonArrayProjection)
                    .map_err(|error| {
                        D::Error::custom(format!(
                            "invalid build_json json_array_projection source: {error}"
                        ))
                    })
            }
            other => Err(D::Error::custom(format!(
                "unsupported build_json field source {other}; supported sources are string_parameter, json_parameter and json_array_projection"
            ))),
        }
    }
}

impl Serialize for BuildJsonField {
    /// Serializes the field back to its single-key YAML/JSON mapping.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            Self::StringParameter(parameter) => {
                map.serialize_entry("string_parameter", parameter)?
            }
            Self::JsonParameter(parameter) => map.serialize_entry("json_parameter", parameter)?,
            Self::JsonArrayProjection(projection) => {
                map.serialize_entry("json_array_projection", projection)?
            }
        }
        map.end()
    }
}

/// Applies the `build_json` action by resolving every configured field and
/// serializing the resulting object as the current value.
///
/// # Arguments
///
/// * `texts` - Values produced by earlier actions; replaced by the serialized object.
/// * `fields` - Object field names mapped to their value sources.
/// * `params` - Runtime parameters referenced by the field sources.
///
/// # Returns
///
/// A single-element list containing the serialized JSON object.
///
/// # Errors
///
/// Returns an error when a referenced parameter is absent, when a
/// `json_parameter` value is not valid JSON, or when serialization fails.
pub(super) fn apply(
    mut texts: Vec<String>,
    fields: &HashMap<String, BuildJsonField>,
    params: &HashMap<String, String>,
) -> Result<Vec<String>> {
    let mut object = serde_json::Map::with_capacity(fields.len());
    for (name, field) in fields {
        let value = match field {
            BuildJsonField::StringParameter(parameter) => Value::String(
                params
                    .get(parameter)
                    .with_context(|| format!("Missing build_json string parameter {parameter}."))?
                    .clone(),
            ),
            BuildJsonField::JsonParameter(parameter) => {
                let raw = params
                    .get(parameter)
                    .with_context(|| format!("Missing build_json JSON parameter {parameter}."))?;
                serde_json::from_str(raw).with_context(|| {
                    format!("Invalid JSON value for build_json parameter {parameter}.")
                })?
            }
            BuildJsonField::JsonArrayProjection(projection) => {
                let raw = params.get(&projection.parameter).with_context(|| {
                    format!(
                        "Missing build_json JSON array parameter {}.",
                        projection.parameter
                    )
                })?;
                let value: Value = serde_json::from_str(raw).with_context(|| {
                    format!(
                        "Invalid JSON value for build_json parameter {}.",
                        projection.parameter
                    )
                })?;
                let items = value.as_array().with_context(|| {
                    format!(
                        "build_json parameter {} must be a JSON array.",
                        projection.parameter
                    )
                })?;
                Value::Array(
                    items
                        .iter()
                        .enumerate()
                        .map(|(index, item)| {
                            item.pointer(&projection.path).cloned().with_context(|| {
                                format!(
                                    "build_json parameter {} item {index} has no value at JSON pointer {}.",
                                    projection.parameter, projection.path
                                )
                            })
                        })
                        .collect::<Result<Vec<_>>>()?,
                )
            }
        };
        object.insert(name.clone(), value);
    }

    let serialized = serde_json::to_string(&Value::Object(object))
        .context("Failed to serialize build_json output.")?;
    texts.clear();
    texts.push(serialized);
    Ok(texts)
}

/// Validates the configuration of a `build_json` action.
///
/// # Arguments
///
/// * `owner_name` - Name of the owning entry included in validation diagnostics.
/// * `owner` - Owning query or entry label used to contextualize validation errors.
/// * `fields` - Object field names mapped to their value sources.
///
/// # Errors
///
/// Returns an error when no field is declared or when a field name is blank.
pub(super) fn validate(
    owner_name: &str,
    owner: &str,
    fields: &HashMap<String, BuildJsonField>,
) -> Result<()> {
    if fields.is_empty() {
        bail!("{owner} {owner_name} build_json requires at least one field");
    }
    if fields.keys().any(|name| name.trim().is_empty()) {
        bail!("{owner} {owner_name} build_json has a blank field name");
    }
    for field in fields.values() {
        match field {
            BuildJsonField::JsonArrayProjection(projection) => {
                if projection.parameter.trim().is_empty() {
                    bail!("{owner} {owner_name} build_json has a blank array parameter name");
                }
                validate_pointer(owner_name, owner, "build_json", &projection.path)?;
            }
            BuildJsonField::StringParameter(parameter)
            | BuildJsonField::JsonParameter(parameter) => {
                if parameter.trim().is_empty() {
                    bail!("{owner} {owner_name} build_json has a blank parameter name");
                }
            }
        }
    }
    Ok(())
}

/// Applies the `build_json_array` action by projecting each declared field from
/// every item of a JSON array runtime parameter.
///
/// # Arguments
///
/// * `texts` - Values produced by earlier actions; replaced by the serialized array.
/// * `source` - Runtime parameter holding the source JSON array.
/// * `fields` - Output property names mapped to JSON pointers inside each item.
/// * `params` - Runtime parameters referenced by the source.
///
/// # Returns
///
/// A single-element list containing the serialized JSON array.
///
/// # Errors
///
/// Returns an error when the parameter is absent, is not a JSON array, or an
/// item does not contain one of the configured JSON pointers.
pub(super) fn apply_array(
    mut texts: Vec<String>,
    source: &BuildJsonArraySource,
    fields: &HashMap<String, String>,
    params: &HashMap<String, String>,
) -> Result<Vec<String>> {
    let raw = params.get(&source.json_parameter).with_context(|| {
        format!(
            "Missing build_json_array JSON parameter {}.",
            source.json_parameter
        )
    })?;
    let value: Value = serde_json::from_str(raw).with_context(|| {
        format!(
            "Invalid JSON value for build_json_array parameter {}.",
            source.json_parameter
        )
    })?;
    let items = value.as_array().with_context(|| {
        format!(
            "build_json_array parameter {} must be a JSON array.",
            source.json_parameter
        )
    })?;

    let mut output = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let mut object = serde_json::Map::with_capacity(fields.len());
        for (name, pointer) in fields {
            let selected = item.pointer(pointer).cloned().with_context(|| {
                format!(
                    "build_json_array parameter {} item {index} has no value at JSON pointer {pointer}.",
                    source.json_parameter
                )
            })?;
            object.insert(name.clone(), selected);
        }
        output.push(Value::Object(object));
    }

    let serialized = serde_json::to_string(&Value::Array(output))
        .context("Failed to serialize build_json_array output.")?;
    texts.clear();
    texts.push(serialized);
    Ok(texts)
}

/// Validates the configuration of a `build_json_array` action.
///
/// # Arguments
///
/// * `owner_name` - Name of the owning entry included in validation diagnostics.
/// * `owner` - Owning query or entry label used to contextualize validation errors.
/// * `source` - Runtime parameter holding the source JSON array.
/// * `fields` - Output property names mapped to JSON pointers.
///
/// # Errors
///
/// Returns an error when the source or a field is blank, when no field is
/// declared, or when a JSON pointer is not syntactically valid.
pub(super) fn validate_array(
    owner_name: &str,
    owner: &str,
    source: &BuildJsonArraySource,
    fields: &HashMap<String, String>,
) -> Result<()> {
    if source.json_parameter.trim().is_empty() {
        bail!("{owner} {owner_name} build_json_array has a blank source parameter name");
    }
    if fields.is_empty() {
        bail!("{owner} {owner_name} build_json_array requires at least one field");
    }
    for (name, pointer) in fields {
        if name.trim().is_empty() {
            bail!("{owner} {owner_name} build_json_array has a blank field name");
        }
        validate_pointer(owner_name, owner, "build_json_array", pointer)?;
    }
    Ok(())
}

/// Validates the syntax of an optional JSON pointer.
///
/// # Arguments
///
/// * `owner_name` - Name of the owning entry included in validation diagnostics.
/// * `owner` - Owning query or entry label used to contextualize validation errors.
/// * `action` - Action name included in validation diagnostics.
/// * `pointer` - JSON pointer to validate; an empty pointer selects the value itself.
///
/// # Errors
///
/// Returns an error when a non-empty pointer does not start with `/` or contains
/// an invalid `~` escape.
fn validate_pointer(owner_name: &str, owner: &str, action: &str, pointer: &str) -> Result<()> {
    if pointer.is_empty() {
        return Ok(());
    }
    if !pointer.starts_with('/') {
        bail!("{owner} {owner_name} {action} JSON pointer {pointer} must start with `/`");
    }
    let bytes = pointer.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'~' {
            match bytes.get(index + 1) {
                Some(b'0' | b'1') => {}
                _ => bail!(
                    "{owner} {owner_name} {action} JSON pointer {pointer} has an invalid escape"
                ),
            }
        }
    }
    Ok(())
}
