//! Placeholder for the unified query executor.
//!
//! Step 5 introduces the shared types ([`ScraperQuery`], [`ScraperEntrySpec`],
//! [`SubQuerySpec`], [`RowLocator`]). Step 6+ will move the existing
//! `JsonScraperSubQuery` execution logic into this module and add HTML
//! support so the trait surface is the single execution entry point.

use anyhow::Result;
use std::collections::HashMap;

use crate::scrapyfy::scraper_data_node::ScraperDataNode;
use crate::scrapyfy::ScraperHttpConfig;

use super::query_trait::ScraperQuery;
use super::sub_query_spec::SubQuerySpec;

/// Runtime context forwarded to sub-query execution.
///
/// Mirrors the runtime context used by the legacy `JsonScraperSubQuery`
/// execution path; the unified executor will adopt the same shape.
pub struct QueryContext<'a> {
    /// Runtime template parameters.
    pub params: &'a HashMap<String, String>,
    /// URL of the parent request for action resolution.
    pub request_url: &'a str,
    /// Optional parent response body available to actions.
    pub response_body: Option<&'a str>,
    /// Shared HTTP client.
    pub http_client: &'a crate::scrapyfy::http_client::HttpClient,
}

/// Stub executor used to keep the trait surface compilable in step 5.
///
/// The full implementation lands in steps 6+ and replaces the legacy
/// `JsonScraperSubQuery::execute` path.
///
/// # Arguments
///
/// * `query` - The scraper query to execute.
/// * `context` - Runtime context forwarded to sub-query execution.
///
/// # Errors
///
/// Always returns an error — this is a placeholder until step 6+.
pub async fn execute_query(
    _query: &dyn ScraperQuery,
    _context: &QueryContext<'_>,
) -> Result<ScraperDataNode> {
    anyhow::bail!("execute_query is not implemented yet; see step 6 of the sub_query_at_entry refactor")
}

/// Returns the optional [`SubQuerySpec`] attached to the given query.
///
/// # Arguments
///
/// * `query` - The scraper query to inspect.
pub fn resolve_sub_query_spec(_query: &dyn ScraperQuery) -> Option<&SubQuerySpec> {
    _query.sub_query_spec()
}

/// Returns the HTTP configuration associated with the given query.
///
/// # Arguments
///
/// * `query` - The scraper query whose HTTP config is returned.
pub fn http_config(_query: &dyn ScraperQuery) -> &ScraperHttpConfig {
    _query.http_config()
}
