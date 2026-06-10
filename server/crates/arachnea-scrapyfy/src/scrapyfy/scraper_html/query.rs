//! Runtime query type for HTML scraper endpoints.
//!
//! An [`HtmlScraperQuery`] wraps a validated CSS selector, entry definitions,
//! and HTTP configuration.  Execution is delegated to the unified executor
//! ([`ScraperQuery`](crate::scrapyfy::scraper::query_trait::ScraperQuery)).
//!
//! Raw YAML deserialization and bidirectional conversion live in [`config`].
//! HTML response parsing lives in [`response_parser`].
//! Row-level post-processing lives in [`row_extractor`].

use std::any::Any;

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;

use crate::scrapyfy::*;
use crate::scrapyfy::scraper::config::{ScraperRequestHeader, ScraperRequestMethod};
use crate::scrapyfy::scraper::entry_trait::ScraperEntrySpec;
use crate::scrapyfy::scraper::query_trait::ScraperQuery;
use crate::scrapyfy::scraper::row_locator::{RowLocator, ScraperType};
use crate::scrapyfy::scraper::sub_query_spec::SubQuerySpec;
use crate::scrapyfy::scraper_html::config::HtmlScraperQueryRaw;
use crate::scrapyfy::scraper_html::entry::{HtmlScraperEntry, HtmlScraperSelectMode};
use crate::scrapyfy::query_helpers::QueryTemplateParamMapping;

/// A query definition for one HTML source and one result shape.
///
/// Fields are `pub(crate)` so that [`config`] can populate them during
/// the raw-to-validated conversion.
#[derive(Deserialize)]
#[serde(try_from = "HtmlScraperQueryRaw")]
pub struct HtmlScraperQuery {
    /// Query identifier used as the lookup key in a collection.
    pub(crate) name: String,
    /// Resolved base URL of the source.
    pub(crate) base_url: String,
    /// Original base URL template preserved from YAML.
    pub(crate) base_url_template: String,
    /// Content types this query produces.
    pub(crate) media_types: Vec<String>,
    /// URL template used to build the request.
    pub(crate) query_url: String,
    /// HTTP method used to issue the request.
    pub(crate) request_method: ScraperRequestMethod,
    /// Optional JSON pointer selecting the request body from runtime params.
    pub(crate) request_body_pointer: Option<String>,
    /// Selection mode for the request body pointer.
    pub(crate) request_body_select: HtmlScraperSelectMode,
    /// Actions applied to the request body before the HTTP call.
    pub(crate) request_body_actions: Vec<ScraperAction>,
    /// HTTP headers attached to the request.
    pub(crate) request_headers: Vec<ScraperRequestHeader>,
    /// HTTP client configuration (mode, user agent, max redirects).
    pub(crate) http_config: ScraperHttpConfig,
    /// Source-to-target parameter mappings applied before template resolution.
    pub(crate) query_param_mappings: Vec<QueryTemplateParamMapping>,
    /// Maximum number of rows whose async post-process steps may run together.
    pub(crate) row_concurrency: usize,
    /// Compiled CSS selector matching each result row in the fetched page.
    row_selector_compiled: ::scraper::Selector,
    /// Original row selector template preserved from YAML.
    pub(crate) row_selector_template: String,
    /// Optional group field whose items should become query rows.
    pub(crate) result_item_field: Option<String>,
    /// Field extractors executed for every matched row.
    pub(crate) scraper_entries: Vec<HtmlScraperEntry>,
    /// Post-processing steps applied to each extracted row.
    pub(crate) post_processes: Vec<ScraperPostProcess>,

    /// HTTP client used to issue requests.
    pub(crate) http_client: HttpClient,
}

impl HtmlScraperQuery {
    /// Validates every request-body action against the query-level contract.
    ///
    /// # Arguments
    ///
    /// * `name` - Query name used in diagnostic messages.
    /// * `actions` - Request body actions to validate.
    ///
    /// # Errors
    ///
    /// Returns an error if any action fails validation for the `"query"` context.
    pub(crate) fn validate_request_actions(name: &str, actions: &[ScraperAction]) -> Result<()> {
        crate::scrapyfy::query_helpers::validate_request_actions("query", name, actions)
    }

    /// Creates a query and panics if the row selector is invalid.
    ///
    /// # Arguments
    ///
    /// * `name` - Query identifier used to look up this query in a collection.
    /// * `base_url` - Base URL of the source, also available as `{base_url}` in templates.
    /// * `media_types` - Content type result.
    /// * `query_url` - URL template used to build the request.
    /// * `row_selector` - CSS selector matching each result row in the fetched page.
    /// * `scraper_entries` - Field extractors executed for every matched row.
    ///
    /// # Panics
    ///
    /// Panics if `row_selector` is not a valid CSS selector.
    #[allow(dead_code)]
    pub fn new(
        name: &str,
        base_url: &str,
        media_types: Vec<String>,
        query_url: &str,
        row_selector: &str,
        scraper_entries: Vec<HtmlScraperEntry>,
    ) -> Self {
        Self::try_new(
            name,
            base_url,
            media_types,
            query_url,
            4,
            row_selector,
            scraper_entries,
        )
        .unwrap_or_else(|_| panic!("Invalid format: {}", row_selector))
    }

    #[allow(dead_code)]
    /// Returns the raw list of configured HTML scraper entries.
    pub fn get_scraper_entries(&self) -> &Vec<HtmlScraperEntry> {
        &self.scraper_entries
    }

    /// Validates the row selector and stores the configured field extractors.
    ///
    /// # Arguments
    ///
    /// * `name` - Query identifier used to look up this query in a collection.
    /// * `base_url` - Base URL of the source, also available as `{base_url}` in templates.
    /// * `media_types` - Content type result.
    /// * `query_url` - URL template used to build the request.
    /// * `row_concurrency` - Max number of extracted rows whose async post-process steps
    ///   may run together.
    /// * `row_selector` - CSS selector matching each result row in the fetched page.
    /// * `scraper_entries` - Field extractors executed for every matched row.
    ///
    /// # Errors
    ///
    /// Returns an error if `row_selector` is not a valid CSS selector.
    pub fn try_new(
        name: &str,
        base_url: &str,
        media_types: Vec<String>,
        query_url: &str,
        row_concurrency: usize,
        row_selector: &str,
        scraper_entries: Vec<HtmlScraperEntry>,
    ) -> Result<Self> {
        if row_concurrency == 0 {
            anyhow::bail!(
                "Invalid HTML row concurrency for {}: concurrency must be greater than 0",
                name
            );
        }

        let row_selector_compiled = ::scraper::Selector::parse(row_selector)
            .map_err(|err| anyhow::anyhow!("Invalid row selector {}: {:?}", row_selector, err))?;

        let mut query = HtmlScraperQuery {
            name: name.to_string(),
            base_url: base_url.to_string(),
            base_url_template: base_url.to_string(),
            media_types,
            http_client: HttpClient::new(base_url),
            http_config: ScraperHttpConfig::default(),
            query_url: query_url.to_string(),
            request_method: ScraperRequestMethod::Get,
            request_body_pointer: None,
            request_body_select: HtmlScraperSelectMode::All,
            request_body_actions: Vec::new(),
            request_headers: Vec::new(),
            query_param_mappings: Vec::new(),
            row_concurrency,
            row_selector_compiled,
            row_selector_template: row_selector.to_string(),
            result_item_field: None,
            scraper_entries: Vec::new(),
            post_processes: Vec::new(),
        };

        for scraper_entry in scraper_entries {
            query.add_entry(scraper_entry);
        }

        Ok(query)
    }

    /// Registers an additional field extractor on the query.
    ///
    /// # Arguments
    ///
    /// * `scraper_entry` - Field extractor added to the query result shape.
    pub fn add_entry(&mut self, scraper_entry: HtmlScraperEntry) -> &mut Self {
        self.scraper_entries.push(scraper_entry);
        self
    }

    /// Returns the optional group field whose items should become query rows.
    pub fn result_item_field(&self) -> Option<&str> {
        self.result_item_field.as_deref()
    }

    /// Returns the configured field extractors executed for every matched row.
    pub fn scraper_entries(&self) -> &Vec<HtmlScraperEntry> {
        &self.scraper_entries
    }

    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// Returns the base URL used by the query and exposed as `{base_url}` in templates.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Returns the media types associated with the query results.
    pub fn media_types(&self) -> &Vec<String> {
        &self.media_types
    }

    /// Returns the URL template used to build the request.
    pub fn query_url(&self) -> &str {
        &self.query_url
    }

    /// Returns the flattened list of leaf field names produced by this query.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_field_names(&self) -> Vec<String> {
        self.scraper_entries()
            .iter()
            .flat_map(HtmlScraperEntry::field_names)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// HtmlScraperSubQuery
// ---------------------------------------------------------------------------

/// Runtime definition of one chained HTML follow-up request.
///
/// An `HtmlScraperSubQuery` fetches an HTML page derived from a parent row,
/// selects rows via a CSS selector, and merges the extracted fields into the
/// parent data tree.  This type implements [`ScraperQuery`] so it can be
/// executed by the unified executor.
pub struct HtmlScraperSubQuery {
    /// Optional CSS selector scoping the context rows that seed follow-up requests.
    pub(crate) context_pointer: Option<String>,
    /// Selection mode for the context pointer.
    #[allow(dead_code)]
    pub(crate) context_select: HtmlScraperSelectMode,
    /// Filters applied to the context row before issuing the follow-up request.
    #[allow(dead_code)]
    pub(crate) filters: HashMap<String, Vec<String>>,
    /// Filters applied to the fetched rows (after the HTTP response).
    #[allow(dead_code)]
    pub(crate) row_filters: HashMap<String, Vec<String>>,
    /// Entries extracted from the context row rather than from fetched rows.
    pub(crate) context_entries: Vec<HtmlScraperEntry>,
    /// Path where the sub-query result is nested (default: root level).
    #[allow(dead_code)]
    pub(crate) target: Option<String>,
    /// Pointer selecting the request URL from the context row.
    pub(crate) request_pointer: Option<String>,
    /// Selection mode for the request pointer.
    pub(crate) request_select: HtmlScraperSelectMode,
    /// Actions applied to the selected request URLs.
    pub(crate) request_actions: Vec<ScraperAction>,
    /// HTTP method used for the follow-up request.
    pub(crate) request_method: ScraperRequestMethod,
    /// HTTP headers attached to the follow-up request.
    pub(crate) request_headers: Vec<ScraperRequestHeader>,
    /// HTTP client configuration for the follow-up request.
    pub(crate) http_config: ScraperHttpConfig,
    /// CSS selector matching each result row in the response.
    pub(crate) row_selector: String,
    /// Compiled CSS selector for efficient matching.
    #[allow(dead_code)]
    pub(crate) row_selector_compiled: ::scraper::Selector,
    /// Field extractors executed for every matched row.
    pub(crate) entries: Vec<HtmlScraperEntry>,
    /// Post-processing steps applied to each extracted row.
    pub(crate) post_processes: Vec<ScraperPostProcess>,
    /// Nested sub-queries (recursion). Polymorphic slot allowing
    /// heterogeneous composition at the entry level.
    pub(crate) sub_queries: Vec<Box<dyn ScraperQuery>>,
    /// HTTP client for follow-up requests.
    pub(crate) http_client: HttpClient,
}

impl ScraperQuery for HtmlScraperQuery {
    fn scraper_type(&self) -> ScraperType {
        ScraperType::Html
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn media_types(&self) -> &[String] {
        &self.media_types
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn query_url(&self) -> &str {
        &self.query_url
    }

    fn request_method(&self) -> ScraperRequestMethod {
        self.request_method
    }

    fn request_pointer(&self) -> Option<&str> {
        self.request_body_pointer.as_deref()
    }

    fn request_select(&self) -> HtmlScraperSelectMode {
        self.request_body_select
    }

    fn request_actions(&self) -> &[ScraperAction] {
        &self.request_body_actions
    }

    fn request_headers(&self) -> &[ScraperRequestHeader] {
        &self.request_headers
    }

    fn http_config(&self) -> &ScraperHttpConfig {
        &self.http_config
    }

    fn http_client(&self) -> &HttpClient {
        &self.http_client
    }

    fn extract_next_data(&self) -> bool {
        false
    }

    fn row_locator(&self) -> RowLocator {
        RowLocator::Selector {
            selector: self.row_selector_template.clone(),
            select: HtmlScraperSelectMode::All,
        }
    }

    fn post_processes(&self) -> &[crate::scrapyfy::ScraperPostProcess] {
        &self.post_processes
    }

    fn entries(&self) -> Vec<&dyn ScraperEntrySpec> {
        self.scraper_entries
            .iter()
            .map(|entry| entry as &dyn ScraperEntrySpec)
            .collect()
    }

    fn sub_queries(&self) -> Vec<&dyn ScraperQuery> {
        Vec::new()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ScraperQuery for HtmlScraperSubQuery {
    fn scraper_type(&self) -> ScraperType {
        ScraperType::Html
    }

    fn name(&self) -> &str {
        "html-sub-query"
    }

    fn media_types(&self) -> &[String] {
        &[]
    }

    fn is_media_type(&self, _media_types: &[String]) -> bool {
        true
    }

    fn base_url(&self) -> &str {
        ""
    }

    fn query_url(&self) -> &str {
        ""
    }

    fn request_method(&self) -> ScraperRequestMethod {
        self.request_method
    }

    fn request_pointer(&self) -> Option<&str> {
        self.request_pointer.as_deref()
    }

    fn request_select(&self) -> HtmlScraperSelectMode {
        self.request_select
    }

    fn request_actions(&self) -> &[ScraperAction] {
        &self.request_actions
    }

    fn request_headers(&self) -> &[ScraperRequestHeader] {
        &self.request_headers
    }

    fn http_config(&self) -> &ScraperHttpConfig {
        &self.http_config
    }

    fn http_client(&self) -> &HttpClient {
        &self.http_client
    }

    fn extract_next_data(&self) -> bool {
        false
    }

    fn row_locator(&self) -> RowLocator {
        RowLocator::Selector {
            selector: self.row_selector.clone(),
            select: HtmlScraperSelectMode::All,
        }
    }

    fn post_processes(&self) -> &[crate::scrapyfy::ScraperPostProcess] {
        &self.post_processes
    }

    fn entries(&self) -> Vec<&dyn ScraperEntrySpec> {
        let mut all: Vec<&dyn ScraperEntrySpec> = self
            .context_entries
            .iter()
            .map(|entry| entry as &dyn ScraperEntrySpec)
            .collect();
        all.extend(
            self.entries
                .iter()
                .map(|entry| entry as &dyn ScraperEntrySpec),
        );
        all
    }

    fn sub_queries(&self) -> Vec<&dyn ScraperQuery> {
        self.sub_queries
            .iter()
            .map(|sub| &**sub as &dyn ScraperQuery)
            .collect()
    }

    fn sub_query_spec(&self) -> Option<&SubQuerySpec> {
        None
    }

    // --- Query-level sub-query overrides (legacy semantics) ---

    fn context_pointer(&self) -> Option<&str> {
        self.context_pointer.as_deref()
    }

    fn context_select(&self) -> HtmlScraperSelectMode {
        self.context_select
    }

    fn context_entries(&self) -> Vec<&dyn ScraperEntrySpec> {
        self.context_entries
            .iter()
            .map(|entry| entry as &dyn ScraperEntrySpec)
            .collect()
    }

    fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}