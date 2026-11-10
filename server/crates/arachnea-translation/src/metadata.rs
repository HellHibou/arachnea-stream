//! Validated service metadata, languages, directions and freshness.

use crate::capability::{SourceDetection, TextTranslationCapability, UrlTranslationCapability};
use crate::error::TranslationError;
use crate::language::{LanguageCode, SupportedPair};
use crate::TRANSLATION_METADATA_SCHEMA_VERSION;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Validated metadata returned by the `get_translation_metadata` query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationMetadata {
    /// Normalized metadata schema version.
    pub schema_version: u32,
    /// Required common language list.
    pub languages: Vec<LanguageCode>,
    /// Optional source languages; defaults to `languages` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_languages: Option<Vec<LanguageCode>>,
    /// Optional target languages; defaults to `languages` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_languages: Option<Vec<LanguageCode>>,
    /// Optional directional pair restriction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_pairs: Option<Vec<SupportedPair>>,
    /// Optional source detection behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_detection: Option<SourceDetection>,
    /// Optional text translation capability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_translation: Option<TextTranslationCapability>,
    /// Optional URL document translation capability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url_translation: Option<UrlTranslationCapability>,
}

impl TranslationMetadata {
    /// Parses, deserializes and validates one metadata value.
    ///
    /// # Arguments
    ///
    /// * `value` - Metadata JSON extracted from the Scrapyfy query result.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::MetadataUnavailable`] when the value does
    /// not match the normalized contract.
    pub fn from_value(value: &serde_json::Value) -> Result<Self, TranslationError> {
        let metadata: Self = serde_json::from_value(value.clone()).map_err(|error| {
            TranslationError::MetadataUnavailable {
                detail: format!("invalid translation metadata: {error}"),
            }
        })?;
        metadata.validate()?;
        Ok(metadata)
    }

    /// Validates schema version, language lists and declared limits.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::InvalidMetadata`] when the metadata is
    /// inconsistent or a declared bound is unusable.
    pub fn validate(&self) -> Result<(), TranslationError> {
        let invalid = |detail: String| TranslationError::InvalidMetadata {
            service_id: "unknown".to_string(),
            detail,
        };
        if self.schema_version != TRANSLATION_METADATA_SCHEMA_VERSION {
            return Err(invalid(format!(
                "unsupported translation metadata schema version {}",
                self.schema_version
            )));
        }
        if self.languages.is_empty() {
            return Err(invalid(
                "translation metadata must declare at least one language".to_string(),
            ));
        }
        reject_duplicates(&self.languages)
            .map_err(|detail| invalid(format!("languages: {detail}")))?;

        let sources = self.effective_source_languages();
        let targets = self.effective_target_languages();
        reject_duplicates(sources)
            .map_err(|detail| invalid(format!("source_languages: {detail}")))?;
        reject_duplicates(targets)
            .map_err(|detail| invalid(format!("target_languages: {detail}")))?;
        for language in sources.iter().chain(targets.iter()) {
            if !self.languages.contains(language) {
                return Err(invalid(format!(
                    "language {language} is not listed in languages"
                )));
            }
        }

        if let Some(pairs) = &self.supported_pairs {
            let mut seen = HashSet::new();
            for pair in pairs {
                if pair.source == pair.target {
                    return Err(invalid(format!(
                        "supported pair {} -> {} translates to the same language",
                        pair.source, pair.target
                    )));
                }
                if !sources.contains(&pair.source) || !targets.contains(&pair.target) {
                    return Err(invalid(format!(
                        "supported pair {} -> {} is outside declared language lists",
                        pair.source, pair.target
                    )));
                }
                if !seen.insert((pair.source.clone(), pair.target.clone())) {
                    return Err(invalid(format!(
                        "duplicate supported pair {} -> {}",
                        pair.source, pair.target
                    )));
                }
            }
        }

        if let Some(detection) = &self.source_detection {
            if detection.query.as_deref().is_some_and(str::is_empty) {
                return Err(invalid(
                    "source_detection query must not be empty".to_string(),
                ));
            }
        }
        if let Some(text) = &self.text_translation {
            validate_text_capability(text)
                .map_err(|detail| invalid(format!("text_translation: {detail}")))?;
        }
        if let Some(url) = &self.url_translation {
            if url.query.trim().is_empty() {
                return Err(invalid(
                    "url_translation query must not be empty".to_string(),
                ));
            }
            if url.accepted_formats.is_empty() {
                return Err(invalid(
                    "url_translation must declare at least one accepted format".to_string(),
                ));
            }
            check_positive(url.max_document_bytes, "max_document_bytes")
                .map_err(|detail| invalid(format!("url_translation: {detail}")))?;
            check_positive(url.max_document_units, "max_document_units")
                .map_err(|detail| invalid(format!("url_translation: {detail}")))?;
        }
        Ok(())
    }

    /// Returns the effective source language list.
    pub fn effective_source_languages(&self) -> &[LanguageCode] {
        self.source_languages
            .as_deref()
            .unwrap_or(self.languages.as_slice())
    }

    /// Returns the effective target language list.
    pub fn effective_target_languages(&self) -> &[LanguageCode] {
        self.target_languages
            .as_deref()
            .unwrap_or(self.languages.as_slice())
    }

    /// Returns the available directional pairs.
    ///
    /// When `supported_pairs` is absent, every source/target combination is
    /// available except translation into the same language. An explicitly
    /// empty pair list therefore yields no pair at all.
    pub fn available_pairs(&self) -> Vec<SupportedPair> {
        if let Some(pairs) = &self.supported_pairs {
            return pairs.clone();
        }
        let mut pairs = Vec::new();
        for source in self.effective_source_languages() {
            for target in self.effective_target_languages() {
                if source != target {
                    pairs.push(SupportedPair {
                        source: source.clone(),
                        target: target.clone(),
                    });
                }
            }
        }
        pairs
    }

    /// Returns whether the common language list contains one code.
    ///
    /// # Arguments
    ///
    /// * `language` - Normalized code to test.
    pub fn supports_language(&self, language: &LanguageCode) -> bool {
        self.languages.contains(language)
    }

    /// Returns whether a directional pair is declared as available.
    ///
    /// # Arguments
    ///
    /// * `source` - Known source language, or `None` for automatic detection.
    /// * `target` - Target language.
    ///
    /// With an unknown source, a pair is considered potentially available only
    /// when the service can detect the source. Restricted pair lists still
    /// require a prior detection to establish eligibility.
    pub fn supports_pair(&self, source: Option<&LanguageCode>, target: &LanguageCode) -> bool {
        if !self.effective_target_languages().contains(target) {
            return false;
        }
        match source {
            Some(source) => {
                if !self.effective_source_languages().contains(source) {
                    return false;
                }
                if source == target {
                    return false;
                }
                match &self.supported_pairs {
                    Some(pairs) => pairs
                        .iter()
                        .any(|pair| pair.source == *source && pair.target == *target),
                    None => true,
                }
            }
            None => {
                if !self.detection_available() {
                    return false;
                }
                match &self.supported_pairs {
                    Some(pairs) => pairs.iter().any(|pair| pair.target == *target),
                    None => true,
                }
            }
        }
    }

    /// Returns whether source detection is declared.
    pub fn detection_available(&self) -> bool {
        self.source_detection
            .as_ref()
            .is_some_and(|detection| detection.automatic || detection.query.as_deref().is_some())
    }

    /// Returns whether detection can establish a source for restricted pairs.
    pub fn detection_can_establish_source(&self) -> bool {
        self.source_detection.as_ref().is_some_and(|detection| {
            detection.returns_detected_language || detection.query.as_deref().is_some()
        })
    }
}

/// Freshness window attached to loaded metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataFreshness {
    /// Load time as Unix seconds.
    pub loaded_at_unix_seconds: u64,
    /// Maximum accepted age in seconds.
    pub max_age_seconds: u64,
}

impl MetadataFreshness {
    /// Returns whether the metadata is still fresh at one instant.
    ///
    /// # Arguments
    ///
    /// * `now_unix_seconds` - Current Unix time in seconds.
    pub fn is_fresh(&self, now_unix_seconds: u64) -> bool {
        now_unix_seconds.saturating_sub(self.loaded_at_unix_seconds) <= self.max_age_seconds
    }
}

/// Rejects duplicated language codes while preserving order.
///
/// # Arguments
///
/// * `languages` - Languages to inspect.
///
/// # Errors
///
/// Returns a human-readable detail for the first duplicate.
fn reject_duplicates(languages: &[LanguageCode]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for language in languages {
        if !seen.insert(language) {
            return Err(format!("duplicate language {language}"));
        }
    }
    Ok(())
}

/// Validates one declared text capability.
///
/// # Arguments
///
/// * `text` - Capability to validate.
///
/// # Errors
///
/// Returns a human-readable detail for a blank query or a non-positive bound.
fn validate_text_capability(text: &TextTranslationCapability) -> Result<(), String> {
    if text.query.trim().is_empty() {
        return Err("query must not be empty".to_string());
    }
    check_positive(text.max_blocks_per_request, "max_blocks_per_request")?;
    check_positive(text.max_units_per_block, "max_units_per_block")?;
    check_positive(text.max_units_per_request, "max_units_per_request")?;
    check_positive(text.max_request_bytes, "max_request_bytes")?;
    Ok(())
}

/// Rejects an explicitly zero bound; an absent bound means undeclared.
///
/// # Arguments
///
/// * `value` - Optional declared bound.
/// * `name` - Field name used in the error detail.
///
/// # Errors
///
/// Returns a human-readable detail when the bound is zero.
fn check_positive(value: Option<u64>, name: &str) -> Result<(), String> {
    match value {
        Some(0) => Err(format!("{name} must be greater than zero")),
        _ => Ok(()),
    }
}
