//! Authorized conditional document retrieval contracts.

use super::{error::SubtitleTranslationError, types::SubtitleFormat};
use crate::{AccessScope, LanguageCode};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

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
    /// Upstream Last-Modified header, when present.
    pub last_modified: Option<String>,
    /// Final URL after authorized redirects, when known.
    pub final_url: Option<String>,
}

/// Result of an authorized conditional source retrieval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtitleRetrievalOutcome {
    /// Complete source bytes and current upstream metadata.
    Modified(RetrievedSubtitle),
    /// The upstream server confirmed the conditional request was unchanged.
    /// This variant is only valid when a conditional validator was sent.
    NotModified,
}

/// Validator used to check whether the upstream subtitle source changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubtitleSourceValidator {
    /// Original HTTP entity tag, including quotes and any `W/` prefix.
    Etag {
        /// Upstream entity tag to send in `If-None-Match`.
        etag: String,
    },
    /// Digest of the source bytes, optionally with an HTTP modification date.
    Content {
        /// Digest of the complete source bytes.
        hash: String,
        /// Original `Last-Modified` value for `If-Modified-Since`, if present.
        last_modified: Option<String>,
    },
}

/// Authorized source retrieval supplied by an adapter or application.
///
/// The implementation controls destinations and redirects. It sends the
/// upstream ETag or modification date when supplied, and returns complete
/// bytes when the server responds with content. A hash without a date requires
/// a full retrieval; the caller compares the returned bytes against that hash.
#[async_trait]
pub trait SubtitleRetrievalPort: Send + Sync {
    /// Retrieves or conditionally revalidates one authorized source track.
    ///
    /// # Arguments
    ///
    /// * `source` - Source track descriptor.
    /// * `access_scope` - Access scope resolved for the current caller.
    /// * `validator` - Previously received source validator, if available.
    ///
    /// # Errors
    ///
    /// Returns a contextualized retrieval or access error.
    async fn retrieve(
        &self,
        source: &SubtitleSourceDescriptor,
        access_scope: &AccessScope,
        validator: Option<&SubtitleSourceValidator>,
    ) -> Result<SubtitleRetrievalOutcome, SubtitleTranslationError>;
}
