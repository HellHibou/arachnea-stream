//! Injected authorized conditional source retrieval port.

use super::cache::SubtitleSourceValidator;
use super::document::{SubtitleRetrievalOutcome, SubtitleSourceDescriptor};
use super::error::SubtitleTranslationError;
use arachnea_translation::AccessScope;
use async_trait::async_trait;

/// Authorized source retrieval implemented by the stream layer.
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
