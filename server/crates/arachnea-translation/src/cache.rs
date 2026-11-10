//! Generic result cache identity.

use crate::error::TranslationError;
use crate::language::LanguageCode;
use crate::{
    TRANSLATION_CACHE_KEY_PAIR_SEPARATOR, TRANSLATION_CACHE_KEY_SEPARATOR,
    TRANSLATION_CACHE_KEY_VERSION,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Opaque access scope isolating private translations.
///
/// The scope is a stable identifier, never a credential or a signed URL. A
/// result produced for one scope must not be returned for another.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccessScope(String);

impl AccessScope {
    /// Creates a validated access scope.
    ///
    /// # Arguments
    ///
    /// * `value` - Nonblank opaque scope identifier.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::InvalidRequest`] for a blank value.
    pub fn new(value: impl Into<String>) -> Result<Self, TranslationError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(TranslationError::InvalidRequest {
                detail: "access scope must not be blank".to_string(),
            });
        }
        Ok(Self(value))
    }

    /// Returns the opaque scope as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable fingerprint of the source document content.
///
/// The value is an opaque digest computed by the document layer. It must not
/// be derived from a signed URL alone, because that URL can be renewed while
/// the content stays identical.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceFingerprint(String);

impl SourceFingerprint {
    /// Creates a validated source fingerprint.
    ///
    /// # Arguments
    ///
    /// * `value` - Nonblank opaque content digest.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::InvalidRequest`] for a blank value.
    pub fn new(value: impl Into<String>) -> Result<Self, TranslationError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(TranslationError::InvalidRequest {
                detail: "source fingerprint must not be blank".to_string(),
            });
        }
        Ok(Self(value))
    }

    /// Returns the fingerprint as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Complete identity of one generic translation result.
///
/// The identity deliberately excludes credentials. Optional language options
/// are sorted in a [`BTreeMap`] so the produced key does not depend on map
/// iteration order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationCacheIdentity {
    /// Content fingerprint of the source document.
    pub source_fingerprint: SourceFingerprint,
    /// Effective source language, or `None` when detection stayed unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_language: Option<LanguageCode>,
    /// Target language.
    pub target_language: LanguageCode,
    /// Service identifier that actually produced the result.
    pub service_id: String,
    /// Provider endpoint or model identity when relevant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_or_model: Option<String>,
    /// Language options influencing the translation.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub language_options: BTreeMap<String, String>,
    /// Revision of the service YAML, actions and parameters influencing output.
    pub service_configuration_revision: String,
    /// Revision of the generic translation pipeline.
    pub pipeline_revision: String,
    /// Access scope isolating private content.
    pub access_scope: AccessScope,
}

impl TranslationCacheIdentity {
    /// Builds the persisted cache key for this identity.
    pub fn cache_key(&self) -> TranslationCacheKey {
        let mut key = String::from(TRANSLATION_CACHE_KEY_VERSION);
        let mut push = |name: &str, value: &str| {
            key.push(TRANSLATION_CACHE_KEY_SEPARATOR);
            key.push_str(&escape(name));
            key.push(TRANSLATION_CACHE_KEY_PAIR_SEPARATOR);
            key.push_str(&escape(value));
        };
        push("source", self.source_fingerprint.as_str());
        push(
            "source_language",
            self.source_language
                .as_ref()
                .map(LanguageCode::as_str)
                .unwrap_or("auto"),
        );
        push("target_language", self.target_language.as_str());
        push("service", &self.service_id);
        push("endpoint", self.endpoint_or_model.as_deref().unwrap_or(""));
        for (name, value) in &self.language_options {
            push(name, value);
        }
        push("configuration", &self.service_configuration_revision);
        push("pipeline", &self.pipeline_revision);
        push("scope", self.access_scope.as_str());
        TranslationCacheKey(key)
    }
}

/// Persisted translation cache key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TranslationCacheKey(String);

impl TranslationCacheKey {
    /// Returns the key as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the key and returns its string.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for TranslationCacheKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Escapes separators so distinct identities cannot produce the same key.
///
/// # Arguments
///
/// * `value` - Raw component value.
fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character == '\\' {
            escaped.push_str("\\\\");
        } else if character == TRANSLATION_CACHE_KEY_SEPARATOR
            || character == TRANSLATION_CACHE_KEY_PAIR_SEPARATOR
        {
            escaped.push_str(&format!("\\u{{{:x}}}", character as u32));
        } else {
            escaped.push(character);
        }
    }
    escaped
}
