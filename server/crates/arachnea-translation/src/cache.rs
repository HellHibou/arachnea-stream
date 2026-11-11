//! Access scope and provider-independent translation revision identity.

use crate::error::TranslationError;
use crate::language::LanguageCode;
use serde::{Deserialize, Serialize};

/// Opaque access scope isolating private translations.
///
/// This identifier must never be a credential or a signed URL.
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

/// Translation inputs needed to revalidate a browser-held representation.
///
/// Revisions and fingerprints must not contain credentials, URLs, option
/// values or other sensitive material. `context_fingerprint` covers endpoint,
/// model and language options that can change the translated output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranslationRevision {
    /// Requested source language, or `None` for automatic detection.
    pub source_language: Option<LanguageCode>,
    /// Requested target language.
    pub target_language: LanguageCode,
    /// Identifier of the service that produced the translation.
    pub service_id: String,
    /// Hash of the service YAML bytes.
    pub service_yaml_hash: String,
    /// Revision covering effective service configuration.
    pub service_configuration_revision: String,
    /// Revision of the generic translation pipeline.
    pub pipeline_revision: String,
    /// Fingerprint of other output-affecting options and endpoint identity.
    pub context_fingerprint: String,
}
