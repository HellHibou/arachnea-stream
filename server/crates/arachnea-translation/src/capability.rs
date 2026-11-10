//! Provider capabilities and explicit size limits.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Unit used by a provider for size accounting and declared limits.
///
/// Unknown units are preserved so a service YAML can describe its real unit
/// without pretending it is bytes or code points. Callers applying a local
/// bound must treat an unknown unit conservatively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SizeUnit {
    /// UTF-8 bytes.
    Utf8Bytes,
    /// Unicode scalar values (code points).
    UnicodeCodePoints,
    /// Any other provider unit, preserved verbatim.
    Other(String),
}

impl SizeUnit {
    /// Returns the normalized spelling used in metadata.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Utf8Bytes => "utf8_bytes",
            Self::UnicodeCodePoints => "unicode_code_points",
            Self::Other(value) => value,
        }
    }

    /// Measures one text under this unit.
    ///
    /// Unknown units are measured as UTF-8 bytes, which is the conservative
    /// choice for text made of ASCII and multi-byte characters.
    ///
    /// # Arguments
    ///
    /// * `text` - Text whose size must be measured.
    pub fn measure(&self, text: &str) -> u64 {
        match self {
            Self::UnicodeCodePoints => text.chars().count() as u64,
            Self::Utf8Bytes | Self::Other(_) => text.len() as u64,
        }
    }
}

impl Serialize for SizeUnit {
    /// Serializes the unit as its metadata string.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SizeUnit {
    /// Deserializes a metadata unit string.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "utf8_bytes" => Self::Utf8Bytes,
            "unicode_code_points" => Self::UnicodeCodePoints,
            _ => Self::Other(value),
        })
    }
}

/// Optional source-language detection behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDetection {
    /// Whether translation accepts an omitted or automatic source language.
    pub automatic: bool,
    /// Whether the translation response exposes the detected language.
    pub returns_detected_language: bool,
    /// Optional dedicated Scrapyfy detection query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

/// Declared text translation capability and per-request bounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextTranslationCapability {
    /// Scrapyfy query executing normalized text translation.
    pub query: String,
    /// Optional query used when the source language must be detected inline.
    ///
    /// A service that cannot express "automatic" through
    /// `translation_source_language` declares a second query omitting the
    /// source parameter. The bridge prefers this query for a source-less
    /// request; known sources always use [`Self::query`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_query: Option<String>,
    /// Whether one request accepts several blocks.
    #[serde(default)]
    pub accepts_multiple_blocks: bool,
    /// Unit used by the numeric limits below.
    pub size_unit: SizeUnit,
    /// Maximum blocks per request, when declared by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_blocks_per_request: Option<u64>,
    /// Maximum units in one block, when declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_units_per_block: Option<u64>,
    /// Maximum cumulative units in one request, when declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_units_per_request: Option<u64>,
    /// Maximum serialized request bytes, when declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_request_bytes: Option<u64>,
    /// Whether the provider accepts neighbouring context.
    #[serde(default)]
    pub context_support: bool,
    /// Whether markup is preserved by the provider.
    #[serde(default)]
    pub markup_support: bool,
    /// Whether translated rows are returned in the same order as the request.
    ///
    /// When true, provider rows may omit block identifiers and are matched to
    /// submitted blocks by position. Cardinality is still validated.
    #[serde(default)]
    pub preserves_request_order: bool,
}

/// How a provider returns a translated document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlResultMode {
    /// The translated document is returned inline in the query response.
    InlineDocument,
}

/// Declared URL document translation capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UrlTranslationCapability {
    /// Scrapyfy query executing URL translation.
    pub query: String,
    /// Media types the provider accepts for the source document.
    pub accepted_formats: Vec<String>,
    /// Maximum source document bytes, when declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_document_bytes: Option<u64>,
    /// Maximum source document units, when declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_document_units: Option<u64>,
    /// Normalized result mode.
    pub result_mode: UrlResultMode,
}

impl UrlTranslationCapability {
    /// Returns whether the capability accepts one media type.
    ///
    /// # Arguments
    ///
    /// * `media_type` - Source media type, for example `text/vtt`.
    pub fn accepts_format(&self, media_type: &str) -> bool {
        self.accepted_formats
            .iter()
            .any(|accepted| accepted.eq_ignore_ascii_case(media_type))
    }
}
