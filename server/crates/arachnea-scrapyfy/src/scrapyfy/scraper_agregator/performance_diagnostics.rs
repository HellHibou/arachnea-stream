//! Provider query lifetime diagnostics.

/// Observes one provider query without logging its parameters or response body.
#[cfg(all(feature = "arachnea-proxy", feature = "performance-diagnostics"))]
pub(super) struct ProxySourceObservation<'a> {
    pub(super) source: &'a str,
    pub(super) query: &'a str,
    pub(super) country: Option<String>,
    pub(super) started: std::time::Instant,
    pub(super) outcome: &'static str,
    pub(super) rows: usize,
}

#[cfg(all(feature = "arachnea-proxy", feature = "performance-diagnostics"))]
impl Drop for ProxySourceObservation<'_> {
    fn drop(&mut self) {
        tracing::debug!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
            source = self.source,
            query = self.query,
            country = self.country.as_deref(),
            outcome = self.outcome,
            rows = self.rows,
            duration_ms = self.started.elapsed().as_millis(),
            "finished proxy provider source query"
        );
    }
}
