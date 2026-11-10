//! Injected retrieval and document cache ports.

use super::cache::{CachedSubtitleTranslation, SubtitleTranslationCacheKey};
use super::document::{RetrievedSubtitle, SubtitleSourceDescriptor};
use super::error::SubtitleTranslationError;
use arachnea_translation::AccessScope;
use async_trait::async_trait;

/// Authorized source retrieval implemented by the stream layer.
///
/// The stream layer resolves application proxy links, controls HTTP(S)
/// destinations, redirects and private addresses, and returns the complete
/// document bytes. The subtitle layer never performs its own unrestricted
/// network access.
#[async_trait]
pub trait SubtitleRetrievalPort: Send + Sync {
    /// Retrieves one authorized source track.
    ///
    /// # Arguments
    ///
    /// * `source` - Source track descriptor.
    /// * `access_scope` - Access scope resolved for the current caller.
    ///
    /// # Errors
    ///
    /// Returns a contextualized retrieval or access error.
    async fn retrieve(
        &self,
        source: &SubtitleSourceDescriptor,
        access_scope: &AccessScope,
    ) -> Result<RetrievedSubtitle, SubtitleTranslationError>;
}

/// Persistent translated document cache implemented by the stream layer.
///
/// Entries are written atomically and are never published after a partial
/// failure. The cache is distinct from the Scrapyfy row cache and store
/// separate source fingerprints and final document ETags.
#[async_trait]
pub trait SubtitleTranslationCachePort: Send + Sync {
    /// Looks up one retained translated document.
    ///
    /// # Arguments
    ///
    /// * `key` - Complete subtitle document cache key.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::CacheUnavailable`] on storage errors.
    async fn lookup(
        &self,
        key: &SubtitleTranslationCacheKey,
    ) -> Result<Option<CachedSubtitleTranslation>, SubtitleTranslationError>;

    /// Stores or replaces one translated document atomically.
    ///
    /// # Arguments
    ///
    /// * `key` - Complete subtitle document cache key.
    /// * `entry` - Validated result bytes and response metadata.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::CacheUnavailable`] on storage errors.
    async fn store(
        &self,
        key: &SubtitleTranslationCacheKey,
        entry: &CachedSubtitleTranslation,
    ) -> Result<(), SubtitleTranslationError>;
}
