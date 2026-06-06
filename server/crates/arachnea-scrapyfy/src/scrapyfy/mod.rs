use anyhow::Result;
use async_trait::async_trait;

/// Tree-shaped output node model used by all scraper pipelines.
pub mod scraper_data_node;
use std::collections::HashMap;

pub use scraper_data_node::ScraperDataNode;

/// HTTP client helpers used by query executors and tests.
pub mod http_client;
pub use http_client::HttpClient;
pub use http_client::{ScraperHttpConfig, ScraperHttpMode, ScraperHttpUserAgentProfile};

/// Text extraction and normalization actions.
pub mod actions;
pub use actions::{GetDateSource, GetDateSources, ScraperAction};

/// Post-process transformations applied after raw extraction.
pub mod scraper_post_process;
pub use scraper_post_process::{ScraperPostProcess, ScraperPostProcessContext};

/// HTML field extractor definitions.
pub mod html_scraper_entry;
pub use html_scraper_entry::HtmlScraperEntry;
pub use html_scraper_entry::HtmlScraperEntryRaw;

/// HTML query definitions and executors.
pub mod html_scraper_query;
pub use html_scraper_query::HtmlScraperQuery;
pub use html_scraper_query::HtmlScraperQueryRaw;

/// JSON field extractor definitions.
pub mod json_scraper_entry;
pub use json_scraper_entry::JsonScraperEntry;
pub use json_scraper_entry::JsonScraperEntryRaw;

/// JSON query definitions and executors.
pub mod json_scraper_query;
pub use json_scraper_query::JsonScraperQuery;
pub use json_scraper_query::JsonScraperQueryRaw;

/// Static query definitions and executors.
pub mod static_scraper_query;
pub use static_scraper_query::{StaticScraperEntryRaw, StaticScraperQuery, StaticScraperQueryRaw};

/// Shared helpers for query URL formatting and filtering.
pub mod query_helpers;

/// Query collection models and file loading helpers.
pub mod scraper_query_collection;
pub use scraper_query_collection::{
    ScraperQueryCollection, ScraperQueryCollectionParameter, ScraperQueryCollectionRaw,
};

/// Multi-source query aggregator.
pub mod scraper_agregator;
pub use scraper_agregator::{ScraperAgregator, ScraperSourceParams};

/// Scraper manager trait used by runtime and test harnesses.
pub mod scraper_manager;
pub use scraper_manager::ScraperManager;

/// Shared contract for scraper queries that can format a request and extract rows.
#[async_trait]
pub trait ScraperQuery {
    /// Returns the query name used as the lookup key in a collection.
    fn name(&self) -> String;

    /// Returns the base URL used by the query and exposed as `{base_url}` in templates.
    fn base_url(&self) -> &str;

    /// Returns the media types associated with the query results.
    fn media_types(&self) -> &Vec<String>;

    /// Returns the URL template used to build the request.
    fn query_url(&self) -> &str;

    /// Returns the configured field extractors executed for every matched row.
    fn scraper_entries(&self) -> &Vec<HtmlScraperEntry>;

    /// Returns whether the query matches at least one requested media type.
    fn is_media_type(&self, media_types: &[String]) -> bool;

    /// Returns the flattened list of leaf field names produced by this query.
    #[cfg(any(test, feature = "test-support"))]
    fn get_field_names(&self) -> Vec<String>;

    /// Executes the query by formatting the URL, fetching the page, and extracting each row.
    ///
    /// # Arguments
    ///
    /// * `params` - Runtime values used to replace placeholders in the URL template
    ///   and in formatting actions executed by the scraper pipeline.
    /// * `fields_filters` - Root fields filter list or None.
    ///
    /// Field names containing `>` are interpreted as hierarchical paths and will
    /// be converted into nested objects or arrays in the returned entries.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL template is missing parameters, if the page
    /// cannot be downloaded, or if a response cannot be parsed.
    async fn execute_query(
        &self,
        params: &HashMap<String, String>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
    ) -> Result<Vec<HashMap<String, ScraperDataNode>>>;
}
