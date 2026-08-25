//! Runtime options controlling how an aggregated query is executed.

use super::scraper_cache::CacheType;

/// Options controlling how a query execution behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryParameters {
    /// Cache behavior for this execution (client ETag flow and/or server cache).
    ///
    /// Defaults to [`CacheType::ClientCache`] — the historical behavior where
    /// only the client-driven conditional ETag validation runs and no
    /// server-side caching is involved.
    pub cache_type: CacheType,
}

impl Default for QueryParameters {
    /// Returns the default options — client-side ETag validation enabled.
    fn default() -> Self {
        Self {
            cache_type: CacheType::default(),
        }
    }
}

impl QueryParameters {

    /// Creates a new [`QueryParameters`] with the given cache type.
    /// 
    ///  # Arguments
    /// * `cache_type:` - The cache type to use for this query execution.
    pub fn from_cache_type(cache_type: CacheType) -> Self {
        Self { cache_type }
    }

    /// Sets the cache type for this query execution.
    /// 
    ///  # Arguments
    /// * `cache_type:` - The cache type to use for this query execution.
    pub fn set_cache_type(&mut self, cache_type: CacheType) -> Self {
        self.cache_type = cache_type;
        *self  
    }
}
