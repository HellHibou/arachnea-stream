//! Common subtitle document formats and bounds.

use super::error::SubtitleTranslationError;
use crate::{
    DEFAULT_MAX_CUES, DEFAULT_MAX_DOCUMENT_BYTES, DEFAULT_MAX_TRANSLATABLE_UNITS, SRT_MEDIA_TYPE,
    WEBVTT_MEDIA_TYPE,
};
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

/// Local subtitle document bounds applied before translation.
///
/// These bounds are independent from provider limits. A document that exceeds
/// them is rejected before any translation call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalSubtitleLimits {
    /// Maximum decompressed source document size in bytes.
    pub max_document_bytes: u64,
    /// Maximum number of cues in one document.
    pub max_cues: u64,
    /// Maximum translatable units in one document.
    pub max_translatable_units: u64,
}

impl Default for LocalSubtitleLimits {
    /// Returns prudent local defaults from the crate constants.
    fn default() -> Self {
        Self {
            max_document_bytes: DEFAULT_MAX_DOCUMENT_BYTES,
            max_cues: DEFAULT_MAX_CUES,
            max_translatable_units: DEFAULT_MAX_TRANSLATABLE_UNITS,
        }
    }
}

impl LocalSubtitleLimits {
    /// Validates measured stats against the local bounds.
    ///
    /// # Arguments
    ///
    /// * `stats` - Measured source document characteristics.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::DocumentTooLarge`],
    /// [`SubtitleTranslationError::TooManyCues`] or
    /// [`SubtitleTranslationError::TooManyUnits`] for the first exceeded bound.
    pub fn validate(&self, stats: &SubtitleDocumentStats) -> Result<(), SubtitleTranslationError> {
        if stats.document_bytes > self.max_document_bytes {
            return Err(SubtitleTranslationError::DocumentTooLarge {
                bytes: stats.document_bytes,
                limit: self.max_document_bytes,
            });
        }
        if stats.cue_count > self.max_cues {
            return Err(SubtitleTranslationError::TooManyCues {
                cues: stats.cue_count,
                limit: self.max_cues,
            });
        }
        if stats.translatable_units > self.max_translatable_units {
            return Err(SubtitleTranslationError::TooManyUnits {
                units: stats.translatable_units,
                limit: self.max_translatable_units,
            });
        }
        Ok(())
    }
}
