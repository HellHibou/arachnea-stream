//! Subtitle formats, source descriptors and document results.

use super::error::SubtitleTranslationError;
pub use super::types::{LocalSubtitleLimits, SubtitleDocumentStats, SubtitleFormat};
use crate::{DEFAULT_SUBTITLE_CACHE_CONTROL, WEBVTT_MEDIA_TYPE};

pub use super::retrieval::{RetrievedSubtitle, SubtitleRetrievalOutcome, SubtitleSourceDescriptor};

/// Complete translated subtitle document ready for publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedSubtitleDocument {
    /// Complete WebVTT bytes.
    pub bytes: Vec<u8>,
    /// Weak ETag identifying the document/provider revision and source validator.
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
    /// * `etag` - Weak provider/source ETag including surrounding quotes.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::InvalidDescriptor`] when the ETag is
    /// not a valid weak provider/source validator.
    pub fn new(bytes: Vec<u8>, etag: impl Into<String>) -> Result<Self, SubtitleTranslationError> {
        let etag = etag.into();
        if !etag.starts_with("W/\"") {
            return Err(SubtitleTranslationError::InvalidDescriptor {
                detail: "subtitle document ETag must be weak".to_string(),
            });
        }
        super::cache::SubtitleTranslationValidator::from_etag(&etag)?;
        Ok(Self {
            bytes,
            etag,
            media_type: WEBVTT_MEDIA_TYPE.to_string(),
            cache_control: DEFAULT_SUBTITLE_CACHE_CONTROL.to_string(),
        })
    }
}
