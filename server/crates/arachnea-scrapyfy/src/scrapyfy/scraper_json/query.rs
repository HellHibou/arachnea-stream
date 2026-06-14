//! Runtime JSON query and sub-query types.
//!
//! This module defines the validated runtime types [`JsonScraperQuery`] and
//! [`JsonScraperSubQuery`], along with their execution logic (HTTP fetch,
//! row extraction, entry application, sub-query recursion).  Raw YAML
//! configuration types and conversions live in [`super::config`] and are
//! re-exported here so that existing import paths continue to work.

// Re-export shared types from scraper::config for backward compatibility.
// Types like ScraperRequestMethod, ScraperRequestHeader, etc. are now defined
// in crate::scrapyfy::scraper::config but re-exported here so that existing
// import paths like `use crate::scrapyfy::scraper_json::query::{ScraperRequestMethod, …}`
// continue to work.
pub(crate) use super::config::JsonScraperExecutionOptions;
pub use super::config::{EntrySubQueryRaw, JsonScraperQueryRaw, JsonScraperSubQueryRaw};

// Re-export from scraper::config for backward compatibility
pub use crate::scrapyfy::scraper::config::{
    ScraperRequestHeader, ScraperRequestHeaderRaw, ScraperRequestMethod,
};

use std::any::Any;

use anyhow::Result;
use futures::stream::{self, StreamExt};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

use crate::scrapyfy::query_helpers::{self, QueryTemplateParamMapping};
use crate::scrapyfy::scraper::entry_trait::ScraperEntrySpec;
use crate::scrapyfy::scraper::query_trait::ScraperQuery;
use crate::scrapyfy::scraper::query_unified::{ScraperQueryConfig, ScraperTypeConfig};
use crate::scrapyfy::scraper::row_locator::{RowLocator, ScraperType};
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;
use crate::scrapyfy::scraper_json::entry::{json_value_to_strings, select_json_values};
use crate::scrapyfy::scraper_json::response_parser::{collect_ordered_results, matches};
use crate::scrapyfy::*;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A query definition for one JSON source and one result shape.
///
/// Represents a validated JSON scraping query that can fetch data from a JSON endpoint
/// and extract structured information using JSON pointers.
#[derive(Deserialize)]
#[serde(try_from = "JsonScraperQueryRaw")]
pub struct JsonScraperQuery {
    /// Query identifier used as the lookup key in a collection.
    pub(crate) name: String,

    /// Resolved base URL of the source.
    pub(crate) base_url: String,

    /// Original base URL template before parameter resolution.
    pub(crate) base_url_template: String,

    /// Content types this query produces.
    pub(crate) media_types: Vec<String>,

    /// URL template used to build the request.
    pub(crate) query_url: String,

    /// When `true`, the response is parsed as Next.js `__NEXT_DATA__` payload.
    pub(crate) extract_next_data: bool,

    /// HTTP method used to issue the request (GET or POST).
    pub(crate) request_method: ScraperRequestMethod,

    /// Optional JSON pointer selecting the request body from runtime params.
    pub(crate) request_body_pointer: Option<String>,

    /// Selection mode for the request body pointer.
    pub(crate) request_body_select: HtmlScraperSelectMode,

    /// Actions applied to the request body before the HTTP call.
    pub(crate) request_body_actions: Vec<ScraperAction>,

    /// HTTP headers attached to the request.
    pub(crate) request_headers: Vec<ScraperRequestHeader>,

    /// HTTP client configuration (timeouts, user agent, etc.).
    pub(crate) http_config: ScraperHttpConfig,

    /// Source-to-target parameter mappings applied before template resolution.
    pub(crate) query_param_mappings: Vec<QueryTemplateParamMapping>,

    /// Maximum number of sibling sub-queries executed concurrently.
    pub(crate) sibling_sub_query_concurrency: usize,

    /// Maximum number of sub-query contexts executed concurrently.
    pub(crate) sub_query_context_concurrency: usize,

    /// Maximum number of follow-up HTTP requests executed concurrently.
    pub(crate) sub_query_fetch_concurrency: usize,

    /// Root-level filters applied to the entire JSON response.
    pub(crate) filters: HashMap<String, Vec<String>>,

    /// JSON pointer-like path matching each result row.
    pub(crate) row_pointer: String,

    /// Optional group field whose items should become query rows.
    pub(crate) result_item_field: Option<String>,

    /// Field extractors executed for every matched row.
    pub(crate) scraper_entries: Vec<JsonScraperEntry>,

    /// Top-level sub-queries attached to this query. Polymorphic slot:
    /// a typical YAML config attaches only [`JsonScraperSubQuery`] children,
    /// but the trait object slot allows future heterogeneous composition.
    pub(crate) sub_queries: Vec<Box<dyn ScraperQuery>>,

    /// Post-processing steps applied to each extracted row.
    pub(crate) post_processes: Vec<ScraperPostProcess>,

    /// HTTP client for making requests.
    pub(crate) http_client: HttpClient,

    // --- Champs sub-query (optionnels, utilisés quand ce query est une sub-query) ---
    /// Optional JSON pointer selecting the context rows that seed follow-up requests.
    pub(crate) context_pointer: Option<String>,

    /// Selection mode for the context pointer.
    pub(crate) context_select: HtmlScraperSelectMode,

    /// Filters applied to the context row before issuing the follow-up request.
    pub(crate) context_filters: HashMap<String, Vec<String>>,

    /// Filters applied to the fetched rows (after the HTTP response).
    pub(crate) row_filters: HashMap<String, Vec<String>>,

    /// Path where the sub-query result is nested.
    pub(crate) target: Option<String>,

    /// Pointer selecting the request URL from the context row.
    pub(crate) request_pointer: Option<String>,

    /// Selection mode for the request pointer.
    pub(crate) request_select: HtmlScraperSelectMode,

    /// Actions applied to the selected request URLs.
    pub(crate) request_sub_actions: Vec<ScraperAction>,
}

// ---------------------------------------------------------------------------
// JsonScraperSubQuery
// ---------------------------------------------------------------------------

/// Runtime definition of one chained JSON follow-up request.
///
/// Represents a validated sub-query that can make follow-up HTTP requests to extract
/// additional JSON data based on values from parent rows.
#[derive(Deserialize)]
#[serde(try_from = "JsonScraperSubQueryRaw")]
pub struct JsonScraperSubQuery {
    /// Optional JSON pointer selecting the context rows that seed follow-up requests.
    pub(crate) context_pointer: Option<String>,

    /// Selection mode for the context pointer.
    pub(crate) context_select: HtmlScraperSelectMode,

    /// Filters applied to the context row before issuing the follow-up request.
    pub(crate) filters: HashMap<String, Vec<String>>,

    /// Filters applied to the fetched rows (after the HTTP response).
    pub(crate) row_filters: HashMap<String, Vec<String>>,

    /// Entries extracted from the context row rather than from fetched rows.
    pub(crate) context_entries: Vec<JsonScraperEntry>,

    /// Path where the sub-query result is nested.
    ///
    /// Path format: `"parent>child>grandchild"` creates nested objects.
    /// `None` means results are merged at the top level.
    pub(crate) target: Option<String>,

    /// Pointer selecting the request URL from the context row.
    pub(crate) request_pointer: Option<String>,

    /// Selection mode for the request pointer.
    pub(crate) request_select: HtmlScraperSelectMode,

    /// Actions applied to the selected request URLs.
    pub(crate) request_actions: Vec<ScraperAction>,

    /// HTTP method used for the follow-up request.
    pub(crate) request_method: ScraperRequestMethod,

    /// Optional JSON pointer selecting the request body from the context.
    pub(crate) request_body_pointer: Option<String>,

    /// Selection mode for the request body pointer.
    pub(crate) request_body_select: HtmlScraperSelectMode,

    /// Actions applied to the request body values.
    pub(crate) request_body_actions: Vec<ScraperAction>,

    /// When `true`, the follow-up response is parsed as Next.js `__NEXT_DATA__`.
    pub(crate) extract_next_data: bool,

    /// HTTP headers attached to the follow-up request.
    pub(crate) request_headers: Vec<ScraperRequestHeader>,

    /// HTTP client configuration for the follow-up request.
    pub(crate) http_config: ScraperHttpConfig,

    /// JSON pointer matching each result row in the follow-up response.
    pub(crate) row_pointer: String,

    /// Field extractors executed for every matched row.
    pub(crate) entries: Vec<JsonScraperEntry>,

    /// Post-processing steps applied to each extracted row.
    pub(crate) post_processes: Vec<ScraperPostProcess>,

    /// Nested sub-queries (recursion). Polymorphic: typically a
    /// [`JsonScraperSubQuery`], but the trait object slot allows
    /// heterogeneous composition at the entry level.
    pub(crate) sub_queries: Vec<Box<dyn ScraperQuery>>,

    /// HTTP client for follow-up requests.
    #[serde(skip)]
    pub(crate) http_client: HttpClient,
}

impl JsonScraperSubQuery {
    /// Returns the flattened list of leaf field names produced by this sub-query tree.
    ///
    /// Only the [`JsonScraperSubQuery`] children contribute to the list; nested
    /// sub-queries of other concrete types (e.g. HTML) are silently skipped
    /// because they are not part of the JSON sub-query field-name contract.
    #[cfg(any(test, feature = "test-support"))]
    pub fn field_names(&self) -> Vec<String> {
        let mut names = self
            .entries
            .iter()
            .flat_map(JsonScraperEntry::field_names)
            .collect::<Vec<_>>();

        names.extend(
            self.context_entries
                .iter()
                .flat_map(JsonScraperEntry::field_names),
        );

        for sub_query in &self.sub_queries {
            if let Some(json_sub) = sub_query.as_any().downcast_ref::<JsonScraperSubQuery>() {
                names.extend(json_sub.field_names());
            }
        }

        names
    }

    /// Validates every action against the sub-query-level contract.
    ///
    /// # Arguments
    ///
    /// * `name` - Identifier used in diagnostic messages.
    /// * `actions` - Actions to validate.
    ///
    /// # Errors
    ///
    /// Returns an error if any action fails validation for the `"sub-query"` context.
    pub(crate) fn validate_actions(name: &str, actions: &[ScraperAction]) -> Result<()> {
        for action in actions {
            action.validate(name, "sub-query")?;
        }

        Ok(())
    }

    /// Splits a `>`-separated target path into trimmed path segments.
    ///
    /// # Arguments
    ///
    /// * `target` - Hierarchical target path string.
    fn split_target_path(target: &str) -> Vec<&str> {
        target
            .split('>')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// JsonScraperQuery
// ---------------------------------------------------------------------------

impl JsonScraperQuery {
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

    /// Creates a query and panics if the row pointer is invalid.
    ///
    /// # Arguments
    ///
    /// * `name` - Query identifier used to look up this query in a collection.
    /// * `base_url` - Base URL of the source, also available as `{base_url}` in templates.
    /// * `media_types` - Content type result.
    /// * `query_url` - URL template used to build the request.
    /// * `row_pointer` - JSON pointer-like path matching each result row.
    /// * `scraper_entries` - Field extractors executed for every matched row.
    /// * `sub_queries` - Polymorphic sub-queries attached to this query. Each
    ///   element is boxed and stored as `Box<dyn ScraperQuery>`; callers
    ///   typically pass `Vec<JsonScraperSubQuery>` values boxed individually.
    ///
    /// # Panics
    ///
    /// Panics if `row_pointer` is empty.
    #[allow(dead_code)]
    pub fn new(
        name: &str,
        base_url: &str,
        media_types: Vec<String>,
        query_url: &str,
        row_pointer: &str,
        scraper_entries: Vec<JsonScraperEntry>,
        sub_queries: Vec<Box<dyn ScraperQuery>>,
    ) -> Self {
        Self::try_new(
            name,
            base_url,
            media_types,
            query_url,
            4,
            4,
            8,
            HashMap::new(),
            row_pointer,
            scraper_entries,
            sub_queries,
        )
        .unwrap_or_else(|_| panic!("Invalid format: {}", row_pointer))
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

    /// Returns the JSON pointer-like path matching each result row.
    pub fn row_pointer(&self) -> &str {
        &self.row_pointer
    }

    /// Returns the optional group field whose items should become query rows.
    pub fn result_item_field(&self) -> Option<&str> {
        self.result_item_field.as_deref()
    }

    /// Returns the configured field extractors executed for every matched row.
    pub fn scraper_entries(&self) -> &Vec<JsonScraperEntry> {
        &self.scraper_entries
    }

    /// Returns the chained JSON follow-up requests executed after the main entries.
    pub fn sub_queries(&self) -> &[Box<dyn ScraperQuery>] {
        &self.sub_queries
    }

    /// Validates the row pointer and stores the configured field extractors.
    ///
    /// # Arguments
    ///
    /// * `name` - Query identifier used to look up this query in a collection.
    /// * `base_url` - Base URL of the source, also available as `{base_url}` in templates.
    /// * `media_types` - Content type result.
    /// * `query_url` - URL template used to build the request.
    /// * `sibling_sub_query_concurrency` - Max number of sibling sub-queries executed together.
    /// * `sub_query_context_concurrency` - Max number of sub-query contexts executed together.
    /// * `sub_query_fetch_concurrency` - Max number of follow-up HTTP requests executed together.
    /// * `row_pointer` - JSON pointer-like path matching each result row.
    /// * `scraper_entries` - Field extractors executed for every matched row.
    /// * `sub_queries` - Polymorphic sub-queries attached to this query. Each
    ///   element is stored as `Box<dyn ScraperQuery>`; callers typically
    ///   pass `Vec<JsonScraperSubQuery>` values boxed individually.
    ///
    /// # Errors
    ///
    /// Returns an error if `row_pointer` is empty.
    pub fn try_new(
        name: &str,
        base_url: &str,
        media_types: Vec<String>,
        query_url: &str,
        sibling_sub_query_concurrency: usize,
        sub_query_context_concurrency: usize,
        sub_query_fetch_concurrency: usize,
        filters: HashMap<String, Vec<String>>,
        row_pointer: &str,
        scraper_entries: Vec<JsonScraperEntry>,
        sub_queries: Vec<Box<dyn ScraperQuery>>,
    ) -> Result<Self> {
        if row_pointer.trim().is_empty() {
            anyhow::bail!("Invalid row pointer for {}: pointer cannot be empty", name);
        }
        if sibling_sub_query_concurrency == 0
            || sub_query_context_concurrency == 0
            || sub_query_fetch_concurrency == 0
        {
            anyhow::bail!(
                "Invalid JSON concurrency for {}: concurrency values must be greater than 0",
                name
            );
        }

        let mut query = JsonScraperQuery {
            name: name.to_string(),
            base_url: base_url.to_string(),
            base_url_template: base_url.to_string(),
            media_types,
            http_client: HttpClient::new(base_url),
            http_config: ScraperHttpConfig::default(),
            query_url: query_url.to_string(),
            extract_next_data: false,
            request_method: ScraperRequestMethod::Get,
            request_body_pointer: None,
            request_body_select: HtmlScraperSelectMode::All,
            request_body_actions: Vec::new(),
            request_headers: Vec::new(),
            query_param_mappings: Vec::new(),
            sibling_sub_query_concurrency,
            sub_query_context_concurrency,
            sub_query_fetch_concurrency,
            filters,
            row_pointer: row_pointer.to_string(),
            result_item_field: None,
            scraper_entries: Vec::new(),
            sub_queries,
            post_processes: Vec::new(),
            // Champs sub-query (par défaut)
            context_pointer: None,
            context_select: HtmlScraperSelectMode::All,
            context_filters: HashMap::new(),
            row_filters: HashMap::new(),
            target: None,
            request_pointer: None,
            request_select: HtmlScraperSelectMode::All,
            request_sub_actions: Vec::new(),
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
    pub fn add_entry(&mut self, scraper_entry: JsonScraperEntry) -> &mut Self {
        self.scraper_entries.push(scraper_entry);
        self
    }

    /// Resolves the full set of request headers from templates and collection params.
    ///
    /// # Arguments
    ///
    /// * `params` - Runtime template parameters available for header value resolution.
    /// * `query_url` - Fully resolved request URL passed to action pipelines.
    fn resolve_request_headers(
        &self,
        params: &HashMap<String, String>,
        query_url: &str,
    ) -> HashMap<String, String> {
        let request_context = query_helpers::build_params_json_value(params);
        crate::scrapyfy::query_helpers::resolve_request_headers(
            &self.request_headers,
            &request_context,
            params,
            query_url,
        )
    }

    /// Builds the optional request body by selecting a JSON pointer value from the
    /// params context and applying the configured body actions.
    ///
    /// # Arguments
    ///
    /// * `params` - Runtime template parameters used for pointer selection and actions.
    /// * `query_url` - Fully resolved request URL passed to action pipelines.
    fn resolve_request_body(
        &self,
        params: &HashMap<String, String>,
        query_url: &str,
    ) -> Option<String> {
        let request_context = query_helpers::build_params_json_value(params);
        crate::scrapyfy::query_helpers::resolve_request_body(
            self.request_body_pointer.as_deref(),
            self.request_body_select,
            &self.request_body_actions,
            &request_context,
            params,
            query_url,
        )
    }

    /// Builds the concurrency options forwarded to sub-query execution.
    fn execution_options(&self) -> JsonScraperExecutionOptions {
        JsonScraperExecutionOptions {
            sibling_sub_query_concurrency: self.sibling_sub_query_concurrency,
            sub_query_context_concurrency: self.sub_query_context_concurrency,
            sub_query_fetch_concurrency: self.sub_query_fetch_concurrency,
        }
    }

    /// Returns the flattened list of leaf field names produced by this query.
    ///
    /// Only the [`JsonScraperSubQuery`] children contribute to the list; nested
    /// sub-queries of other concrete types (e.g. HTML) are silently skipped.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_field_names(&self) -> Vec<String> {
        let mut names = self
            .scraper_entries()
            .iter()
            .flat_map(JsonScraperEntry::field_names)
            .collect::<Vec<_>>();

        for sub_query in self.sub_queries() {
            if let Some(json_sub) = sub_query.as_any().downcast_ref::<JsonScraperSubQuery>() {
                names.extend(json_sub.field_names());
            }
        }

        names
    }
}

// ---------------------------------------------------------------------------
// ScraperQuery trait implementations
// ---------------------------------------------------------------------------

impl ScraperQuery for JsonScraperQuery {
    fn scraper_type(&self) -> ScraperType {
        ScraperType::Json
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
        // Pour une root query, request_pointer utilise request_body_pointer
        // Pour une sub-query, request_pointer est le champ dédié (prioritaire)
        self.request_pointer
            .as_deref()
            .or(self.request_body_pointer.as_deref())
    }

    fn request_select(&self) -> HtmlScraperSelectMode {
        // Pour sub-query, request_select est prioritaire
        if self.request_pointer.is_some() {
            self.request_select
        } else {
            self.request_body_select
        }
    }

    fn request_actions(&self) -> &[ScraperAction] {
        // Pour sub-query, request_sub_actions est prioritaire
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
        self.extract_next_data
    }

    fn row_locator(&self) -> RowLocator {
        RowLocator::Pointer(self.row_pointer.clone())
    }

    fn post_processes(&self) -> &[crate::scrapyfy::ScraperPostProcess] {
        &self.post_processes
    }

    fn result_item_field(&self) -> Option<&str> {
        self.result_item_field.as_deref()
    }

    fn entries(&self) -> Vec<&dyn ScraperEntrySpec> {
        self.scraper_entries
            .iter()
            .map(|entry| entry as &dyn ScraperEntrySpec)
            .collect()
    }

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
        // No dedicated context_entries on JsonScraperQuery yet;
        // context entries are merged into scraper_entries.
        Vec::new()
    }

    fn filters(&self) -> &HashMap<String, Vec<String>> {
        &self.filters
    }

    fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ScraperQuery for JsonScraperSubQuery {
    fn scraper_type(&self) -> ScraperType {
        ScraperType::Json
    }

    fn name(&self) -> &str {
        "json-sub-query"
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

    fn request_body_pointer(&self) -> Option<&str> {
        self.request_body_pointer.as_deref()
    }

    fn request_body_select(&self) -> HtmlScraperSelectMode {
        self.request_body_select
    }

    fn request_body_actions(&self) -> &[ScraperAction] {
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
        self.extract_next_data
    }

    fn row_locator(&self) -> RowLocator {
        RowLocator::Pointer(self.row_pointer.clone())
    }

    fn post_processes(&self) -> &[crate::scrapyfy::ScraperPostProcess] {
        &self.post_processes
    }

    fn result_item_field(&self) -> Option<&str> {
        None
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

    fn filters(&self) -> &HashMap<String, Vec<String>> {
        &self.filters
    }

    fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
