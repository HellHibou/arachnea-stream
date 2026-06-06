use anyhow::{bail, Result};
use serde_yaml::{Mapping as YamlMapping, Value as YamlValue};

/// Applies the `map` scraper action, rewriting each value through the lookup
/// table and falling back to `default` when no entry matches.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline.
/// * `map` - YAML mapping used to normalize raw values.
/// * `default` - Optional fallback applied when an input has no mapping entry.
///
/// # Returns
///
/// A new value list where each entry is either the mapped value, the default
/// value (with `{}` replaced), or the original value when no entry or default
/// matched.
pub(super) fn apply(
    texts: Vec<String>,
    map: &YamlMapping,
    default: &Option<YamlValue>,
) -> Vec<String> {
    if texts.is_empty() {
        return match find_map_value(map, None, "") {
            Some(Some(mapped)) => vec![mapped],
            Some(None) | None => Vec::new(),
        };
    }

    texts
        .into_iter()
        .filter_map(
            |value| match find_map_value(map, Some(&value), &value) {
                Some(Some(mapped)) => Some(mapped),
                Some(None) => None,
                None => match default {
                    Some(template) => yaml_scalar_to_optional_string(template)
                        .unwrap_or(Some(value.clone()))
                        .map(|template| template.replace("{}", &value)),
                    None => Some(value),
                },
            },
        )
        .collect()
}

/// Validates the configuration of a `map` action, ensuring every key, value
/// and default entry is a YAML scalar (string, bool, number, or `null`).
///
/// # Arguments
///
/// * `name` - Field or sub-query name that owns the action.
/// * `owner` - Human-readable owner kind used in validation errors.
/// * `argument` - YAML mapping declared by the action.
/// * `default` - Optional default YAML value declared by the action.
///
/// # Errors
///
/// Returns an error when any key, value, or default is not a YAML scalar
/// (sequence and mapping nodes are rejected).
pub(super) fn validate(
    name: &str,
    owner: &str,
    argument: &YamlMapping,
    default: &Option<YamlValue>,
) -> Result<()> {
    for (key, value) in argument {
        if yaml_scalar_to_optional_string(key).is_none() {
            bail!(
                "map for {} {} only supports scalar keys (string, bool, number, or null)",
                owner,
                name
            );
        }

        if yaml_scalar_to_optional_string(value).is_none() {
            bail!(
                "map for {} {} only supports scalar values (string, bool, number, or null)",
                owner,
                name
            );
        }
    }

    if let Some(default) = default {
        if yaml_scalar_to_optional_string(default).is_none() {
            bail!(
                "map for {} {} only supports scalar default values (string, bool, number, or null)",
                owner,
                name
            );
        }
    }

    Ok(())
}

/// Looks up `expected_key` in `map` and returns the mapped value when found.
///
/// # Arguments
///
/// * `map` - YAML mapping searched for the entry.
/// * `expected_key` - Key to look up as a string. `None` matches the
///   `YamlValue::Null` key (used to evaluate the default branch).
/// * `original_value` - Original input value used to replace `{}` in the
///   mapped string.
///
/// # Returns
///
/// `Some(Some(mapped))` when a non-null match is found, `Some(None)` when the
/// match maps to YAML `null` (entry should be dropped), or `None` when the key
/// is absent from the mapping.
fn find_map_value(
    map: &YamlMapping,
    expected_key: Option<&str>,
    original_value: &str,
) -> Option<Option<String>> {
    map.iter().find_map(|(key, value)| {
        let normalized_key = yaml_scalar_to_optional_string(key)?;
        if normalized_key.as_deref() != expected_key {
            return None;
        }

        yaml_scalar_to_optional_string(value)
            .map(|mapped| mapped.map(|mapped| mapped.replace("{}", original_value)))
    })
}

/// Converts a YAML scalar into a `String` representation, or returns `None` for
/// non-scalar nodes (sequences and mappings).
///
/// # Arguments
///
/// * `value` - YAML value to inspect.
///
/// # Returns
///
/// `Some(Some(s))` for scalar values, `Some(None)` for explicit YAML `null`,
/// and `None` for sequence and mapping nodes.
pub(super) fn yaml_scalar_to_optional_string(value: &YamlValue) -> Option<Option<String>> {
    match value {
        YamlValue::Null => Some(None),
        YamlValue::Bool(value) => Some(Some(value.to_string())),
        YamlValue::Number(value) => Some(Some(value.to_string())),
        YamlValue::String(value) => Some(Some(value.clone())),
        YamlValue::Tagged(tagged) => yaml_scalar_to_optional_string(&tagged.value),
        YamlValue::Sequence(_) | YamlValue::Mapping(_) => None,
    }
}
