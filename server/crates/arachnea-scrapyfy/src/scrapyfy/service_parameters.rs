//! YAML editor contracts and effective service configuration resolution.

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::{query_helpers::replace_template_placeholders, ScraperQueryCollectionParameter};

/// One labeled value of a parameter selection editor.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParameterOption {
    /// Value supplied to the scraper.
    pub value: String,
    /// Human-readable selection label.
    pub label: String,
}

/// Editing and server-side validation contract declared by a service YAML.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParameterEditor {
    /// Text input validated against a whole-value portable regular expression.
    Regex {
        /// Rust/JavaScript compatible expression, without inline flags.
        pattern: String,
        /// Description displayed when validation fails.
        message: String,
    },
    /// Closed selection: custom values are rejected.
    Select {
        /// Available values and labels, in display order.
        options: Vec<ParameterOption>,
    },
}

impl ParameterEditor {
    /// Validates a raw value against the declared editor contract.
    ///
    /// # Arguments
    /// * `value` - Unprocessed YAML default or administrator-provided value.
    ///
    /// # Errors
    /// Returns an error for an invalid editor or a nonmatching value.
    pub fn validate(&self, value: &str) -> Result<()> {
        match self {
            Self::Regex { pattern, message } => {
                // Reject syntax with differing Rust/JavaScript semantics.
                if pattern.replace("(?:", "(").contains("(?")
                    || pattern.contains("\\A")
                    || pattern.contains("\\z")
                    || pattern.contains("\\p")
                    || pattern.contains("\\P")
                    || pattern.contains("&&")
                    || pattern.contains("--")
                {
                    bail!("Parameter patterns must use portable Rust/JavaScript regex syntax");
                }
                let regex = Regex::new(&format!("^(?:{pattern})$"))
                    .context("Invalid parameter validation pattern")?;
                if message.trim().is_empty() {
                    bail!("Parameter validation message cannot be empty");
                }
                if !regex
                    .find(value)
                    .is_some_and(|m| m.start() == 0 && m.end() == value.len())
                {
                    bail!("{message}");
                }
            }
            Self::Select { options } => {
                let mut values = HashSet::new();
                if options.is_empty() {
                    bail!("Parameter selection must declare at least one option");
                }
                for option in options {
                    if option.label.trim().is_empty() || !values.insert(&option.value) {
                        bail!("Parameter options must have labels and unique values");
                    }
                }
                if !options.iter().any(|option| option.value == value) {
                    bail!("Value is not a declared parameter option");
                }
            }
        }
        Ok(())
    }
}

/// Validates YAML-owned and editable definitions without resolving runtime placeholders.
///
/// # Arguments
/// * `constants` - Values that cannot be overridden outside the YAML.
/// * `parameters` - Editable definitions with their default values.
///
/// # Errors
/// Returns an error for duplicate/reserved names, missing editor labels, or invalid defaults.
pub fn validate_definitions(
    constants: &[ScraperQueryCollectionParameter],
    parameters: &[ScraperQueryCollectionParameter],
) -> Result<()> {
    let mut names = HashSet::new();
    for parameter in constants.iter().chain(parameters) {
        let name = &parameter.name;
        if name.trim().is_empty()
            || name != name.trim()
            || (name.starts_with("service_") || name.starts_with('@'))
            || !names.insert(name)
        {
            bail!("Invalid, duplicate or reserved service configuration name: {name}");
        }
    }
    for constant in constants {
        if constant.editor.is_some() {
            bail!("Constant {} cannot declare an editor", constant.name);
        }
    }
    for parameter in parameters {
        if parameter
            .label
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            bail!(
                "Editable parameter {} requires a label; migrate legacy parameters to constants",
                parameter.name
            );
        }
        validate_parameter(parameter, &parameter.value)?;
    }
    Ok(())
}

/// Validates a parameter value, including absolute HTTP URL semantics for `base_url`.
///
/// # Arguments
/// * `parameter` - Editable definition.
/// * `value` - Raw value before actions.
///
/// # Errors
/// Returns an error naming the parameter when validation fails.
pub fn validate_parameter(parameter: &ScraperQueryCollectionParameter, value: &str) -> Result<()> {
    let editor = parameter
        .editor
        .as_ref()
        .with_context(|| format!("Parameter {} requires an editor", parameter.name))?;
    editor
        .validate(value)
        .with_context(|| format!("Invalid parameter {}", parameter.name))?;
    if parameter.name == "base_url" {
        let url = url::Url::parse(value).context("Invalid base_url")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || value.chars().any(char::is_whitespace)
            || value.ends_with('/')
        {
            bail!("base_url must be an absolute HTTP(S) URL without credentials, query, fragment or trailing slash");
        }
    }
    Ok(())
}

/// Applies only declared administrator overrides, preserving YAML declaration order and actions.
///
/// # Arguments
/// * `parameters` - Editable definitions whose defaults are replaced in place.
/// * `overrides` - Persisted or submitted raw values; orphaned entries are ignored.
///
/// # Errors
/// Returns an error for a persisted value incompatible with its current definition.
pub fn apply_overrides(
    parameters: &mut [ScraperQueryCollectionParameter],
    overrides: &HashMap<String, String>,
) -> Result<()> {
    for parameter in parameters {
        if let Some(value) = overrides.get(&parameter.name) {
            validate_parameter(parameter, value)?;
            parameter.value = value.clone();
        }
    }
    Ok(())
}

/// Resolves cross-section dependencies, then applies each entry's actions once.
///
/// # Arguments
/// * `parameters` - Combined constants and editable parameters.
///
/// # Errors
/// Returns an error for cycles, empty names or duplicates. Unknown runtime placeholders remain deferred.
pub fn resolve_values(
    parameters: &[ScraperQueryCollectionParameter],
) -> Result<HashMap<String, String>> {
    let mut resolved = HashMap::new();
    let mut visiting = HashSet::new();
    let mut definitions = HashMap::new();
    for parameter in parameters {
        if parameter.name.trim().is_empty()
            || definitions
                .insert(parameter.name.as_str(), parameter)
                .is_some()
        {
            bail!(
                "Empty or duplicate collection parameter: {}",
                parameter.name
            );
        }
    }
    let placeholders = Regex::new(r"\{([A-Za-z_][A-Za-z0-9_]*)\}")?;
    for parameter in parameters {
        resolve_one(
            &parameter.name,
            &definitions,
            &placeholders,
            &mut visiting,
            &mut resolved,
        )?;
    }
    Ok(resolved)
}

fn resolve_one(
    name: &str,
    definitions: &HashMap<&str, &ScraperQueryCollectionParameter>,
    placeholders: &Regex,
    visiting: &mut HashSet<String>,
    resolved: &mut HashMap<String, String>,
) -> Result<()> {
    if resolved.contains_key(name) {
        return Ok(());
    }
    if !visiting.insert(name.to_string()) {
        bail!("Cyclic service configuration dependency at {name}");
    }
    let parameter = definitions[name];
    let action_values = serde_yaml::to_value(&parameter.actions)?;
    let mut inputs = vec![parameter.value.as_str()];
    collect_strings(&action_values, &mut inputs);
    for input in inputs {
        for capture in placeholders.captures_iter(input) {
            let dependency = &capture[1];
            if definitions.contains_key(dependency) {
                resolve_one(dependency, definitions, placeholders, visiting, resolved)?;
            }
        }
    }
    let mut values = vec![replace_template_placeholders(&parameter.value, resolved).0];
    for action in &parameter.actions {
        values = action.apply(&None, values, resolved, "", None, None);
    }
    resolved.insert(
        name.to_string(),
        values.into_iter().next().unwrap_or_default(),
    );
    visiting.remove(name);
    Ok(())
}

fn collect_strings<'a>(value: &'a serde_yaml::Value, values: &mut Vec<&'a str>) {
    match value {
        serde_yaml::Value::String(value) => values.push(value),
        serde_yaml::Value::Sequence(array) => array
            .iter()
            .for_each(|value| collect_strings(value, values)),
        serde_yaml::Value::Mapping(object) => object
            .values()
            .for_each(|value| collect_strings(value, values)),
        _ => {}
    }
}
