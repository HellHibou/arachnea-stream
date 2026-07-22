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

use crate::scrapyfy::query_helpers::QueryTemplateParamMapping;
use crate::scrapyfy::scraper::config::{ScraperRequestHeader, ScraperRequestMethod};
use crate::scrapyfy::scraper::entry_trait::ScraperEntrySpec;
use crate::scrapyfy::scraper::query_trait::ScraperQuery;
use crate::scrapyfy::scraper::row_locator::{RowLocator, ScraperType};
use crate::scrapyfy::scraper_html::config::HtmlScraperQueryRaw;
use crate::scrapyfy::scraper_html::entry::{HtmlScraperEntry, HtmlScraperSelectMode};
use crate::scrapyfy::*;

/// A query definition for one HTML source and one result shape.
///
/// Represents a validated HTML scraper query that fetches HTML pages and extracts
/// structured data using CSS selectors. Fields are `pub(crate)` so that the
/// [`config`] module can populate them during the raw-to-validated conversion.
///
/// # Type Parameters
///
/// This struct contains all the configuration needed to execute an HTML scraping query:
/// - Query identification (name, base URL)
/// - HTTP request configuration (method, headers, body)
/// - Row selection (CSS selector for finding data rows)
/// - Data extraction (entries with their own selectors and actions)
/// - Post-processing (transformations applied to extracted data)
#[derive(Deserialize)]
#[serde(try_from = "HtmlScraperQueryRaw")]
pub struct HtmlScraperQuery {
    /// Query identifier used as the lookup key in a collection.
    ///
    /// This name is used to reference the query within a collection of scrapers.
    pub(crate) name: String,
    /// Resolved base URL of the source.
    ///
    /// The base URL after all template placeholders have been resolved.
    /// This is available as `{base_url}` in URL templates.
    pub(crate) base_url: String,
    /// Original base URL template preserved from YAML.
    ///
    /// The raw base URL as defined in the YAML configuration, before placeholder resolution.
    pub(crate) base_url_template: String,
    /// Content types this query produces.
    ///
    /// Media types (MIME types) associated with the data extracted by this query.
    /// Used for filtering queries based on requested content types.
    pub(crate) media_types: Vec<String>,
    /// URL template used to build the request.
    ///
    /// The URL template that is formatted with runtime parameters to build
    /// the actual request URL. Supports `{placeholder}` syntax.
    pub(crate) query_url: String,
    /// Actions applied to the resolved root query URL before the HTTP call.
    pub(crate) request_url_actions: Vec<ScraperAction>,
    /// HTTP method used to issue the request.
    ///
    /// The HTTP method (GET or POST) used when making the request.
    pub(crate) request_method: ScraperRequestMethod,
    /// Optional JSON pointer selecting the request body from runtime params.
    ///
    /// When set, the request body is extracted from the specified JSON pointer
    /// in the runtime parameters before the request is sent.
    pub(crate) request_body_pointer: Option<String>,
    /// Selection mode for the request body pointer.
    ///
    /// Controls whether to use the first match or all matches when extracting
    /// the request body from the runtime parameters.
    pub(crate) request_body_select: HtmlScraperSelectMode,
    /// Actions applied to the request body before the HTTP call.
    ///
    /// Transformations applied to the request body before it is sent.
    pub(crate) request_body_actions: Vec<ScraperAction>,
    /// HTTP headers attached to the request.
    ///
    /// Headers that are included in the HTTP request.
    pub(crate) request_headers: Vec<ScraperRequestHeader>,
    /// HTTP client configuration (mode, user agent, max redirects).
    ///
    /// Settings for the HTTP client used to make requests.
    pub(crate) http_config: ScraperHttpConfig,
    /// Source-to-target parameter mappings applied before template resolution.
    ///
    /// Mappings that transform source parameter names to target parameter names
    /// before URL and other templates are resolved.
    pub(crate) query_param_mappings: Vec<QueryTemplateParamMapping>,
    /// Maximum number of rows whose async post-process steps may run together.
    ///
    /// Controls the concurrency of async post-processing operations.
    /// A value of 0 is invalid and will cause an error during query construction.
    pub(crate) row_concurrency: usize,
    /// Compiled CSS selector matching each result row in the fetched page.
    #[allow(dead_code)]
    pub(crate) row_selector_compiled: ::scraper::Selector,
    /// Original row selector template preserved from YAML.
    ///
    /// The raw CSS selector string as defined in the YAML configuration,
    /// before placeholder resolution. Used for serialization.
    pub(crate) row_selector_template: String,
    /// Optional group field whose items should become query rows.
    ///
    /// When set, items from the specified field are hoisted to become individual rows
    /// in the output, rather than being nested under a parent object.
    pub(crate) result_item_field: Option<String>,
    /// Field extractors executed for every matched row.
    ///
    /// Entries that define how to extract data from each row matched by the
    /// row selector. Each entry can have its own CSS selector and action pipeline.
    pub(crate) scraper_entries: Vec<HtmlScraperEntry>,
    /// Post-processing steps applied to each extracted row.
    ///
    /// Transformations and processing steps applied to each extracted row
    /// after the data has been extracted but before it is finalized.
    pub(crate) post_processes: Vec<ScraperPostProcess>,

    /// Optional template providing HTML content directly, bypassing the HTTP
    /// fetch. When set, the template is resolved with runtime parameters
    /// (typically `{html}`) and the resulting HTML is parsed directly without
    /// an HTTP request. `query_url` is still used as the context URL for
    /// actions, headers, and error messages.
    pub(crate) input_html: Option<String>,

    /// HTTP client used to issue requests.
    ///
    /// The HTTP client instance configured with the query's HTTP settings.
    pub(crate) http_client: HttpClient,

    // --- Champs sub-query (optionnels, utilisés quand ce query est une sub-query) ---
    /// Optional CSS selector scoping the context rows that seed follow-up requests.
    pub(crate) context_pointer: Option<String>,

    /// Selection mode for the context pointer.
    pub(crate) context_select: HtmlScraperSelectMode,

    /// Filters applied to the context row before issuing the follow-up request.
    #[allow(dead_code)]
    pub(crate) context_filters: HashMap<String, Vec<String>>,

    /// Filters applied to the fetched rows (after the HTTP response).
    #[allow(dead_code)]
    pub(crate) row_filters: HashMap<String, Vec<String>>,

    /// Path where the sub-query result is nested.
    pub(crate) target: Option<String>,

    /// Pointer selecting the request URL from the context row.
    pub(crate) request_pointer: Option<String>,

    /// Selection mode for the request pointer.
    pub(crate) request_select: HtmlScraperSelectMode,

    /// Actions applied to the selected request URLs.
    pub(crate) request_sub_actions: Vec<ScraperAction>,

    /// Top-level sub-queries attached to this query.
    pub(crate) sub_queries: Vec<Box<dyn ScraperQuery>>,
}

impl HtmlScraperQuery {
    /// Validates every request-body action against the query-level contract.
    ///
    /// Ensures that all actions configured for the request body are valid and supported
    /// in the query context.
    ///
    /// # Arguments
    ///
    /// * `name` - Query name used in diagnostic messages when validation fails.
    /// * `actions` - Request body actions to validate.
    ///
    /// # Errors
    ///
    /// Returns an error if any action fails validation for the `"query"` context.
    /// The error includes the query name and the specific validation failure.
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

    /// Returns the raw list of configured HTML scraper entries.
    ///
    /// Provides access to the field extractors that define how data is extracted
    /// from each matched row.
    ///
    /// # Returns
    ///
    /// A reference to the vector of configured [`HtmlScraperEntry`] instances.
    #[allow(dead_code)]
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
            request_url_actions: Vec::new(),
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
            input_html: None,
            // Nouveaux champs sub-query (par défaut)
            context_pointer: None,
            context_select: HtmlScraperSelectMode::All,
            context_filters: HashMap::new(),
            row_filters: HashMap::new(),
            target: None,
            request_pointer: None,
            request_select: HtmlScraperSelectMode::All,
            request_sub_actions: Vec::new(),
            sub_queries: Vec::new(),
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
    ///
    /// Recursively collects all leaf field names from all configured entries,
    /// including nested group entries. This is useful for understanding the
    /// output structure of the query.
    ///
    /// # Returns
    ///
    /// A vector of strings representing all leaf field paths in the output,
    /// with group hierarchies separated by ` > `.
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
/// parent data tree. This type implements [`ScraperQuery`] so it can be
/// executed by the unified executor.
///
/// Sub-queries enable hierarchical data fetching where follow-up requests
/// are made based on values extracted from parent rows.
pub struct HtmlScraperSubQuery {
    /// Optional CSS selector scoping the context rows that seed follow-up requests.
    ///
    /// When set, this selector identifies which rows in the parent result
    /// should trigger follow-up requests.
    pub(crate) context_pointer: Option<String>,
    /// Selection mode for the context pointer.
    ///
    /// Controls whether to use the first match or all matches when selecting
    /// context rows for follow-up requests.
    #[allow(dead_code)]
    pub(crate) context_select: HtmlScraperSelectMode,
    /// Filters applied to the context row before issuing the follow-up request.
    ///
    /// Format: `{"field_name": ["allowed_value_1", "allowed_value_2", ...]}`.
    /// The sub-query is only executed if the context row's field values match.
    #[allow(dead_code)]
    pub(crate) filters: HashMap<String, Vec<String>>,
    /// Filters applied to the fetched rows (after the HTTP response).
    ///
    /// Format: `{"field_name": ["allowed_value_1", "allowed_value_2", ...]}`.
    /// Only rows with matching field values are kept.
    #[allow(dead_code)]
    pub(crate) row_filters: HashMap<String, Vec<String>>,
    /// Entries extracted from the context row rather than from fetched rows.
    ///
    /// These entries extract data from the parent context row that triggered
    /// the sub-query, rather than from the fetched HTML response.
    pub(crate) context_entries: Vec<HtmlScraperEntry>,
    /// Path where the sub-query result is nested (default: root level).
    ///
    /// Path format: `"parent>child>grandchild"` creates nested objects.
    /// `None` means results are merged at the top level.
    #[allow(dead_code)]
    pub(crate) target: Option<String>,
    /// Pointer selecting the request URL from the context row.
    ///
    /// JSON pointer used to extract the request URL from the context row.
    /// This URL is used for the follow-up request.
    pub(crate) request_pointer: Option<String>,
    /// Selection mode for the request pointer.
    ///
    /// Controls whether to use the first match or all matches when extracting
    /// the request URL from the context row.
    pub(crate) request_select: HtmlScraperSelectMode,
    /// Actions applied to the selected request URLs.
    ///
    /// Transformations applied to the extracted URLs before making the request.
    pub(crate) request_actions: Vec<ScraperAction>,
    /// HTTP method used for the follow-up request.
    ///
    /// Either GET or POST.
    pub(crate) request_method: ScraperRequestMethod,
    /// HTTP headers attached to the follow-up request.
    pub(crate) request_headers: Vec<ScraperRequestHeader>,
    /// HTTP client configuration for the follow-up request.
    ///
    /// Settings like user agent, max redirects, timeouts, etc.
    pub(crate) http_config: ScraperHttpConfig,
    /// CSS selector matching each result row in the response.
    ///
    /// This selector is used to find individual data rows in the HTML response
    /// fetched by the sub-query.
    pub(crate) row_selector: String,
    /// Compiled CSS selector for efficient matching.
    ///
    /// The parsed and validated version of `row_selector` for efficient use.
    #[allow(dead_code)]
    pub(crate) row_selector_compiled: ::scraper::Selector,
    /// Field extractors executed for every matched row.
    ///
    /// Entries that define how to extract data from each row matched by the
    /// row selector in the follow-up response.
    pub(crate) entries: Vec<HtmlScraperEntry>,
    /// Post-processing steps applied to each extracted row.
    ///
    /// Transformations applied to extracted rows after extraction.
    pub(crate) post_processes: Vec<ScraperPostProcess>,
    /// Nested sub-queries (recursion). Polymorphic slot allowing
    /// heterogeneous composition at the entry level.
    ///
    /// Child sub-queries that are executed for each row matched by this sub-query,
    /// enabling arbitrary levels of nested data fetching.
    pub(crate) sub_queries: Vec<Box<dyn ScraperQuery>>,
    /// HTTP client for follow-up requests.
    ///
    /// The HTTP client instance used to make follow-up requests.
    pub(crate) http_client: HttpClient,
}

impl ScraperQuery for HtmlScraperQuery {
    /// Returns the scraper type identifier.
    ///
    /// # Returns
    ///
    /// Always returns [`ScraperType::Html`] for HTML scraper queries.
    fn scraper_type(&self) -> ScraperType {
        ScraperType::Html
    }

    /// Returns the query name.
    ///
    /// # Returns
    ///
    /// The query identifier as a string slice.
    fn name(&self) -> &str {
        &self.name
    }

    /// Returns the media types produced by this query.
    ///
    /// # Returns
    ///
    /// A slice of media type strings.
    fn media_types(&self) -> &[String] {
        &self.media_types
    }

    /// Returns the base URL of the source.
    ///
    /// # Returns
    ///
    /// The resolved base URL string.
    fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Returns the URL template used to build the request.
    ///
    /// # Returns
    ///
    /// The query URL template string.
    fn query_url(&self) -> &str {
        &self.query_url
    }

    fn request_url_actions(&self) -> &[ScraperAction] {
        &self.request_url_actions
    }

    /// Returns the HTTP method used for requests.
    ///
    /// # Returns
    ///
    /// The request method (GET or POST).
    fn request_method(&self) -> ScraperRequestMethod {
        self.request_method
    }

    /// Returns the JSON pointer for selecting the request URL or body.
    ///
    /// For root queries, returns `request_body_pointer`.
    /// For sub-queries, returns `request_pointer` (prioritaire) or falls back to `request_body_pointer`.
    ///
    /// # Returns
    ///
    /// The request pointer if configured, otherwise `None`.
    fn request_pointer(&self) -> Option<&str> {
        self.request_pointer
            .as_deref()
            .or(self.request_body_pointer.as_deref())
    }

    /// Returns the selection mode for the request pointer.
    ///
    /// For sub-queries, `request_select` est prioritaire.
    ///
    /// # Returns
    ///
    /// The selection mode controlling how request values are selected.
    fn request_select(&self) -> HtmlScraperSelectMode {
        if self.request_pointer.is_some() {
            self.request_select
        } else {
            self.request_body_select
        }
    }

    /// Returns the actions applied to the request.
    ///
    /// For sub-queries, `request_sub_actions` est prioritaire si non vide.
    ///
    /// # Returns
    ///
    /// A slice of actions applied before the HTTP call.
    fn request_actions(&self) -> &[ScraperAction] {
        if self.request_pointer.is_some() && !self.request_sub_actions.is_empty() {
            &self.request_sub_actions
        } else {
            &self.request_body_actions
        }
    }

    fn request_body_pointer(&self) -> Option<&str> {
        self.request_body_pointer.as_deref()
    }

    fn request_body_select(&self) -> HtmlScraperSelectMode {
        self.request_body_select
    }

    fn request_body_actions(&self) -> &[ScraperAction] {
        &self.request_body_actions
    }

    /// Returns the HTTP headers attached to the request.
    ///
    /// # Returns
    ///
    /// A slice of request headers.
    fn request_headers(&self) -> &[ScraperRequestHeader] {
        &self.request_headers
    }

    /// Returns the HTTP client configuration.
    ///
    /// # Returns
    ///
    /// A reference to the HTTP configuration.
    fn http_config(&self) -> &ScraperHttpConfig {
        &self.http_config
    }

    /// Returns the HTTP client used for requests.
    ///
    /// # Returns
    ///
    /// A reference to the HTTP client instance.
    fn http_client(&self) -> &HttpClient {
        &self.http_client
    }

    /// Returns whether this query should extract next page data.
    ///
    /// # Returns
    ///
    /// `false` for HTML scraper queries (pagination not supported at query level).
    fn extract_next_data(&self) -> bool {
        false
    }

    /// Returns the row locator for this query.
    ///
    /// # Returns
    ///
    /// A [`RowLocator::Selector`] using the row selector template.
    fn row_locator(&self) -> RowLocator {
        RowLocator::Selector {
            selector: self.row_selector_template.clone(),
            select: HtmlScraperSelectMode::All,
        }
    }

    /// Returns the post-processing steps for this query.
    ///
    /// # Returns
    ///
    /// A slice of post-process configurations.
    fn post_processes(&self) -> &[crate::scrapyfy::ScraperPostProcess] {
        &self.post_processes
    }

    fn input_html(&self) -> Option<&str> {
        self.input_html.as_deref()
    }

    /// Returns the entries for this query.
    ///
    /// # Returns
    ///
    /// A vector of references to all configured entries as trait objects.
    fn entries(&self) -> Vec<&dyn ScraperEntrySpec> {
        self.scraper_entries
            .iter()
            .map(|entry| entry as &dyn ScraperEntrySpec)
            .collect()
    }

    /// Returns the sub-queries for this query.
    ///
    /// # Returns
    ///
    /// An empty vector - HTML scraper queries don't have root-level sub-queries.
    fn sub_queries(&self) -> Vec<&dyn ScraperQuery> {
        self.sub_queries
            .iter()
            .map(|sub| &**sub as &dyn ScraperQuery)
            .collect()
    }

    // --- Sub-query overrides ---

    fn context_pointer(&self) -> Option<&str> {
        self.context_pointer.as_deref()
    }

    fn context_select(&self) -> HtmlScraperSelectMode {
        self.context_select
    }

    fn context_entries(&self) -> Vec<&dyn ScraperEntrySpec> {
        // No dedicated context_entries on HtmlScraperQuery yet
        Vec::new()
    }

    fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    /// Returns a reference to this query as a trait object.
    ///
    /// # Returns
    ///
    /// A reference to this query as `&dyn Any`, allowing downcasting.
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl ScraperQuery for HtmlScraperSubQuery {
    /// Returns the scraper type identifier.
    ///
    /// # Returns
    ///
    /// Always returns [`ScraperType::Html`] for HTML sub-queries.
    fn scraper_type(&self) -> ScraperType {
        ScraperType::Html
    }

    /// Returns the sub-query name.
    ///
    /// # Returns
    ///
    /// A static string "html-sub-query" identifying this as a sub-query.
    fn name(&self) -> &str {
        "html-sub-query"
    }

    /// Returns the media types produced by this sub-query.
    ///
    /// # Returns
    ///
    /// An empty slice - sub-queries don't produce media types directly.
    fn media_types(&self) -> &[String] {
        &[]
    }

    /// Returns whether this sub-query matches any of the requested media types.
    ///
    /// # Arguments
    ///
    /// * `_media_types` - Requested media types to match against.
    ///
    /// # Returns
    ///
    /// Always returns `true` for HTML sub-queries.
    fn is_media_type(&self, _media_types: &[String]) -> bool {
        true
    }

    /// Returns the base URL of the source.
    ///
    /// # Returns
    ///
    /// An empty string - sub-queries don't have their own base URL.
    fn base_url(&self) -> &str {
        ""
    }

    /// Returns the URL template used to build the request.
    ///
    /// # Returns
    ///
    /// An empty string - sub-queries derive their URL from context rows.
    fn query_url(&self) -> &str {
        ""
    }

    /// Returns the HTTP method used for the follow-up request.
    ///
    /// # Returns
    ///
    /// The configured request method (GET or POST).
    fn request_method(&self) -> ScraperRequestMethod {
        self.request_method
    }

    /// Returns the JSON pointer for selecting the request URL.
    ///
    /// # Returns
    ///
    /// The request pointer if configured, otherwise `None`.
    fn request_pointer(&self) -> Option<&str> {
        self.request_pointer.as_deref()
    }

    /// Returns the selection mode for the request pointer.
    ///
    /// # Returns
    ///
    /// The selection mode controlling how request URLs are selected from context.
    fn request_select(&self) -> HtmlScraperSelectMode {
        self.request_select
    }

    /// Returns the actions applied to the selected request URLs.
    ///
    /// # Returns
    ///
    /// A slice of actions applied to extracted URLs before making the request.
    fn request_actions(&self) -> &[ScraperAction] {
        &self.request_actions
    }

    /// Returns the HTTP headers attached to the follow-up request.
    ///
    /// # Returns
    ///
    /// A slice of request headers.
    fn request_headers(&self) -> &[ScraperRequestHeader] {
        &self.request_headers
    }

    /// Returns the HTTP client configuration for the follow-up request.
    ///
    /// # Returns
    ///
    /// A reference to the HTTP configuration.
    fn http_config(&self) -> &ScraperHttpConfig {
        &self.http_config
    }

    /// Returns the HTTP client used for follow-up requests.
    ///
    /// # Returns
    ///
    /// A reference to the HTTP client instance.
    fn http_client(&self) -> &HttpClient {
        &self.http_client
    }

    /// Returns whether this sub-query should extract next page data.
    ///
    /// # Returns
    ///
    /// `false` for HTML sub-queries.
    fn extract_next_data(&self) -> bool {
        false
    }

    /// Returns the row locator for this sub-query.
    ///
    /// # Returns
    ///
    /// A [`RowLocator::Selector`] using the compiled row selector.
    fn row_locator(&self) -> RowLocator {
        RowLocator::Selector {
            selector: self.row_selector.clone(),
            select: HtmlScraperSelectMode::All,
        }
    }

    /// Returns the post-processing steps for this sub-query.
    ///
    /// # Returns
    ///
    /// A slice of post-process configurations.
    fn post_processes(&self) -> &[crate::scrapyfy::ScraperPostProcess] {
        &self.post_processes
    }

    /// Returns the entries for this sub-query.
    ///
    /// # Returns
    ///
    /// A vector of references to all configured entries (context entries + regular entries).
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

    /// Returns the sub-queries for this sub-query.
    ///
    /// # Returns
    ///
    /// A vector of references to all nested sub-queries.
    fn sub_queries(&self) -> Vec<&dyn ScraperQuery> {
        self.sub_queries
            .iter()
            .map(|sub| &**sub as &dyn ScraperQuery)
            .collect()
    }

    // --- Query-level sub-query overrides (legacy semantics) ---

    /// Returns the context pointer for selecting context rows.
    ///
    /// # Returns
    ///
    /// The JSON pointer if configured, otherwise `None`.
    fn context_pointer(&self) -> Option<&str> {
        self.context_pointer.as_deref()
    }

    /// Returns the selection mode for the context pointer.
    ///
    /// # Returns
    ///
    /// The selection mode controlling how context rows are selected.
    fn context_select(&self) -> HtmlScraperSelectMode {
        self.context_select
    }

    /// Returns the context entries for this sub-query.
    ///
    /// # Returns
    ///
    /// A vector of references to all context entries as trait objects.
    fn context_entries(&self) -> Vec<&dyn ScraperEntrySpec> {
        self.context_entries
            .iter()
            .map(|entry| entry as &dyn ScraperEntrySpec)
            .collect()
    }

    /// Returns the target path where results are nested.
    ///
    /// # Returns
    ///
    /// The target path if configured, otherwise `None`.
    fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    /// Returns a reference to this sub-query as a trait object.
    ///
    /// # Returns
    ///
    /// A reference to this sub-query as `&dyn Any`, allowing downcasting.
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
