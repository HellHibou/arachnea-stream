//! Subtitle formats, source descriptors and document results.

use crate::error::SubtitleTranslationError;
use crate::{DEFAULT_SUBTITLE_CACHE_CONTROL, SRT_MEDIA_TYPE, WEBVTT_MEDIA_TYPE};
use arachnea_translation::LanguageCode;
use serde::{Deserialize, Serialize};

/// Subtitle document format handled by the document layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtitleFormat {
    /// WebVTT, the only format promised by the initial delivery.
    WebVtt,
    /// SubRip, reserved for an explicit later normalization extension.
    Srt,
}

impl SubtitleFormat {
    /// Returns the canonical media type.
    pub fn media_type(self) -> &'static str {
        match self {
            Self::WebVtt => WEBVTT_MEDIA_TYPE,
            Self::Srt => SRT_MEDIA_TYPE,
        }
    }

    /// Returns the usual filename extension without a leading dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::WebVtt => "vtt",
            Self::Srt => "srt",
        }
    }

    /// Returns whether the initial delivery accepts this format directly.
    ///
    /// SRT is only reachable through a future explicit normalization step;
    /// existing resolvers must not expose raw SRT to the player as WebVTT.
    pub fn is_initial_delivery_supported(self) -> bool {
        matches!(self, Self::WebVtt)
    }

    /// Detects a format from a media type.
    ///
    /// # Arguments
    ///
    /// * `content_type` - Media type, with optional parameters.
    pub fn from_content_type(content_type: &str) -> Option<Self> {
        let media_type = content_type
            .split(';')
            .next()
            .unwrap_or(content_type)
            .trim();
        if media_type.eq_ignore_ascii_case(WEBVTT_MEDIA_TYPE) {
            Some(Self::WebVtt)
        } else if media_type.eq_ignore_ascii_case(SRT_MEDIA_TYPE) {
            Some(Self::Srt)
        } else {
            None
        }
    }
}

/// Opaque fingerprint of the source document content.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentFingerprint(String);

impl DocumentFingerprint {
    /// Creates a validated fingerprint.
    ///
    /// # Arguments
    ///
    /// * `value` - Nonblank opaque content digest.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::InvalidDescriptor`] for a blank value.
    pub fn new(value: impl Into<String>) -> Result<Self, SubtitleTranslationError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(SubtitleTranslationError::InvalidDescriptor {
                detail: "document fingerprint must not be blank".to_string(),
            });
        }
        Ok(Self(value))
    }

    /// Returns the fingerprint as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Persistent source track identity and retrieval metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleSourceDescriptor {
    /// Stable track identifier supplied by the resolver when it knows one.
    pub track_id: String,
    /// Source URL or recognized application proxy link.
    pub url: String,
    /// Language declared by the resolver when reliable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_language: Option<LanguageCode>,
    /// Source subtitle format.
    pub format: SubtitleFormat,
}

impl SubtitleSourceDescriptor {
    /// Validates the source descriptor.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::InvalidDescriptor`] for a blank
    /// track identifier or URL.
    pub fn validate(&self) -> Result<(), SubtitleTranslationError> {
        if self.track_id.trim().is_empty() {
            return Err(SubtitleTranslationError::InvalidDescriptor {
                detail: "source track identifier must not be blank".to_string(),
            });
        }
        if self.url.trim().is_empty() {
            return Err(SubtitleTranslationError::InvalidDescriptor {
                detail: "source track URL must not be blank".to_string(),
            });
        }
        Ok(())
    }
}

/// Source document returned by an authorized retrieval port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievedSubtitle {
    /// Complete source bytes.
    pub bytes: Vec<u8>,
    /// Response media type when available.
    pub content_type: Option<String>,
    /// Upstream validator, distinct from the final document ETag.
    pub remote_etag: Option<String>,
    /// Final URL after authorized redirects, when known.
    pub final_url: Option<String>,
}

/// Measured document characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleDocumentStats {
    /// Decompressed source document size in bytes.
    pub document_bytes: u64,
    /// Number of cues in the document.
    pub cue_count: u64,
    /// Translatable units under the selected provider unit.
    pub translatable_units: u64,
    /// Source document format.
    pub format: SubtitleFormat,
}

/// Complete translated subtitle document ready for publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedSubtitleDocument {
    /// Complete WebVTT bytes.
    pub bytes: Vec<u8>,
    /// Strong ETag of the final bytes, including its surrounding quotes.
    pub etag: String,
    /// Response media type.
    pub media_type: String,
    /// Response cache policy.
    pub cache_control: String,
}

impl TranslatedSubtitleDocument {
    /// Creates a translated document with the default cache policy.
    ///
    /// # Arguments
    ///
    /// * `bytes` - Complete translated WebVTT bytes.
    /// * `etag` - Strong ETag including surrounding quotes.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::InvalidDescriptor`] when the ETag is
    /// empty or not quoted as a strong validator.
    pub fn new(bytes: Vec<u8>, etag: impl Into<String>) -> Result<Self, SubtitleTranslationError> {
        let etag = etag.into();
        if etag.len() < 2 || !etag.starts_with('"') || !etag.ends_with('"') {
            return Err(SubtitleTranslationError::InvalidDescriptor {
                detail: "final document ETag must be a quoted strong validator".to_string(),
            });
        }
        Ok(Self {
            bytes,
            etag,
            media_type: WEBVTT_MEDIA_TYPE.to_string(),
            cache_control: DEFAULT_SUBTITLE_CACHE_CONTROL.to_string(),
        })
    }
}
