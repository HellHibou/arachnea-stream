//! Runtime options controlling how an aggregated query is executed.

/// Options controlling how a query execution behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryParameters {
    /// Whether conditional ETag validation is enabled for this execution.
    ///
    /// When enabled, the client fragments decoded from the incoming global
    /// ETag drive per-source conditional requests, stale sources are re-fetched
    /// with forced full GETs, and the aggregation result carries the rebuilt
    /// global ETag.
    pub enable_etag: bool,
}

impl Default for QueryParameters {
    /// Returns the default options — conditional ETag validation enabled.
    fn default() -> Self {
        Self { enable_etag: true }
    }
}
