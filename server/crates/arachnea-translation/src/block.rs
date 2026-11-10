//! Abstract text blocks, translation requests and normalized results.

use crate::error::TranslationError;
use crate::language::LanguageCode;
use serde::{Deserialize, Serialize};

/// Opaque identifier of one abstract translation block.
///
/// The identifier has no subtitle or cue semantics. The `arachnea-stream`
/// subtitle module owns the mapping from a cue or segment to these block
/// identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockId(String);

impl BlockId {
    /// Creates a validated block identifier.
    ///
    /// # Arguments
    ///
    /// * `value` - Nonblank opaque identifier.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::InvalidBlockId`] for a blank value.
    pub fn new(value: impl Into<String>) -> Result<Self, TranslationError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(TranslationError::InvalidBlockId { value });
        }
        Ok(Self(value))
    }

    /// Returns the identifier as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One text block submitted to a translation service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextBlock {
    /// Caller-owned block identifier.
    pub id: BlockId,
    /// Text to translate.
    pub text: String,
}

impl TextBlock {
    /// Creates a block, rejecting an empty text.
    ///
    /// # Arguments
    ///
    /// * `id` - Validated block identifier.
    /// * `text` - Nonempty text to translate.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::InvalidRequest`] when the text is blank.
    pub fn new(id: BlockId, text: impl Into<String>) -> Result<Self, TranslationError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(TranslationError::InvalidRequest {
                detail: format!("block {} has no translatable text", id.as_str()),
            });
        }
        Ok(Self { id, text })
    }
}

/// Optional language options applied to one translation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationOptions {
    /// Optional neighbouring context when the chosen provider supports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// Whether the caller requires markup preservation.
    #[serde(default)]
    pub preserve_markup: bool,
}

/// Normalized text translation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextTranslationRequest {
    /// Known source language, or `None` to request automatic detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_language: Option<LanguageCode>,
    /// Target language.
    pub target_language: LanguageCode,
    /// Blocks to translate.
    pub blocks: Vec<TextBlock>,
    /// Optional language options.
    #[serde(default)]
    pub options: TranslationOptions,
}

/// One normalized translated block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslatedBlock {
    /// Identifier matching an input block.
    pub id: BlockId,
    /// Translated text.
    pub text: String,
}

/// Normalized result of a text translation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslatedTexts {
    /// Translated blocks in input order.
    pub blocks: Vec<TranslatedBlock>,
    /// Effective source language when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_language: Option<LanguageCode>,
    /// Source language detected by the provider, when returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected_source_language: Option<LanguageCode>,
}

/// Normalized URL document translation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UrlTranslationRequest {
    /// Source document URL, retrievable by the provider infrastructure.
    pub source_url: String,
    /// Known source language, or `None` to request automatic detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_language: Option<LanguageCode>,
    /// Target language.
    pub target_language: LanguageCode,
    /// Declared source media type, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// Optional language options.
    #[serde(default)]
    pub options: TranslationOptions,
}

/// Normalized translated document returned by a URL capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedDocument {
    /// Complete translated document bytes.
    pub bytes: Vec<u8>,
    /// Media type declared by the provider response.
    pub content_type: String,
    /// Source language detected by the provider, when returned.
    pub detected_source_language: Option<LanguageCode>,
}
