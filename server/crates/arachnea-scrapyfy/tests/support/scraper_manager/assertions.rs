//! Provider-independent assertions over serialized public payloads.

use anyhow::{ensure, Result};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;

/// Minimum useful-data requirements applied to one operation's public roots.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expectations {
    /// Literal object keys with explicit `*` array expansion.
    pub items_path: Vec<String>,
    /// Minimum number of selected items, including invalid ones.
    pub min_count: usize,
    /// Minimum number satisfying every per-object assertion.
    pub min_valid_count: usize,
    /// Required nonempty field paths relative to each selected object.
    pub required_non_empty: Vec<Vec<String>>,
    /// Constraints applied only when an optional field exists.
    #[serde(default)]
    pub optional_fields: Vec<OptionalField>,
    /// Requires every selected item to satisfy the assertions.
    #[serde(default)]
    pub all_valid: bool,
}

/// Type and optional nonempty constraint on a field that may be absent.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalField {
    /// Literal field path relative to the selected object.
    pub path: Vec<String>,
    /// Required JSON type when the field exists, including explicit null.
    #[serde(rename = "type")]
    pub value_type: JsonType,
    /// Rejects empty values when true.
    #[serde(default)]
    pub non_empty: bool,
}

/// JSON types supported by optional-field assertions; null is never valid.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsonType {
    /// JSON string, without coercion.
    String,
    /// JSON number, without coercion.
    Number,
    /// JSON boolean, without coercion.
    Boolean,
    /// JSON object.
    Object,
    /// JSON array.
    Array,
}

impl JsonType {
    fn matches(&self, value: &Value) -> bool {
        match self {
            Self::String => value.is_string(),
            Self::Number => value.is_number(),
            Self::Boolean => value.is_boolean(),
            Self::Object => value.is_object(),
            Self::Array => value.is_array(),
        }
    }
}

/// Assertion outcome, independent of transport and application operations.
#[derive(Debug, PartialEq, Eq)]
pub enum AssertionOutcome {
    /// Both minima and all declared constraints are satisfied.
    Passed,
    /// Fewer items were extracted than requested.
    ExtractionFailed,
    /// Shape or per-object requirements failed.
    ContractFailed,
}

impl AssertionOutcome {
    /// Returns the stable result name used in source-check reports.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::ExtractionFailed => "extraction_failed",
            Self::ContractFailed => "contract_failed",
        }
    }
}

/// Counts and structural diagnostics for one evaluated expectation.
#[derive(Debug)]
pub struct AssertionReport {
    /// Stable assertion classification.
    pub outcome: AssertionOutcome,
    /// Selected items, including malformed items.
    pub total: usize,
    /// Objects satisfying every field assertion.
    pub valid: usize,
    /// Selected items failing at least one field assertion.
    pub invalid: usize,
    /// Structural errors; payload values are never included.
    pub shape_errors: Vec<String>,
    /// Failed field paths for each invalid item, indexed in selection order.
    pub invalid_fields: Vec<(usize, Vec<Vec<String>>)>,
    /// Actual JSON kinds at invalid paths, without logging field values.
    pub invalid_types: Vec<(usize, Vec<String>, &'static str)>,
}

/// Rejects blank path segments and wildcards where only literal keys are allowed.
///
/// # Arguments
/// * `path` - Ordered path segments; an empty path selects the current value.
/// * `allow_wildcard` - Whether explicit array expansion is permitted.
///
/// # Errors
/// Returns an error for blank segments or disallowed wildcards.
pub fn validate_path(path: &[String], allow_wildcard: bool) -> Result<()> {
    ensure!(
        path.iter().all(|key| !key.trim().is_empty()),
        "Blank path segment"
    );
    ensure!(
        allow_wildcard || !path.iter().any(|key| key == "*"),
        "Wildcard in literal field path"
    );
    Ok(())
}

impl Expectations {
    /// Validates useful-data minima, field paths and conflicting declarations.
    ///
    /// # Errors
    /// Returns an error before execution if the expectations are inconsistent.
    pub fn validate(&self) -> Result<()> {
        validate_path(&self.items_path, true)?;
        ensure!(
            self.min_count > 0 && self.min_valid_count > 0,
            "Item minima must be positive"
        );
        ensure!(
            self.min_valid_count <= self.min_count,
            "min_valid_count exceeds min_count"
        );
        let mut paths = HashSet::new();
        for path in &self.required_non_empty {
            validate_path(path, false)?;
            ensure!(paths.insert(path), "Duplicate required path {path:?}");
        }
        ensure!(
            self.required_non_empty.iter().any(|path| {
                !path.is_empty() && path.as_slice() != ["source"] && path.as_slice() != ["id"]
            }),
            "Expected at least one useful field beyond source/id"
        );
        for field in &self.optional_fields {
            validate_path(&field.path, false)?;
            ensure!(
                paths.insert(&field.path),
                "Duplicate or conflicting optional path {:?}",
                field.path
            );
        }
        Ok(())
    }

    /// Evaluates already-validated expectations over individual public root values.
    ///
    /// # Arguments
    /// * `roots` - Public serialized roots, without an extra vector container.
    pub fn evaluate(&self, roots: &[Value]) -> AssertionReport {
        let (items, shape_errors) = select_items(roots, &self.items_path);
        let mut invalid_fields = Vec::new();
        let mut invalid_types = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let fields = self.invalid_paths(item);
            if !fields.is_empty() {
                for path in &fields {
                    let kind = match lookup(item, path) {
                        Err(()) => "non_object_parent",
                        Ok(None) => "missing",
                        Ok(Some(Value::Null)) => "null",
                        Ok(Some(Value::Bool(_))) => "boolean",
                        Ok(Some(Value::Number(_))) => "number",
                        Ok(Some(Value::String(_))) => "string",
                        Ok(Some(Value::Array(_))) => "array",
                        Ok(Some(Value::Object(_))) => "object",
                    };
                    invalid_types.push((index, path.clone(), kind));
                }
                invalid_fields.push((index, fields));
            }
        }
        let total = items.len();
        let invalid = invalid_fields.len();
        let valid = total - invalid;
        let outcome = if !shape_errors.is_empty() {
            AssertionOutcome::ContractFailed
        } else if total < self.min_count {
            AssertionOutcome::ExtractionFailed
        } else if valid < self.min_valid_count || (self.all_valid && invalid > 0) {
            AssertionOutcome::ContractFailed
        } else {
            AssertionOutcome::Passed
        };
        AssertionReport {
            outcome,
            total,
            valid,
            invalid,
            shape_errors,
            invalid_fields,
            invalid_types,
        }
    }

    fn invalid_paths(&self, item: &Value) -> Vec<Vec<String>> {
        if !item.is_object() {
            return vec![Vec::new()];
        }
        let mut invalid = Vec::new();
        for path in &self.required_non_empty {
            if !literal_value(item, path).is_some_and(non_empty) {
                invalid.push(path.clone());
            }
        }
        for field in &self.optional_fields {
            match lookup(item, &field.path) {
                Ok(None) => (),
                Ok(Some(value))
                    if field.value_type.matches(value)
                        && (!field.non_empty || non_empty(value)) =>
                {
                    ()
                }
                _ => invalid.push(field.path.clone()),
            }
        }
        invalid
    }

    /// Selects the first valid upstream object with a nonblank string input.
    ///
    /// # Arguments
    /// * `roots` - Successful upstream public payload roots.
    /// * `value_path` - Literal path relative to each selected object.
    ///
    /// # Errors
    /// Returns an error for a malformed collection or no usable input.
    pub fn first_valid_string(&self, roots: &[Value], value_path: &[String]) -> Result<String> {
        validate_path(value_path, false)?;
        let (items, shape_errors) = select_items(roots, &self.items_path);
        ensure!(
            shape_errors.is_empty(),
            "Malformed upstream collection: {shape_errors:?}"
        );
        items
            .into_iter()
            .find_map(|item| {
                if !self.invalid_paths(item).is_empty() {
                    return None;
                }
                literal_value(item, value_path)?
                    .as_str()
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned)
            })
            .ok_or_else(|| anyhow::anyhow!("No valid upstream input at {value_path:?}"))
    }
}

fn non_empty(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(value) => !value.trim().is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
        _ => true,
    }
}

fn lookup<'a>(mut value: &'a Value, path: &[String]) -> Result<Option<&'a Value>, ()> {
    for key in path {
        let Some(object) = value.as_object() else {
            return Err(());
        };
        let Some(child) = object.get(key) else {
            return Ok(None);
        };
        value = child;
    }
    Ok(Some(value))
}

fn literal_value<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    lookup(value, path).ok().flatten()
}

fn select_items<'a>(roots: &'a [Value], path: &[String]) -> (Vec<&'a Value>, Vec<String>) {
    let mut selected: Vec<_> = roots.iter().collect();
    let mut errors = Vec::new();
    for (depth, key) in path.iter().enumerate() {
        let mut next = Vec::new();
        for value in selected {
            if key == "*" {
                if let Some(array) = value.as_array() {
                    next.extend(array);
                } else {
                    errors.push(format!("Expected array at {:?}", &path[..depth]));
                }
            } else if let Some(object) = value.as_object() {
                if let Some(child) = object.get(key) {
                    next.push(child);
                }
            } else {
                errors.push(format!("Expected object at {:?}", &path[..depth]));
            }
        }
        selected = next;
    }
    let items = selected
        .into_iter()
        .flat_map(|value| match value.as_array() {
            Some(array) => array.iter().collect::<Vec<_>>(),
            None => vec![value],
        })
        .collect();
    (items, errors)
}
