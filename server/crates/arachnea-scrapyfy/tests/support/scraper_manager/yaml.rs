//! Strict YAML decoding shared by test-only contract loaders.

use anyhow::{ensure, Context, Result};
use serde::de::DeserializeOwned;
use std::io::Read;

/// Decodes YAML without scalar coercion, duplicate keys or non-string map keys.
///
/// # Arguments
/// * `reader` - YAML document containing the test configuration.
///
/// # Errors
/// Returns contextual syntax, representation or target-schema errors.
pub fn read_strict_yaml<T: DeserializeOwned>(reader: impl Read) -> Result<T> {
    let value: serde_yaml::Value =
        serde_yaml::from_reader(reader).context("Invalid YAML document")?;
    validate_keys(&value)?;
    let json =
        serde_json::to_value(value).context("Test YAML must have a JSON-compatible shape")?;
    serde_json::from_value(json).context("Invalid test configuration schema")
}

fn validate_keys(value: &serde_yaml::Value) -> Result<()> {
    match value {
        serde_yaml::Value::Mapping(mapping) => {
            for (key, value) in mapping {
                ensure!(key.is_string(), "Test YAML mapping keys must be strings");
                validate_keys(value)?;
            }
        }
        serde_yaml::Value::Sequence(values) => {
            for value in values {
                validate_keys(value)?;
            }
        }
        serde_yaml::Value::Tagged(_) => {
            anyhow::bail!("YAML tags are not supported in test contracts")
        }
        _ => (),
    }
    Ok(())
}
