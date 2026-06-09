//! Raw YAML configuration types and validated runtime conversions for JSON queries.
//!
//! This module defines the deserialization targets ([`JsonScraperQueryRaw`],
//! [`JsonScraperSubQueryRaw`], [`ScraperRequestHeaderRaw`]) and the shared
//! types ([`ScraperRequestMethod`], [`ScraperRequestHeader`]) that are
//! referenced by other scraper modules.  Bidirectional conversion between raw
//! YAML config and the validated runtime types is provided through
//! `TryFrom`/`From`/`Serialize` implementations.
//!
//! The runtime query structs themselves live in [`super::query`].

use anyhow::Result;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;

use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;
use crate::scrapyfy::scraper_json::entry::{
    json_value_to_strings, select_json_values, JsonScraperEntryRaw,
};
use crate::scrapyfy::scraper_json::query::{JsonScraperQuery, JsonScraperSubQuery};
use crate::scrapyfy::query_helpers::{self, QueryTemplateParamMapping};
use crate::scrapyfy::{HttpClient, ScraperAction, ScraperHttpConfig, ScraperPostProcess};

// ---------------------------------------------------------------------------
// Default-value helpers
// ---------------------------------------------------------------------------

/// Defaults to [`HtmlScraperSelectMode::All`].
fn default_json_select_mode() -> HtmlScraperSelectMode {
    HtmlScraperSelectMode::All
}

/// Defaults to [`ScraperRequestMethod::Get`].
fn default_json_request_method() -> ScraperRequestMethod {
    ScraperRequestMethod::Get
}

/// Defaults to `4`.
fn default_json_sibling_sub_query_concurrency() -> usize {
    4
}

/// Defaults to `4`.
fn default_json_sub_query_context_concurrency() -> usize {
    4
}

/// Defaults to `8`.
fn default_json_sub_query_fetch_concurrency() -> usize {
    8
}

// ---------------------------------------------------------------------------
// Shared types (referenced by other scraper modules)
// ---------------------------------------------------------------------------

/// Supported HTTP methods for root and follow-up scraper requests.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperRequestMethod {
    /// GET request.
    Get,
    /// POST request.
    Post,
}

impl ScraperRequestMethod {
    /// Converts this enum variant into the corresponding [`http::Method`].
    ///
    /// # Arguments
    ///
    /// * `self` - The request method variant to convert.
    pub fn as_http_method(self) -> http::Method {
        match self {
            Self::Get => http::Method::GET,
            Self::Post => http::Method::POST,
        }
    }
}

/// Raw configuration definition of one header added to a scraper request.
#[derive(Serialize, Deserialize)]
pub struct ScraperRequestHeaderRaw {
    /// Header name.
    name: String,
    /// Optional JSON pointer selecting the header value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pointer: Option<String>,
    /// Selection mode for the header pointer.
    #[serde(default = "default_json_select_mode")]
    select: HtmlScraperSelectMode,
    /// Actions applied to the header value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    actions: Vec<ScraperAction>,
}

/// Runtime definition of one header added to a scraper request.
#[derive(Deserialize)]
#[serde(try_from = "ScraperRequestHeaderRaw")]
pub struct ScraperRequestHeader {
    /// Header name.
    name: String,
    /// Optional JSON pointer selecting the header value.
    pointer: Option<String>,
    /// Selection mode for the header pointer.
    select: HtmlScraperSelectMode,
    /// Actions applied to the header value.
    actions: Vec<ScraperAction>,
}

impl ScraperRequestHeader {
    /// Validates every action against the header-level contract.
    ///
    /// # Arguments
    ///
    /// * `name` - Header name used in diagnostic messages.
    /// * `actions` - Actions to validate.
    ///
    /// # Errors
    ///
    /// Returns an error if any action fails validation for the `"sub-query header"` context.
    fn validate_actions(name: &str, actions: &[ScraperAction]) -> Result<()> {
        for action in actions {
            action.validate(name, "sub-query header")?;
        }

        Ok(())
    }

    /// Resolves this header's value from the context row and template parameters.
    ///
    /// Returns `None` when the resolved value is empty.
    ///
    /// # Arguments
    ///
    /// * `row` - Parent JSON value used for pointer resolution.
    /// * `params` - Runtime template parameters.
    /// * `request_url` - URL of the parent request for action pipelines.
    pub(crate) fn resolve(
        &self,
        row: &serde_json::Value,
        params: &HashMap<String, String>,
        request_url: &str,
    ) -> Option<(String, String)> {
        let mut values = Vec::new();

        for selected in select_json_values(row, self.pointer.as_deref(), self.select) {
            let mut selected_values = json_value_to_strings(selected);
            for action in &self.actions {
                selected_values =
                    action.apply(&None, selected_values, params, request_url, None, None);
            }
            values.extend(selected_values);
        }

        let resolved_values = values
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();

        if resolved_values.is_empty() {
            return None;
        }

        Some((self.name.clone(), resolved_values.join(", ")))
    }
}

impl TryFrom<ScraperRequestHeaderRaw> for ScraperRequestHeader {
    type Error = anyhow::Error;

    /// Converts a raw YAML header definition into a validated runtime header.
    ///
    /// # Errors
    ///
    /// Returns an error if the header name is empty or actions fail validation.
    fn try_from(config: ScraperRequestHeaderRaw) -> Result<Self> {
        let ScraperRequestHeaderRaw {
            name,
            pointer,
            select,
            actions,
        } = config;

        if name.trim().is_empty() {
            anyhow::bail!("Invalid JSON sub-query header: name cannot be empty");
        }

        ScraperRequestHeader::validate_actions(&name, &actions)?;

        Ok(Self {
            name,
            pointer,
            select,
            actions,
        })
    }
}

impl From<&ScraperRequestHeader> for ScraperRequestHeaderRaw {
    /// Converts a runtime header back into its raw YAML-compatible representation.
    fn from(header: &ScraperRequestHeader) -> Self {
        Self {
            name: header.name.clone(),
            pointer: header.pointer.clone(),
            select: header.select,
            actions: header.actions.clone(),
        }
    }
}

impl Serialize for ScraperRequestHeader {
    /// Serializes the header through its raw YAML representation.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ScraperRequestHeaderRaw::from(self).serialize(serializer)
    }
}

/// Concurrency limits forwarded to sub-query execution.
#[derive(Clone, Copy)]
pub(crate) struct JsonScraperExecutionOptions {
    /// Maximum number of sibling sub-queries executed together.
    pub sibling_sub_query_concurrency: usize,
    /// Maximum number of sub-query contexts executed together.
    pub sub_query_context_concurrency: usize,
    /// Maximum number of follow-up HTTP requests executed together.
    pub sub_query_fetch_concurrency: usize,
}

// ---------------------------------------------------------------------------
// Raw YAML configuration types
// ---------------------------------------------------------------------------

/// Raw configuration definition of one JSON query endpoint.
#[derive(Serialize, Deserialize)]
pub struct JsonScraperQueryRaw {
    /// Query identifier used as the lookup key in a collection.
    name: String,
    /// Base URL of the source, also available as `{base_url}` in templates.
    base_url: String,
    /// Resolved base URL after collection-level placeholder substitution.
    #[serde(skip)]
    resolved_base_url: Option<String>,
    /// Content types this query produces.
    media_types: Vec<String>,
    /// URL template used to build the request.
    query_url: String,
    /// When `true` the response is parsed as a Next.js `__NEXT_DATA__` payload.
    #[serde(default)]
    extract_next_data: bool,
    /// HTTP method used to issue the request.
    #[serde(default = "default_json_request_method")]
    request_method: ScraperRequestMethod,
    /// Optional JSON pointer selecting the request body from runtime params.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request_body_pointer: Option<String>,
    /// Selection mode for the request body pointer.
    #[serde(default = "default_json_select_mode")]
    request_body_select: HtmlScraperSelectMode,
    /// Actions applied to the request body before the HTTP call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    request_body_actions: Vec<ScraperAction>,
    /// HTTP headers attached to the request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    request_headers: Vec<ScraperRequestHeaderRaw>,
    /// HTTP client configuration (mode, user agent, max redirects).
    #[serde(default, skip_serializing_if = "ScraperHttpConfig::is_empty")]
    http: ScraperHttpConfig,
    /// Source-to-target parameter mappings applied before template resolution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    query_param_mappings: Vec<QueryTemplateParamMapping>,
    /// Maximum number of sibling sub-queries executed together (default: 4).
    #[serde(default = "default_json_sibling_sub_query_concurrency")]
    sibling_sub_query_concurrency: usize,
    /// Maximum number of sub-query contexts executed together (default: 4).
    #[serde(default = "default_json_sub_query_context_concurrency")]
    sub_query_context_concurrency: usize,
    /// Maximum number of follow-up HTTP requests executed together (default: 8).
    #[serde(default = "default_json_sub_query_fetch_concurrency")]
    sub_query_fetch_concurrency: usize,
    /// Root-level filters applied to the entire JSON response.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    filters: HashMap<String, Vec<String>>,
    /// JSON pointer-like path matching each result row.
    row_pointer: String,
    /// Optional group field whose items should become query rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result_item_field: Option<String>,
    /// Field extractors executed for every matched row.
    entries: Vec<JsonScraperEntryRaw>,
    /// Chained JSON follow-up requests executed after the main entries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sub_queries: Vec<JsonScraperSubQueryRaw>,
    /// Post-processing steps applied to each extracted row.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    post_process: Vec<ScraperPostProcess>,
}

// ---------------------------------------------------------------------------
// Sub-query raw types and conversions (re-exported via query)
// ---------------------------------------------------------------------------

/// Raw configuration definition of one chained JSON follow-up request.
#[derive(Serialize, Deserialize)]
pub struct JsonScraperSubQueryRaw {
    /// Optional pointer selecting the context rows that seed follow-up requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context_pointer: Option<String>,
    /// Selection mode for the context pointer.
    #[serde(default = "default_json_select_mode")]
    context_select: HtmlScraperSelectMode,
    /// Filters applied to the context row before issuing the follow-up request.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    filters: HashMap<String, Vec<String>>,
    /// Filters applied to the fetched rows (after the HTTP response).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    row_filters: HashMap<String, Vec<String>>,
    /// Entries extracted from the context row rather than from fetched rows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    context_entries: Vec<JsonScraperEntryRaw>,
    /// Path where the sub-query result is nested (default: root level).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    /// Pointer selecting the request URL from the context row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request_pointer: Option<String>,
    /// Selection mode for the request pointer.
    #[serde(default = "default_json_select_mode")]
    request_select: HtmlScraperSelectMode,
    /// Actions applied to the selected request URLs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    request_actions: Vec<ScraperAction>,
    /// HTTP method used for the follow-up request.
    #[serde(default = "default_json_request_method")]
    request_method: ScraperRequestMethod,
    /// Optional JSON pointer selecting the request body from the context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request_body_pointer: Option<String>,
    /// Selection mode for the request body pointer.
    #[serde(default = "default_json_select_mode")]
    request_body_select: HtmlScraperSelectMode,
    /// Actions applied to the request body values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    request_body_actions: Vec<ScraperAction>,
    /// When `true` the follow-up response is parsed as Next.js `__NEXT_DATA__`.
    #[serde(default)]
    extract_next_data: bool,
    /// HTTP headers attached to the follow-up request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    request_headers: Vec<ScraperRequestHeaderRaw>,
    /// HTTP client configuration for the follow-up request.
    #[serde(default, skip_serializing_if = "ScraperHttpConfig::is_empty")]
    http: ScraperHttpConfig,
    /// JSON pointer-like path matching each result row in the response.
    row_pointer: String,
    /// Field extractors executed for every matched row.
    entries: Vec<JsonScraperEntryRaw>,
    /// Nested sub-queries (recursion).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sub_queries: Vec<JsonScraperSubQueryRaw>,
}

impl JsonScraperSubQueryRaw {
    /// Propagates the parent HTTP configuration into this sub-query and its children.
    ///
    /// # Arguments
    ///
    /// * `parent_http` - HTTP configuration inherited from the parent query.
    pub(crate) fn apply_parent_http(&mut self, parent_http: &ScraperHttpConfig) {
        self.http = parent_http.merge(&self.http);

        for sub_query in &mut self.sub_queries {
            sub_query.apply_parent_http(&self.http);
        }
    }

    /// Resolves collection-level placeholders in this sub-query's HTTP config.
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
        self.http
            .resolve_collection_params("JSON sub-query", parent_name, params)?;
        for sub_query in &mut self.sub_queries {
            sub_query.resolve_collection_params(parent_name, params)?;
        }
        Ok(())
    }
}

impl JsonScraperQueryRaw {
    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        self.name.clone()
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
        self.resolved_base_url = Some(query_helpers::resolve_required_template(
            "JSON query",
            &self.name,
            "base_url",
            &self.base_url,
            params,
        )?);
        self.http
            .resolve_collection_params("JSON query", &self.name, params)?;
        for sub_query in &mut self.sub_queries {
            sub_query.resolve_collection_params(&self.name, params)?;
        }
        Ok(())
    }

    /// Merges the collection-level HTTP configuration into this query and its sub-queries.
    ///
    /// # Arguments
    ///
    /// * `collection_http` - HTTP configuration inherited from the parent collection.
    pub(crate) fn apply_collection_http(&mut self, collection_http: &ScraperHttpConfig) {
        self.http = collection_http.merge(&self.http);

        for sub_query in &mut self.sub_queries {
            sub_query.apply_parent_http(&self.http);
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
            name,
            base_url,
            resolved_base_url,
            media_types,
            query_url,
            extract_next_data,
            request_method,
            request_body_pointer,
            request_body_select,
            request_body_actions,
            request_headers,
            http,
            query_param_mappings,
            sibling_sub_query_concurrency,
            sub_query_context_concurrency,
            sub_query_fetch_concurrency,
            filters,
            row_pointer,
            result_item_field,
            entries,
            sub_queries,
            post_process,
        } = config;
        let resolved_base_url = query_helpers::resolved_or_template(&base_url, resolved_base_url);

        let entries = entries
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;
        let sub_queries = sub_queries
            .into_iter()
            .map(TryInto::try_into)
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
    fn from(query: &JsonScraperQuery) -> Self {
        Self {
            name: query.name.clone(),
            base_url: query.base_url_template.clone(),
            resolved_base_url: None,
            media_types: query.media_types.clone(),
            query_url: query.query_url.clone(),
            extract_next_data: query.extract_next_data,
            request_method: query.request_method,
            request_body_pointer: query.request_body_pointer.clone(),
            request_body_select: query.request_body_select,
            request_body_actions: query.request_body_actions.clone(),
            request_headers: query
                .request_headers
                .iter()
                .map(ScraperRequestHeaderRaw::from)
                .collect(),
            http: query.http_config.clone(),
            query_param_mappings: query.query_param_mappings.clone(),
            sibling_sub_query_concurrency: query.sibling_sub_query_concurrency,
            sub_query_context_concurrency: query.sub_query_context_concurrency,
            sub_query_fetch_concurrency: query.sub_query_fetch_concurrency,
            filters: query.filters.clone(),
            row_pointer: query.row_pointer.clone(),
            result_item_field: query.result_item_field.clone(),
            entries: query
                .scraper_entries
                .iter()
                .map(JsonScraperEntryRaw::from)
                .collect(),
            sub_queries: query
                .sub_queries
                .iter()
                .map(JsonScraperSubQueryRaw::from)
                .collect(),
            post_process: query.post_processes.clone(),
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
    /// # Errors
    ///
    /// Returns an error if the row pointer is empty, the target is empty, or
    /// actions fail validation.
    fn try_from(config: JsonScraperSubQueryRaw) -> Result<Self> {
        let JsonScraperSubQueryRaw {
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
            http,
            row_pointer,
            entries,
            sub_queries,
        } = config;

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
            .map(TryInto::try_into)
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
            sub_queries,
            http_client: HttpClient::new(""),
        })
    }
}

impl From<&JsonScraperSubQuery> for JsonScraperSubQueryRaw {
    /// Converts a runtime sub-query back into its raw YAML-compatible representation.
    fn from(sub_query: &JsonScraperSubQuery) -> Self {
        Self {
            context_pointer: sub_query.context_pointer.clone(),
            context_select: sub_query.context_select,
            filters: sub_query.filters.clone(),
            row_filters: sub_query.row_filters.clone(),
            context_entries: sub_query
                .context_entries
                .iter()
                .map(JsonScraperEntryRaw::from)
                .collect(),
            target: sub_query.target.clone(),
            request_pointer: sub_query.request_pointer.clone(),
            request_select: sub_query.request_select,
            request_actions: sub_query.request_actions.clone(),
            request_method: sub_query.request_method,
            request_body_pointer: sub_query.request_body_pointer.clone(),
            request_body_select: sub_query.request_body_select,
            request_body_actions: sub_query.request_body_actions.clone(),
            extract_next_data: sub_query.extract_next_data,
            request_headers: sub_query
                .request_headers
                .iter()
                .map(ScraperRequestHeaderRaw::from)
                .collect(),
            http: sub_query.http_config.clone(),
            row_pointer: sub_query.row_pointer.clone(),
            entries: sub_query
                .entries
                .iter()
                .map(JsonScraperEntryRaw::from)
                .collect(),
            sub_queries: sub_query
                .sub_queries
                .iter()
                .map(JsonScraperSubQueryRaw::from)
                .collect(),
        }
    }
}