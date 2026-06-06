use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Typed parameter supplied by a library caller or an inbound proxy protocol.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ClientParameter {
    /// A string parameter such as a country code or application name.
    String(String),
    /// A boolean parameter.
    Bool(bool),
    /// A signed integer parameter.
    Integer(i64),
}

/// Optional client context used by routing policies and extensions.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClientContext {
    /// Arbitrary caller-provided parameters after validation.
    pub parameters: BTreeMap<String, ClientParameter>,
}

impl ClientContext {
    /// Creates an empty client context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces a parameter.
    ///
    /// # Parameters
    ///
    /// - `key`: Parameter key.
    /// - `value`: Parameter value.
    pub fn insert(&mut self, key: impl Into<String>, value: ClientParameter) {
        self.parameters.insert(key.into(), value);
    }

    /// Returns a parameter by key.
    ///
    /// # Parameters
    ///
    /// - `key`: Parameter key to look up.
    ///
    /// # Returns
    ///
    /// Parameter value when present.
    pub fn get(&self, key: &str) -> Option<&ClientParameter> {
        self.parameters.get(key)
    }

    /// Returns a string parameter by key.
    ///
    /// # Parameters
    ///
    /// - `key`: Parameter key to look up.
    ///
    /// # Returns
    ///
    /// String parameter value when present and typed as a string.
    pub fn get_string(&self, key: &str) -> Option<&str> {
        match self.parameters.get(key) {
            Some(ClientParameter::String(value)) => Some(value),
            _ => None,
        }
    }
}
