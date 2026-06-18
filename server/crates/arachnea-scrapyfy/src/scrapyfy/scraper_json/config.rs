//! Raw YAML configuration types and validated runtime conversions for JSON queries.
//!
//! This module defines the deserialization targets ([`JsonScraperQueryRaw`],
//! [`JsonScraperSubQueryRaw`], [`EntrySubQueryRaw`]) and the conversions between
//! raw YAML config and the validated runtime types.
//!
//! Shared types like [`ScraperRequestMethod`], [`ScraperRequestHeaderRaw`],
//! [`ScraperRequestHeader`] are defined in [`crate::scrapyfy::scraper::config`].
//! Bidirectional conversion between raw YAML config and the validated runtime
//! types is provided through `TryFrom`/`From`/`Serialize` implementations.
//!
//! The runtime query structs themselves live in [`super::query`].

use anyhow::Result;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;

use crate::scrapyfy::query_helpers;
use crate::scrapyfy::scraper::config::{
    default_select_mode, ScraperQueryCommon, ScraperRequestHeader, ScraperRequestHeaderRaw,
    SubQueryCommon,
};
use crate::scrapyfy::scraper_html::entry::{
    HtmlScraperEntry, HtmlScraperEntryRaw, HtmlScraperSelectMode,
};
use crate::scrapyfy::scraper_json::entry::JsonScraperEntryRaw;
use crate::scrapyfy::scraper_json::query::{JsonScraperQuery, JsonScraperSubQuery};
use crate::scrapyfy::{HttpClient, ScraperAction, ScraperHttpConfig};

// ---------------------------------------------------------------------------
// Default-value helpers
// ---------------------------------------------------------------------------

/// Default concurrency limit for sibling sub-query execution.
///
/// Controls how many sibling sub-queries can be executed concurrently.
/// Defaults to `4`.
fn default_json_sibling_sub_query_concurrency() -> usize {
    4
}

/// Default concurrency limit for sub-query context execution.
///
/// Controls how many contexts within a sub-query can be processed concurrently.
/// Defaults to `4`.
fn default_json_sub_query_context_concurrency() -> usize {
    4
}

/// Default concurrency limit for sub-query HTTP fetch operations.
///
/// Controls how many follow-up HTTP requests can be made concurrently within a sub-query.
/// Defaults to `8`.
fn default_json_sub_query_fetch_concurrency() -> usize {
    8
}

// ---------------------------------------------------------------------------
// Concurrency limits forwarded to sub-query execution.
// ---------------------------------------------------------------------------

/// Concurrency limits forwarded to sub-query execution.
///
/// Contains configuration parameters that control the parallelism of sub-query
/// operations during JSON scraping.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct JsonScraperExecutionOptions {
    /// Maximum number of sibling sub-queries executed concurrently.
    ///
    /// Controls parallelism when executing multiple sub-queries at the same level.
    pub sibling_sub_query_concurrency: usize,

    /// Maximum number of context values processed concurrently within a sub-query.
    ///
    /// Controls parallelism when a sub-query iterates over multiple context rows.
    pub sub_query_context_concurrency: usize,

    /// Maximum number of follow-up HTTP requests executed concurrently.
    ///
    /// Controls parallelism when fetching URLs for a single sub-query context.
    pub sub_query_fetch_concurrency: usize,
}

// ---------------------------------------------------------------------------
// Raw sub-query attached to an entry (YAML `sub_queries` field)
// ---------------------------------------------------------------------------

/// Raw configuration of one sub-query attached to an entry (YAML `sub_queries` field).
///
/// Tagged by `scraper_type` — only `html` and `json` are valid at the entry level.
/// This type is also used as the polymorphic child slot inside
/// [`JsonScraperSubQueryRaw`] so that entry-level heterogeneous nesting
/// (a JSON sub-query containing an HTML child, etc.) is possible.
#[derive(Serialize, Deserialize)]
#[serde(tag = "scraper_type", rename_all = "snake_case")]
pub enum EntrySubQueryRaw {
    /// HTML follow-up request seeded by the parent entry value.
    ///
    /// Executes an HTML scrape on the follow-up response, extracting data using CSS selectors.
    Html {
        /// Common sub-query configuration fields shared with JSON sub-queries.
        ///
        /// Includes HTTP settings, filtering, targeting, and request configuration.
        #[serde(flatten)]
        common: SubQueryCommon,

        /// CSS selector matching each result row in the follow-up response.
        ///
        /// Used to locate individual data rows in the HTML response.
        #[serde(default)]
        row_selector: String,

        /// Field extractors executed for every matched row.
        ///
        /// Defines how to extract data from each HTML row matched by the selector.
        #[serde(default)]
        entries: Vec<HtmlScraperEntryRaw>,

        /// Entries extracted from the context row rather than from fetched rows.
        ///
        /// Extracts data from the parent context instead of from the fetched HTML.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        context_entries: Vec<HtmlScraperEntryRaw>,

        /// Nested sub-queries (recursion).
        ///
        /// Child sub-queries that are executed for each row in the parent query.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        sub_queries: Vec<EntrySubQueryRaw>,
    },

    /// JSON follow-up request seeded by the parent entry value.
    ///
    /// Executes a JSON scrape on the follow-up response, extracting data using JSON pointers.
    Json {
        /// Common sub-query configuration fields shared with HTML sub-queries.
        ///
        /// Includes HTTP settings, filtering, targeting, and request configuration.
        #[serde(flatten)]
        common: SubQueryCommon,

        /// JSON pointer matching each result row in the follow-up response.
        ///
        /// JSON pointer or pointer-like path used to locate individual data rows in the JSON response.
        #[serde(default)]
        row_pointer: String,

        /// Field extractors executed for every matched row.
        ///
        /// Defines how to extract data from each JSON row matched by the pointer.
        #[serde(default)]
        entries: Vec<JsonScraperEntryRaw>,

        /// Entries extracted from the context row rather than from fetched rows.
        ///
        /// Extracts data from the parent context instead of from the fetched JSON.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        context_entries: Vec<JsonScraperEntryRaw>,

        /// Optional JSON pointer selecting the request body from the context.
        ///
        /// When set, extracts the POST request body from the context row using this pointer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_body_pointer: Option<String>,

        /// Selection mode for the request body pointer.
        ///
        /// Determines whether to use the first match or all matches from the body pointer:
        /// - [`HtmlScraperSelectMode::First`]: Only the first matching value.
        /// - [`HtmlScraperSelectMode::All`]: All matching values.
        #[serde(default = "default_select_mode")]
        request_body_select: HtmlScraperSelectMode,

        /// Actions applied to the request body values.
        ///
        /// Transformations applied to the request body before sending.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        request_body_actions: Vec<ScraperAction>,

        /// When `true` the follow-up response is parsed as Next.js `__NEXT_DATA__`.
        ///
        /// Enables automatic extraction of data from Next.js server-side rendered pages.
        #[serde(default)]
        extract_next_data: bool,

        /// Nested sub-queries (recursion).
        ///
        /// Child sub-queries that are executed for each row in the parent query.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        sub_queries: Vec<EntrySubQueryRaw>,
    },
}

impl EntrySubQueryRaw {
    /// Propagates the parent HTTP configuration into this entry sub-query and its children.
    ///
    /// # Arguments
    ///
    /// * `parent_http` - HTTP configuration inherited from the parent query.
    pub(crate) fn apply_parent_http(&mut self, parent_http: &ScraperHttpConfig) {
        match self {
            Self::Html {
                ref mut common,
                ref mut sub_queries,
                ..
            }
            | Self::Json {
                ref mut common,
                ref mut sub_queries,
                ..
            } => {
                common.apply_parent_http(parent_http);
                let child_http = common.http.clone();
                for child in sub_queries.iter_mut() {
                    child.apply_parent_http(&child_http);
                }
            }
        }
    }

    /// Resolves collection-level placeholders in this entry sub-query's HTTP config.
    ///
    /// # Arguments
    ///
    /// * `parent_name` - Parent query name used in diagnostic messages.
    /// * `params` - Collection-level template parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if a required placeholder is missing from `params`.
    pub(crate) fn resolve_collection_params(
        &mut self,
        parent_name: &str,
        params: &HashMap<String, String>,
    ) -> Result<()> {
        match self {
            Self::Html {
                ref mut common,
                ref mut sub_queries,
                ..
            } => {
                common.resolve_collection_params("HTML entry sub-query", parent_name, params)?;
                for child in sub_queries.iter_mut() {
                    child.resolve_collection_params(parent_name, params)?;
                }
            }
            Self::Json {
                ref mut common,
                ref mut sub_queries,
                ..
            } => {
                common.resolve_collection_params("JSON entry sub-query", parent_name, params)?;
                for child in sub_queries.iter_mut() {
                    child.resolve_collection_params(parent_name, params)?;
                }
            }
        }
        Ok(())
    }

    /// Converts this raw sub-query into a polymorphic `Box<dyn ScraperQuery>`.
    ///
    /// # Arguments
    ///
    /// * `base_url` - Base URL used to build follow-up request URLs.
    ///
    /// # Errors
    ///
    /// Returns an error if the entries cannot be converted or no selector/pointer is provided.
    pub(crate) fn into_boxed_query(
        self,
        base_url: &str,
    ) -> Result<Box<dyn crate::scrapyfy::scraper::query_trait::ScraperQuery>> {
        match self {
            Self::Html {
                common,
                row_selector,
                entries,
                context_entries,
                sub_queries,
            } => {
                if row_selector.is_empty() {
                    anyhow::bail!("HTML entry sub-query must define a row_selector");
                }
                let entries: Vec<HtmlScraperEntry> = entries
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<Vec<_>>>()?;
                let context_entries: Vec<HtmlScraperEntry> = context_entries
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<Vec<_>>>()?;
                let request_headers: Vec<ScraperRequestHeader> = common
                    .request_headers
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<Vec<_>>>()?;
                let sub_queries: Vec<Box<dyn crate::scrapyfy::scraper::query_trait::ScraperQuery>> =
                    sub_queries
                        .into_iter()
                        .map(|raw| raw.into_boxed_query(base_url))
                        .collect::<Result<Vec<_>>>()?;

                let SubQueryCommon {
                    post_process,
                    context_pointer,
                    context_select,
                    filters,
                    row_filters,
                    target,
                    request_pointer,
                    request_select,
                    request_actions,
                    request_method,
                    http,
                    ..
                } = common;

                // Créer l'entrée HTML et tout mettre dans un seul vecteur d'entries
                let mut all_entries: Vec<crate::scrapyfy::scraper_html::entry::HtmlScraperEntry> =
                    context_entries;
                all_entries.extend(entries);

                // Validation du row_selector
                if row_selector.is_empty() {
                    anyhow::bail!("HTML entry sub-query must define a row_selector");
                }
                let row_selector_compiled =
                    ::scraper::Selector::parse(&row_selector).map_err(|e| {
                        anyhow::anyhow!("Invalid row_selector '{}': {}", row_selector, e)
                    })?;
                let row_selector = if row_selector.starts_with('.') || row_selector.starts_with('#')
                {
                    row_selector
                } else if row_selector.starts_with("//") {
                    row_selector
                } else {
                    row_selector
                };

                let query = crate::scrapyfy::scraper_html::query::HtmlScraperQuery {
                    name: "html-sub-query".to_string(),
                    base_url: base_url.to_string(),
                    base_url_template: base_url.to_string(),
                    media_types: Vec::new(),
                    query_url: String::new(),
                    request_method,
                    request_body_pointer: None,
                    request_body_select:
                        crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode::All,
                    request_body_actions: Vec::new(),
                    request_headers,
                    http_config: http,
                    query_param_mappings: Vec::new(),
                    row_concurrency: 4,
                    row_selector_compiled,
                    row_selector_template: row_selector,
                    result_item_field: None,
                    scraper_entries: all_entries,
                    post_processes: post_process,
                    http_client: HttpClient::new(base_url),
                    // Sub-query specific fields
                    context_pointer,
                    context_select,
                    context_filters: filters,
                    row_filters,
                    target,
                    request_pointer,
                    request_select,
                    request_sub_actions: request_actions,
                    sub_queries,
                };
                Ok(Box::new(query))
            }
            Self::Json {
                common,
                row_pointer,
                entries,
                context_entries,
                request_body_pointer,
                request_body_select,
                request_body_actions,
                extract_next_data,
                sub_queries,
            } => {
                if row_pointer.is_empty() {
                    anyhow::bail!("JSON entry sub-query must define a row_pointer");
                }
                let entries: Vec<crate::scrapyfy::scraper_json::entry::JsonScraperEntry> = entries
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<Vec<_>>>()?;
                let context_entries: Vec<crate::scrapyfy::scraper_json::entry::JsonScraperEntry> =
                    context_entries
                        .into_iter()
                        .map(TryInto::try_into)
                        .collect::<Result<Vec<_>>>()?;
                let request_headers: Vec<ScraperRequestHeader> = common
                    .request_headers
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<Vec<_>>>()?;
                let sub_queries: Vec<Box<dyn crate::scrapyfy::scraper::query_trait::ScraperQuery>> =
                    sub_queries
                        .into_iter()
                        .map(|raw| raw.into_boxed_query(base_url))
                        .collect::<Result<Vec<_>>>()?;

                let SubQueryCommon {
                    post_process,
                    context_pointer,
                    context_select,
                    filters,
                    row_filters,
                    target,
                    request_pointer,
                    request_select,
                    request_actions,
                    request_method,
                    http,
                    ..
                } = common;

                // Merge context_entries + entries into a single list
                let mut all_entries: Vec<crate::scrapyfy::scraper_json::entry::JsonScraperEntry> =
                    context_entries;
                all_entries.extend(entries);

                let mut query = crate::scrapyfy::scraper_json::query::JsonScraperQuery::try_new(
                    "json-sub-query",
                    base_url,
                    Vec::new(), // no media_types
                    "",         // no query_url (URL comes from request_pointer)
                    4,
                    4,
                    8,
                    filters,
                    &row_pointer,
                    all_entries,
                    sub_queries,
                )?;
                query.extract_next_data = extract_next_data;
                query.request_method = request_method;
                query.request_body_pointer = request_body_pointer;
                query.request_body_select = request_body_select;
                query.request_body_actions = request_body_actions;
                query.request_headers = request_headers;
                query.http_config = http;
                query.post_processes = post_process;
                // Sub-query specific fields
                query.context_pointer = context_pointer;
                query.context_select = context_select;
                query.row_filters = row_filters;
                query.target = target;
                query.request_pointer = request_pointer;
                query.request_select = request_select;
                query.request_sub_actions = request_actions;
                query.http_client = HttpClient::new(base_url);
                Ok(Box::new(query))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Raw YAML configuration types
// ---------------------------------------------------------------------------

/// Raw configuration definition of one JSON query endpoint.
///
/// Deserializes from YAML configuration to define a JSON scraping query.
/// Contains all settings needed to fetch JSON data and extract structured information.
#[derive(Serialize, Deserialize)]
pub struct JsonScraperQueryRaw {
    /// Common configuration fields shared with HTML scraper.
    ///
    /// Includes base URL, request configuration, HTTP settings, media types, etc.
    #[serde(flatten)]
    pub common: ScraperQueryCommon,

    /// When `true` the response is parsed as a Next.js `__NEXT_DATA__` payload.
    ///
    /// Enables automatic extraction of data from Next.js server-side rendered pages.
    /// The scraper will look for and parse the `__NEXT_DATA__` script tag content.
    #[serde(default)]
    pub extract_next_data: bool,

    /// Maximum number of sibling sub-queries executed concurrently (default: 4).
    ///
    /// Controls parallelism when executing multiple sub-queries at the same level.
    #[serde(default = "default_json_sibling_sub_query_concurrency")]
    pub sibling_sub_query_concurrency: usize,

    /// Maximum number of sub-query contexts executed concurrently (default: 4).
    ///
    /// Controls parallelism when a sub-query iterates over multiple context rows.
    #[serde(default = "default_json_sub_query_context_concurrency")]
    pub sub_query_context_concurrency: usize,

    /// Maximum number of follow-up HTTP requests executed concurrently (default: 8).
    ///
    /// Controls parallelism when fetching URLs for a single sub-query context.
    #[serde(default = "default_json_sub_query_fetch_concurrency")]
    pub sub_query_fetch_concurrency: usize,

    /// Root-level filters applied to the entire JSON response.
    ///
    /// Format: `{"field_name": ["allowed_value_1", "allowed_value_2", ...]}`.
    /// Only rows matching all filter conditions will be processed.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub filters: HashMap<String, Vec<String>>,

    /// JSON pointer-like path matching each result row.
    ///
    /// Path used to locate individual data rows in the JSON response.
    /// Supports pointer syntax with wildcards and array filters (e.g., `/data/*[status=active]`).
    pub row_pointer: String,

    /// Field extractors executed for every matched row.
    ///
    /// Defines the schema and extraction logic for each row matched by the row pointer.
    pub entries: Vec<JsonScraperEntryRaw>,

    /// Chained JSON follow-up requests executed after the main entries.
    ///
    /// Now uses `EntrySubQueryRaw` (tagged format with `scraper_type`) for
    /// all sub-queries. Requires `scraper_type: json` on each sub-query item.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub_queries: Vec<EntrySubQueryRaw>,
}

// ---------------------------------------------------------------------------
// Sub-query raw types and conversions (re-exported via query)
// ---------------------------------------------------------------------------

/// Raw configuration definition of one chained JSON follow-up request.
///
/// Same field set as [`EntrySubQueryRaw::Json`] but used at the query level
/// (YAML `sub_queries` field of a root query). Serialization round-trips
/// through this concrete type; runtime execution goes through
/// [`JsonScraperSubQuery`].
#[derive(Serialize, Deserialize)]
pub struct JsonScraperSubQueryRaw {
    /// Common configuration fields shared with EntrySubQueryRaw.
    ///
    /// Includes HTTP settings, filtering, targeting, and request configuration.
    #[serde(flatten)]
    pub common: SubQueryCommon,

    /// JSON pointer-like path matching each result row in the response.
    ///
    /// Path used to locate individual data rows in the JSON response.
    /// Supports pointer syntax with wildcards and array filters.
    pub row_pointer: String,

    /// Field extractors executed for every matched row.
    ///
    /// Defines how to extract data from each JSON row matched by the row pointer.
    pub entries: Vec<JsonScraperEntryRaw>,

    /// Entries extracted from the context row rather than from fetched rows.
    ///
    /// Extracts data from the parent context instead of from the fetched JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_entries: Vec<JsonScraperEntryRaw>,

    /// Optional JSON pointer selecting the request body from the context.
    ///
    /// When set, extracts the POST request body from the context row using this pointer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_body_pointer: Option<String>,

    /// Selection mode for the request body pointer.
    ///
    /// Determines whether to use the first match or all matches from the body pointer:
    /// - [`HtmlScraperSelectMode::First`]: Only the first matching value.
    /// - [`HtmlScraperSelectMode::All`]: All matching values.
    #[serde(default = "default_select_mode")]
    pub request_body_select: HtmlScraperSelectMode,

    /// Actions applied to the request body values.
    ///
    /// Transformations applied to the request body before sending.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_body_actions: Vec<ScraperAction>,

    /// When `true` the follow-up response is parsed as Next.js `__NEXT_DATA__`.
    ///
    /// Enables automatic extraction of data from Next.js server-side rendered pages.
    #[serde(default)]
    pub extract_next_data: bool,

    /// Nested sub-queries (recursion).
    ///
    /// Kept as `JsonScraperSubQueryRaw` (untagged) for backward compatibility
    /// with query-level sub-queries. Entry-level heterogeneous nesting uses
    /// [`EntrySubQueryRaw`] via entry `sub_queries` fields.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub_queries: Vec<JsonScraperSubQueryRaw>,
}

impl JsonScraperQueryRaw {
    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        self.common.name.clone()
    }

    /// Resolves collection-level placeholders used by this JSON query config.
    ///
    /// # Arguments
    ///
    /// * `params` - Collection-level template parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if a required placeholder is missing from `params`.
    pub(crate) fn resolve_collection_params(
        &mut self,
        params: &HashMap<String, String>,
    ) -> Result<()> {
        self.common
            .resolve_collection_params("JSON query", params)?;
        for sub_query in &mut self.sub_queries {
            sub_query.resolve_collection_params(&self.common.name, params)?;
        }
        Ok(())
    }

    /// Merges the collection-level HTTP configuration into this query and its sub-queries.
    ///
    /// # Arguments
    ///
    /// * `collection_http` - HTTP configuration inherited from the parent collection.
    pub(crate) fn apply_collection_http(&mut self, collection_http: &ScraperHttpConfig) {
        self.common.apply_collection_http(collection_http);
        for sub_query in &mut self.sub_queries {
            sub_query.apply_parent_http(&self.common.http);
        }
    }
}

// ---------------------------------------------------------------------------
// Conversions — Raw → Runtime → Raw
// ---------------------------------------------------------------------------

impl TryFrom<JsonScraperQueryRaw> for JsonScraperQuery {
    type Error = anyhow::Error;

    /// Converts a raw YAML query definition into a validated runtime query.
    ///
    /// # Errors
    ///
    /// Returns an error if the row pointer, entries, sub-queries, or request headers are invalid.
    fn try_from(config: JsonScraperQueryRaw) -> Result<Self> {
        let JsonScraperQueryRaw {
            common,
            extract_next_data,
            sibling_sub_query_concurrency,
            sub_query_context_concurrency,
            sub_query_fetch_concurrency,
            filters,
            row_pointer,
            entries,
            sub_queries,
        } = config;

        let ScraperQueryCommon {
            name,
            base_url,
            resolved_base_url,
            media_types,
            query_url,
            request_method,
            request_body_pointer,
            request_body_select,
            request_body_actions,
            request_headers,
            http,
            query_param_mappings,
            result_item_field,
            post_process,
            ..
        } = common;

        let resolved_base_url = query_helpers::resolved_or_template(&base_url, resolved_base_url);

        let entries = entries
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;
        let sub_queries: Vec<Box<dyn crate::scrapyfy::scraper::query_trait::ScraperQuery>> =
            sub_queries
                .into_iter()
                .map(|raw| raw.into_boxed_query(&resolved_base_url))
                .collect::<Result<Vec<_>>>()?;
        let request_headers = request_headers
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;

        JsonScraperQuery::validate_request_actions(&name, &request_body_actions)?;

        let mut query = JsonScraperQuery::try_new(
            &name,
            &resolved_base_url,
            media_types,
            &query_url,
            sibling_sub_query_concurrency,
            sub_query_context_concurrency,
            sub_query_fetch_concurrency,
            filters,
            &row_pointer,
            entries,
            sub_queries,
        )?;
        query.base_url_template = base_url;
        query.extract_next_data = extract_next_data;
        query.request_method = request_method;
        query.request_body_pointer = request_body_pointer;
        query.request_body_select = request_body_select;
        query.request_body_actions = request_body_actions;
        query.request_headers = request_headers;
        query.http_config = http.clone();
        query.http_client = HttpClient::with_http_config(http);
        query.query_param_mappings = query_param_mappings;
        query.result_item_field = result_item_field;

        for post_process in post_process {
            post_process.validate(&name)?;
            query.post_processes.push(post_process);
        }

        Ok(query)
    }
}

impl From<&JsonScraperQuery> for JsonScraperQueryRaw {
    /// Converts a runtime query back into its raw YAML-compatible representation.
    ///
    /// Sub-queries are serialized as [`JsonScraperSubQueryRaw`] children.
    /// Heterogeneous composition (e.g. HTML sub-queries inside a JSON query)
    /// raises a panic in the serialization path; a round-trip extension is a
    /// planned improvement.
    fn from(query: &JsonScraperQuery) -> Self {
        Self {
            common: ScraperQueryCommon {
                name: query.name.clone(),
                base_url: query.base_url_template.clone(),
                resolved_base_url: None,
                media_types: query.media_types.clone(),
                query_url: query.query_url.clone(),
                request_method: query.request_method,
                request_body_pointer: query.request_body_pointer.clone(),
                request_body_select: query.request_body_select,
                request_body_actions: query.request_body_actions.clone(),
                request_headers: query
                    .request_headers
                    .iter()
                    .map(|h| ScraperRequestHeaderRaw {
                        name: h.name.clone(),
                        pointer: h.pointer.clone(),
                        select: h.select,
                        actions: h.actions.clone(),
                    })
                    .collect(),
                http: query.http_config.clone(),
                query_param_mappings: query.query_param_mappings.clone(),
                result_item_field: query.result_item_field.clone(),
                post_process: query.post_processes.clone(),
            },
            extract_next_data: query.extract_next_data,
            sibling_sub_query_concurrency: query.sibling_sub_query_concurrency,
            sub_query_context_concurrency: query.sub_query_context_concurrency,
            sub_query_fetch_concurrency: query.sub_query_fetch_concurrency,
            filters: query.filters.clone(),
            row_pointer: query.row_pointer.clone(),
            entries: query
                .scraper_entries
                .iter()
                .map(JsonScraperEntryRaw::from)
                .collect(),
            sub_queries: Vec::new(),
            // Note: sub-queries serialization uses legacy JsonScraperSubQueryRaw
            // for backward compatibility. Sub-queries created via EntrySubQueryRaw
            // (the unified format) are not serialized through this path.
            // This is a known limitation; full round-trip support for heterogeneous
            // sub-queries is a planned improvement.
        }
    }
}

impl Serialize for JsonScraperQuery {
    /// Serializes the query through its raw YAML representation.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        JsonScraperQueryRaw::from(self).serialize(serializer)
    }
}

impl TryFrom<JsonScraperSubQueryRaw> for JsonScraperSubQuery {
    type Error = anyhow::Error;

    /// Converts a raw YAML sub-query definition into a validated runtime sub-query.
    ///
    /// Nested sub-queries are boxed into `Box<dyn ScraperQuery>`. They are
    /// dispatched by the unified executor or by [`JsonScraperSubQuery::execute_siblings`]
    /// at runtime; this conversion does not validate the runtime dispatch
    /// compatibility of mixed children (a `JsonScraperSubQuery` cannot dispatch
    /// a non-JSON sibling through its own siblings path, but the unified
    /// executor handles such cases).
    ///
    /// # Errors
    ///
    /// Returns an error if the row pointer is empty, the target is empty, or
    /// actions fail validation.
    fn try_from(config: JsonScraperSubQueryRaw) -> Result<Self> {
        let JsonScraperSubQueryRaw {
            common,
            row_pointer,
            entries,
            context_entries,
            request_body_pointer,
            request_body_select,
            request_body_actions,
            extract_next_data,
            sub_queries,
        } = config;

        let SubQueryCommon {
            context_pointer,
            context_select,
            filters,
            row_filters,
            target,
            request_pointer,
            request_select,
            request_actions,
            request_method,
            request_headers,
            http,
            post_process,
            ..
        } = common;

        if row_pointer.trim().is_empty() {
            anyhow::bail!("Invalid JSON sub-query: row_pointer cannot be empty");
        }
        if target
            .as_deref()
            .is_some_and(|target| target.trim().is_empty())
        {
            anyhow::bail!("Invalid JSON sub-query: target cannot be empty");
        }

        JsonScraperSubQuery::validate_actions(&row_pointer, &request_actions)?;
        JsonScraperSubQuery::validate_actions(&row_pointer, &request_body_actions)?;

        let entries = entries
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;
        let context_entries = context_entries
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;
        let request_headers = request_headers
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;
        let sub_queries = sub_queries
            .into_iter()
            .map(|raw| -> Result<_> {
                let runtime: JsonScraperSubQuery = raw.try_into()?;
                Ok(Box::new(runtime)
                    as Box<
                        dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    >)
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            context_pointer,
            context_select,
            filters,
            row_filters,
            context_entries,
            target,
            request_pointer,
            request_select,
            request_actions,
            request_method,
            request_body_pointer,
            request_body_select,
            request_body_actions,
            extract_next_data,
            request_headers,
            http_config: http,
            row_pointer,
            entries,
            post_processes: post_process,
            sub_queries,
            http_client: HttpClient::new(""),
        })
    }
}

impl From<&JsonScraperSubQuery> for JsonScraperSubQueryRaw {
    /// Converts a runtime sub-query back into its raw YAML-compatible representation.
    ///
    /// Nested sub-queries are not round-tripped through the raw type when they
    /// contain heterogeneous children; the serialization slot is left empty
    /// for such cases.  A full bidirectional conversion for heterogeneous
    /// trees is a planned extension.
    fn from(sub_query: &JsonScraperSubQuery) -> Self {
        Self {
            common: SubQueryCommon {
                context_pointer: sub_query.context_pointer.clone(),
                context_select: sub_query.context_select,
                filters: sub_query.filters.clone(),
                row_filters: sub_query.row_filters.clone(),
                target: sub_query.target.clone(),
                request_pointer: sub_query.request_pointer.clone(),
                request_select: sub_query.request_select,
                request_actions: sub_query.request_actions.clone(),
                request_method: sub_query.request_method,
                request_headers: sub_query
                    .request_headers
                    .iter()
                    .map(|h| ScraperRequestHeaderRaw {
                        name: h.name.clone(),
                        pointer: h.pointer.clone(),
                        select: h.select,
                        actions: h.actions.clone(),
                    })
                    .collect(),
                http: sub_query.http_config.clone(),
                post_process: sub_query.post_processes.clone(),
            },
            row_pointer: sub_query.row_pointer.clone(),
            entries: sub_query
                .entries
                .iter()
                .map(JsonScraperEntryRaw::from)
                .collect(),
            context_entries: sub_query
                .context_entries
                .iter()
                .map(JsonScraperEntryRaw::from)
                .collect(),
            request_body_pointer: sub_query.request_body_pointer.clone(),
            request_body_select: sub_query.request_body_select,
            request_body_actions: sub_query.request_body_actions.clone(),
            extract_next_data: sub_query.extract_next_data,
            sub_queries: sub_query
                .sub_queries
                .iter()
                .map(|child| {
                    child
                        .as_any()
                        .downcast_ref::<JsonScraperSubQuery>()
                        .map(JsonScraperSubQueryRaw::from)
                        .unwrap_or_else(|| {
                            panic!(
                                "JsonScraperSubQuery::from: nested sub-query '{}' is not a JsonScraperSubQuery; \
                                 heterogeneous sub-queries are not yet supported in the JSON serialization path",
                                child.name()
                            )
                        })
                })
                .collect(),
        }
    }
}
