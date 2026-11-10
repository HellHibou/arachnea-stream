//! Language identifiers and directional language pairs.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Error returned when a language code does not follow the two-letter contract.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LanguageCodeError {
    /// The value is not exactly two lowercase ASCII letters.
    #[error("language code {value:?} must be exactly two lowercase ASCII letters")]
    Invalid {
        /// Rejected raw value.
        value: String,
    },
}

/// A normalized two-letter lowercase language code.
///
/// The first delivery intentionally supports only the two-letter form. Codes
/// with regional or script variants are rejected rather than silently
/// truncated, because truncation can change provider support. Provider YAML is
/// responsible for mapping a provider code to this contract when an exact
/// equivalent exists.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LanguageCode(String);

impl LanguageCode {
    /// Parses and validates one normalized language code.
    ///
    /// # Arguments
    ///
    /// * `value` - Raw code expected to be exactly two lowercase ASCII letters.
    ///
    /// # Errors
    ///
    /// Returns [`LanguageCodeError::Invalid`] for any other value.
    pub fn parse(value: impl Into<String>) -> Result<Self, LanguageCodeError> {
        let value = value.into();
        let valid = value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_lowercase());
        if valid {
            Ok(Self(value))
        } else {
            Err(LanguageCodeError::Invalid { value })
        }
    }

    /// Returns the code as its lowercase ASCII string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LanguageCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for LanguageCode {
    /// Serializes the code as a JSON string.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for LanguageCode {
    /// Deserializes and validates a language code.
    ///
    /// # Errors
    ///
    /// Returns a deserialization error when the string violates the two-letter
    /// lowercase contract.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

/// One explicitly declared directional translation pair.
///
/// Pairs are directional: declaring `en -> fr` never implies `fr -> en`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SupportedPair {
    /// Source language.
    pub source: LanguageCode,
    /// Target language.
    pub target: LanguageCode,
}

/// A normalized translation request language combination.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TranslationLanguagePair {
    /// Known source language, or `None` when automatic detection is requested.
    pub source: Option<LanguageCode>,
    /// Target language.
    pub target: LanguageCode,
}
