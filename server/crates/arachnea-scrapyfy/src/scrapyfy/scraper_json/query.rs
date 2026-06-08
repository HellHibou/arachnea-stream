//! Runtime JSON query and sub-query types.
//!
//! This module defines the validated runtime types [`JsonScraperQuery`] and
//! [`JsonScraperSubQuery`], along with their execution logic (HTTP fetch,
//! row extraction, entry application, sub-query recursion).  Raw YAML
//! configuration types and conversions live in [`super::config`] and are
//! re-exported here so that existing import paths continue to work.

// Re-export shared types that moved to `config` so that references like
// `use crate::scrapyfy::scraper_json::query::{ScraperRequestMethod, …}`
// still resolve.
pub use super::config::{JsonScraperQueryRaw, JsonScraperSubQueryRaw};
pub(crate) use super::config::{
    JsonScraperExecutionOptions, ScraperRequestHeader, ScraperRequestHeaderRaw,
    ScraperRequestMethod,
};

use anyhow::Result;
use futures::stream::{self, StreamExt};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

use crate::scrapyfy::*;
use crate::scrapyfy::scraper::entry_trait::ScraperEntrySpec;
use crate::scrapyfy::scraper::query_trait::ScraperQuery;
use crate::scrapyfy::scraper::row_locator::{RowLocator, ScraperType};
use crate::scrapyfy::scraper::sub_query_spec::SubQuerySpec;
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;
use crate::scrapyfy::scraper_json::entry::{json_value_to_strings, select_json_values};
use crate::scrapyfy::scraper_json::response_parser::{collect_ordered_results, matches};
use crate::scrapyfy::query_helpers::{self, QueryTemplateParamMapping};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A query definition for one JSON source and one result shape.
#[derive(Deserialize)]
#[serde(try_from = "JsonScraperQueryRaw")]
pub struct JsonScraperQuery {
    pub(crate) name: String,
    pub(crate) base_url: String,
    pub(crate) base_url_template: String,
    pub(crate) media_types: Vec<String>,
    pub(crate) query_url: String,
    pub(crate) extract_next_data: bool,
    pub(crate) request_method: ScraperRequestMethod,
    pub(crate) request_body_pointer: Option<String>,
    pub(crate) request_body_select: HtmlScraperSelectMode,
    pub(crate) request_body_actions: Vec<ScraperAction>,
    pub(crate) request_headers: Vec<ScraperRequestHeader>,
    pub(crate) http_config: ScraperHttpConfig,
    pub(crate) query_param_mappings: Vec<QueryTemplateParamMapping>,
    pub(crate) sibling_sub_query_concurrency: usize,
    pub(crate) sub_query_context_concurrency: usize,
    pub(crate) sub_query_fetch_concurrency: usize,
    pub(crate) filters: HashMap<String, Vec<String>>,
    pub(crate) row_pointer: String,
    pub(crate) result_item_field: Option<String>,
    pub(crate) scraper_entries: Vec<JsonScraperEntry>,
    pub(crate) sub_queries: Vec<JsonScraperSubQuery>,
    pub(crate) post_processes: Vec<ScraperPostProcess>,

    pub(crate) http_client: HttpClient,
}

// ---------------------------------------------------------------------------
// JsonScraperSubQuery
// ---------------------------------------------------------------------------

/// Runtime definition of one chained JSON follow-up request.
#[derive(Deserialize)]
#[serde(try_from = "JsonScraperSubQueryRaw")]
pub struct JsonScraperSubQuery {
    pub(crate) context_pointer: Option<String>,
    pub(crate) context_select: HtmlScraperSelectMode,
    pub(crate) filters: HashMap<String, Vec<String>>,
    pub(crate) row_filters: HashMap<String, Vec<String>>,
    pub(crate) context_entries: Vec<JsonScraperEntry>,
    pub(crate) target: Option<String>,
    pub(crate) request_pointer: Option<String>,
    pub(crate) request_select: HtmlScraperSelectMode,
    pub(crate) request_actions: Vec<ScraperAction>,
    pub(crate) request_method: ScraperRequestMethod,
    pub(crate) request_body_pointer: Option<String>,
    pub(crate) request_body_select: HtmlScraperSelectMode,
    pub(crate) request_body_actions: Vec<ScraperAction>,
    pub(crate) extract_next_data: bool,
    pub(crate) request_headers: Vec<ScraperRequestHeader>,
    pub(crate) http_config: ScraperHttpConfig,
    pub(crate) row_pointer: String,
    pub(crate) entries: Vec<JsonScraperEntry>,
    pub(crate) sub_queries: Vec<JsonScraperSubQuery>,
}

impl JsonScraperSubQuery {
    /// Returns the flattened list of leaf field names produced by this sub-query tree.
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
            names.extend(sub_query.field_names());
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

    /// Executes this sub-query for a given context row.
    ///
    /// Selects context values, issues follow-up HTTP requests, extracts rows,
    /// and merges the results into `root`.
    ///
    /// # Arguments
    ///
    /// * `root` - Data node receiving the merged sub-query results.
    /// * `context_row` - Parent JSON row providing context values.
    /// * `params` - Runtime template parameters.
    /// * `context_request_url` - URL of the parent request for action resolution.
    /// * `base_url` - Base URL used to build follow-up request URLs.
    /// * `http_client` - Shared HTTP client.
    /// * `execution_options` - Concurrency limits applied to follow-up requests.
    pub(crate) async fn execute(
        &self,
        root: &mut ScraperDataNode,
        context_row: &serde_json::Value,
        params: &HashMap<String, String>,
        context_request_url: &str,
        base_url: &str,
        http_client: &HttpClient,
        execution_options: JsonScraperExecutionOptions,
    ) -> Result<()> {
        let contexts = select_json_values(
            context_row,
            self.context_pointer.as_deref(),
            self.context_select,
        );

        let mut context_jobs = Vec::with_capacity(contexts.len());
        for (context_index, context) in contexts.into_iter().enumerate() {
            context_jobs.push(self.execute_indexed_context(
                context_index,
                context,
                params,
                context_request_url,
                base_url,
                http_client,
                execution_options,
            ));
        }

        let context_results = stream::iter(context_jobs)
            .buffer_unordered(execution_options.sub_query_context_concurrency)
            .collect::<Vec<_>>()
            .await;

        for node in collect_ordered_results(context_results)? {
            root.merge(node);
        }

        Ok(())
    }

    /// Executes sibling sub-queries concurrently and merges their outputs in configuration order.
    ///
    /// # Arguments
    ///
    /// * `sub_queries` - Sub-queries to execute in parallel.
    /// * `root` - Data node receiving the merged results.
    /// * `context_row` - Parent JSON row providing context values.
    /// * `params` - Runtime template parameters.
    /// * `context_request_url` - URL of the parent request for action resolution.
    /// * `base_url` - Base URL used to build follow-up request URLs.
    /// * `http_client` - Shared HTTP client.
    /// * `execution_options` - Concurrency limits applied to follow-up requests.
    pub(crate) async fn execute_siblings(
        sub_queries: &[JsonScraperSubQuery],
        root: &mut ScraperDataNode,
        context_row: &serde_json::Value,
        params: &HashMap<String, String>,
        context_request_url: &str,
        base_url: &str,
        http_client: &HttpClient,
        execution_options: JsonScraperExecutionOptions,
    ) -> Result<()> {
        let mut sub_query_jobs = Vec::with_capacity(sub_queries.len());
        for (sub_query_index, sub_query) in sub_queries.iter().enumerate() {
            sub_query_jobs.push(Self::execute_indexed_sibling(
                sub_query_index,
                sub_query,
                context_row,
                params,
                context_request_url,
                base_url,
                http_client,
                execution_options,
            ));
        }

        let sub_query_results = stream::iter(sub_query_jobs)
            .buffer_unordered(execution_options.sibling_sub_query_concurrency)
            .collect::<Vec<_>>()
            .await;

        for node in collect_ordered_results(sub_query_results)? {
            root.merge(node);
        }

        Ok(())
    }

    /// Executes this sub-query for a single context value.
    ///
    /// Returns the merged [`ScraperDataNode`] produced by all rows fetched
    /// for this context.
    ///
    /// # Arguments
    ///
    /// * `context` - JSON value used as the context for follow-up requests.
    /// * `params` - Runtime template parameters.
    /// * `context_request_url` - URL of the parent request for action resolution.
    /// * `base_url` - Base URL used to build follow-up request URLs.
    /// * `http_client` - Shared HTTP client.
    /// * `execution_options` - Concurrency limits applied to follow-up requests.
    async fn execute_context(
        &self,
        context: &serde_json::Value,
        params: &HashMap<String, String>,
        context_request_url: &str,
        base_url: &str,
        http_client: &HttpClient,
        execution_options: JsonScraperExecutionOptions,
    ) -> Result<ScraperDataNode> {
        if !matches(context, &self.filters) {
            return Ok(ScraperDataNode::default());
        }

        let request_values = select_json_values(
            context,
            self.request_pointer.as_deref(),
            self.request_select,
        );
        let mut values = request_values
            .into_iter()
            .flat_map(json_value_to_strings)
            .collect::<Vec<_>>();
        let has_request_seed = !values.is_empty();

        if !has_request_seed && (self.context_pointer.is_some() || self.request_pointer.is_some()) {
            return Ok(ScraperDataNode::default());
        }

        for action in &self.request_actions {
            values = action.apply(&None, values, params, context_request_url, None, None);
        }

        let mut request_urls = Vec::new();
        let mut seen_urls = HashSet::new();

        for request_url in values
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            if seen_urls.insert(request_url.clone()) {
                request_urls.push(request_url);
            }
        }

        let request_headers = self.resolve_request_headers(context, params, context_request_url);
        let request_body = self.resolve_request_body(context, params, context_request_url);
        let extract_next_data = self.extract_next_data;
        let request_method = self.request_method.as_http_method();
        let http_client = http_client.configured(self.http_config.clone());
        let fetch_results = stream::iter(request_urls.into_iter().enumerate().map(
            |(request_index, request_url)| {
                let request_headers = request_headers.clone();
                let request_body = request_body.clone();
                let request_method = request_method.clone();
                let http_client = http_client.clone();
                async move {
                    let json = if extract_next_data {
                        http_client
                            .get_next_data_json_for_request(
                                request_method,
                                &request_url,
                                &request_headers,
                                request_body.as_deref(),
                            )
                            .await?
                    } else {
                        http_client
                            .get_json_for_request(
                                request_method,
                                &request_url,
                                &request_headers,
                                request_body.as_deref(),
                            )
                            .await?
                    };
                    Ok::<(usize, (String, serde_json::Value)), anyhow::Error>((
                        request_index,
                        (request_url, json),
                    ))
                }
            },
        ))
        .buffer_unordered(execution_options.sub_query_fetch_concurrency)
        .collect::<Vec<_>>()
        .await;

        let mut context_root = ScraperDataNode::default();

        for (request_url, json) in collect_ordered_results(fetch_results)? {
            let rows = select_json_values(
                &json,
                Some(self.row_pointer.as_str()),
                HtmlScraperSelectMode::All,
            );

            for row in rows {
                if !matches(row, &self.row_filters) {
                    continue;
                }

                let row_node = self
                    .build_row_node(
                        context,
                        row,
                        params,
                        context_request_url,
                        &request_url,
                        base_url,
                        &http_client,
                        execution_options,
                    )
                    .await?;
                context_root.merge(row_node);
            }
        }

        Ok(context_root)
    }

    /// Wraps [`execute_context`](Self::execute_context) with an index tag for ordering.
    ///
    /// # Arguments
    ///
    /// * `context_index` - Position index used to restore ordering after async processing.
    /// * `context` - JSON value used as the context for follow-up requests.
    /// * `params` - Runtime template parameters.
    /// * `context_request_url` - URL of the parent request.
    /// * `base_url` - Base URL for follow-up requests.
    /// * `http_client` - Shared HTTP client.
    /// * `execution_options` - Concurrency limits.
    async fn execute_indexed_context(
        &self,
        context_index: usize,
        context: &serde_json::Value,
        params: &HashMap<String, String>,
        context_request_url: &str,
        base_url: &str,
        http_client: &HttpClient,
        execution_options: JsonScraperExecutionOptions,
    ) -> Result<(usize, ScraperDataNode)> {
        let node = self
            .execute_context(
                context,
                params,
                context_request_url,
                base_url,
                http_client,
                execution_options,
            )
            .await?;
        Ok((context_index, node))
    }

    /// Executes a single sibling sub-query and tags the result with its index.
    ///
    /// # Arguments
    ///
    /// * `sub_query_index` - Position index used to restore ordering.
    /// * `sub_query` - Sub-query to execute.
    /// * `context_row` - Parent JSON row providing context values.
    /// * `params` - Runtime template parameters.
    /// * `context_request_url` - URL of the parent request.
    /// * `base_url` - Base URL for follow-up requests.
    /// * `http_client` - Shared HTTP client.
    /// * `execution_options` - Concurrency limits.
    async fn execute_indexed_sibling(
        sub_query_index: usize,
        sub_query: &JsonScraperSubQuery,
        context_row: &serde_json::Value,
        params: &HashMap<String, String>,
        context_request_url: &str,
        base_url: &str,
        http_client: &HttpClient,
        execution_options: JsonScraperExecutionOptions,
    ) -> Result<(usize, ScraperDataNode)> {
        let mut partial = ScraperDataNode::default();
        sub_query
            .execute(
                &mut partial,
                context_row,
                params,
                context_request_url,
                base_url,
                http_client,
                execution_options,
            )
            .await?;
        Ok((sub_query_index, partial))
    }

    /// Builds the output [`ScraperDataNode`] for a single fetched row.
    ///
    /// When `target` is set, entries are nested under that path; otherwise
    /// they are placed at the root level.
    ///
    /// # Arguments
    ///
    /// * `context` - Parent JSON context value.
    /// * `row` - Current fetched JSON row.
    /// * `params` - Runtime template parameters.
    /// * `context_request_url` - URL of the parent request.
    /// * `request_url` - URL used to fetch this row.
    /// * `base_url` - Base URL for follow-up requests.
    /// * `http_client` - Shared HTTP client.
    /// * `execution_options` - Concurrency limits.
    async fn build_row_node(
        &self,
        context: &serde_json::Value,
        row: &serde_json::Value,
        params: &HashMap<String, String>,
        context_request_url: &str,
        request_url: &str,
        base_url: &str,
        http_client: &HttpClient,
        execution_options: JsonScraperExecutionOptions,
    ) -> Result<ScraperDataNode> {
        let mut row_root = ScraperDataNode::default();

        if let Some(target) = &self.target {
            let mut item = ScraperDataNode::default();

            for entry in &self.context_entries {
                entry.apply_to(&mut item, context, params, context_request_url);
            }

            for entry in &self.entries {
                entry.apply_to(&mut item, row, params, base_url);
            }

            Self::execute_siblings(
                &self.sub_queries,
                &mut item,
                row,
                params,
                request_url,
                base_url,
                http_client,
                execution_options,
            )
            .await?;

            let target_path = Self::split_target_path(target);
            row_root.push_node(&target_path, item);
        } else {
            for entry in &self.context_entries {
                entry.apply_to(&mut row_root, context, params, context_request_url);
            }

            for entry in &self.entries {
                entry.apply_to(&mut row_root, row, params, base_url);
            }

            Self::execute_siblings(
                &self.sub_queries,
                &mut row_root,
                row,
                params,
                request_url,
                base_url,
                http_client,
                execution_options,
            )
            .await?;
        }

        Ok(row_root)
    }

    /// Resolves request headers from the context row and template parameters.
    ///
    /// # Arguments
    ///
    /// * `context_row` - Parent JSON value used for header pointer resolution.
    /// * `params` - Runtime template parameters.
    /// * `context_request_url` - URL of the parent request for action pipelines.
    fn resolve_request_headers(
        &self,
        context_row: &serde_json::Value,
        params: &HashMap<String, String>,
        context_request_url: &str,
    ) -> HashMap<String, String> {
        let mut headers = HashMap::new();

        for header in &self.request_headers {
            if let Some((name, value)) = header.resolve(context_row, params, context_request_url) {
                headers.insert(name, value);
            }
        }

        headers
    }

    /// Builds the optional request body from the context row and body actions.
    ///
    /// # Arguments
    ///
    /// * `context_row` - Parent JSON value used for body pointer resolution.
    /// * `params` - Runtime template parameters.
    /// * `context_request_url` - URL of the parent request for action pipelines.
    fn resolve_request_body(
        &self,
        context_row: &serde_json::Value,
        params: &HashMap<String, String>,
        context_request_url: &str,
    ) -> Option<String> {
        let mut values = select_json_values(
            context_row,
            self.request_body_pointer.as_deref(),
            self.request_body_select,
        )
        .into_iter()
        .flat_map(json_value_to_strings)
        .collect::<Vec<_>>();

        for action in &self.request_body_actions {
            values = action.apply(&None, values, params, context_request_url, None, None);
        }

        values
            .into_iter()
            .map(|value| value.trim().to_string())
            .find(|value| !value.is_empty())
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
        for action in actions {
            action.validate(name, "query")?;
        }

        Ok(())
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
        sub_queries: Vec<JsonScraperSubQuery>,
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
    pub fn sub_queries(&self) -> &Vec<JsonScraperSubQuery> {
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
    /// * `sub_queries` - Follow-up sub-queries attached to this query.
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
        sub_queries: Vec<JsonScraperSubQuery>,
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
        let mut headers = HashMap::new();

        for header in &self.request_headers {
            if let Some((name, value)) = header.resolve(&request_context, params, query_url) {
                headers.insert(name, value);
            }
        }

        headers
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
        let mut values = select_json_values(
            &request_context,
            self.request_body_pointer.as_deref(),
            self.request_body_select,
        )
        .into_iter()
        .flat_map(json_value_to_strings)
        .collect::<Vec<_>>();

        for action in &self.request_body_actions {
            values = action.apply(&None, values, params, query_url, None, None);
        }

        values
            .into_iter()
            .map(|value| value.trim().to_string())
            .find(|value| !value.is_empty())
    }

    /// Builds the concurrency options forwarded to sub-query execution.
    fn execution_options(&self) -> JsonScraperExecutionOptions {
        JsonScraperExecutionOptions {
            sibling_sub_query_concurrency: self.sibling_sub_query_concurrency,
            sub_query_context_concurrency: self.sub_query_context_concurrency,
            sub_query_fetch_concurrency: self.sub_query_fetch_concurrency,
        }
    }

    /// Returns whether the query matches at least one requested media type.
    pub fn is_media_type(&self, media_types: &[String]) -> bool {
        media_types
            .iter()
            .any(|media_type| self.media_types().contains(media_type))
    }

    /// Returns the flattened list of leaf field names produced by this query.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_field_names(&self) -> Vec<String> {
        let mut names = self
            .scraper_entries()
            .iter()
            .flat_map(JsonScraperEntry::field_names)
            .collect::<Vec<_>>();

        for sub_query in self.sub_queries() {
            names.extend(sub_query.field_names());
        }

        names
    }

    /// Executes the query by formatting the URL, fetching the JSON payload, and extracting each row.
    ///
    /// # Arguments
    ///
    /// * `params` - Runtime values used to replace placeholders in the URL template.
    /// * `fields_filters` - Root fields filter list or None.
    ///
    /// Field names containing `>` are interpreted as hierarchical paths and will
    /// be converted into nested objects in the returned entries.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL template is missing parameters, if the payload
    /// cannot be downloaded, or if the response cannot be parsed.
    pub async fn execute_query(
        &self,
        params: &HashMap<String, String>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
    ) -> Result<Vec<HashMap<String, ScraperDataNode>>> {
        let resolved_params = query_helpers::build_query_execution_params(
            self.base_url(),
            params,
            &self.query_param_mappings,
        );
        let query_url = query_helpers::format_query_template(
            self.base_url(),
            self.query_url(),
            &resolved_params,
        )?;
        let request_headers = self.resolve_request_headers(&resolved_params, &query_url);
        let request_body = self.resolve_request_body(&resolved_params, &query_url);
        let request_method = self.request_method.as_http_method();
        let json = if self.extract_next_data {
            self.http_client
                .get_next_data_json_for_request(
                    request_method,
                    &query_url,
                    &request_headers,
                    request_body.as_deref(),
                )
                .await?
        } else {
            self.http_client
                .get_json_for_request(
                    request_method,
                    &query_url,
                    &request_headers,
                    request_body.as_deref(),
                )
                .await?
        };

        if !matches(&json, &self.filters) {
            return Ok(Vec::new());
        }

        let rows = select_json_values(&json, Some(self.row_pointer()), HtmlScraperSelectMode::All);
        let mut results: Vec<HashMap<String, ScraperDataNode>> = Vec::new();
        let execution_options = self.execution_options();

        if self.has_group_entries() {
            let mut root = ScraperDataNode::default();

            for scraper in self
                .scraper_entries()
                .iter()
                .filter(|entry| !entry.is_group())
            {
                scraper.apply_to(&mut root, &json, &resolved_params, &query_url);
            }

            for row in rows {
                for scraper in self
                    .scraper_entries()
                    .iter()
                    .filter(|entry| entry.is_group())
                {
                    scraper.apply_to(&mut root, row, &resolved_params, &query_url);
                }

                JsonScraperSubQuery::execute_siblings(
                    self.sub_queries(),
                    &mut root,
                    row,
                    &resolved_params,
                    &query_url,
                    self.base_url(),
                    &self.http_client,
                    execution_options,
                )
                .await?;
            }

            for post_process in &self.post_processes {
                post_process
                    .apply(
                        &mut root,
                        &ScraperPostProcessContext {
                            params: &resolved_params,
                            request_url: &query_url,
                            response_body: None,
                            http_client: &self.http_client,
                        },
                    )
                    .await?;
            }

            if fields_filters.is_none()
                || !query_helpers::is_root_filtered(&root, fields_filters.unwrap())
            {
                results.push(root.children);
            }
        } else {
            for row in rows {
                let mut root = ScraperDataNode::default();
                for scraper in self.scraper_entries() {
                    scraper.apply_to(&mut root, row, &resolved_params, &query_url);
                }

                JsonScraperSubQuery::execute_siblings(
                    self.sub_queries(),
                    &mut root,
                    row,
                    &resolved_params,
                    &query_url,
                    self.base_url(),
                    &self.http_client,
                    execution_options,
                )
                .await?;

                for post_process in &self.post_processes {
                    post_process
                        .apply(
                            &mut root,
                            &ScraperPostProcessContext {
                                params: &resolved_params,
                                request_url: &query_url,
                                response_body: None,
                                http_client: &self.http_client,
                            },
                        )
                        .await?;
                }

                if fields_filters.is_none()
                    || !query_helpers::is_root_filtered(&root, fields_filters.unwrap())
                {
                    results.push(root.children);
                }
            }
        }

        Ok(results)
    }

    /// Returns `true` when at least one scraper entry is a group entry.
    fn has_group_entries(&self) -> bool {
        self.scraper_entries.iter().any(JsonScraperEntry::is_group)
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

    fn is_media_type(&self, media_types: &[String]) -> bool {
        self.is_media_type(media_types)
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
            .map(|sub| sub as &dyn ScraperQuery)
            .collect()
    }

    fn sub_query_spec(&self) -> Option<&SubQuerySpec> {
        None
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

    fn request_headers(&self) -> &[ScraperRequestHeader] {
        &self.request_headers
    }

    fn http_config(&self) -> &ScraperHttpConfig {
        &self.http_config
    }

    fn extract_next_data(&self) -> bool {
        self.extract_next_data
    }

    fn row_locator(&self) -> RowLocator {
        RowLocator::Pointer(self.row_pointer.clone())
    }

    fn post_processes(&self) -> &[crate::scrapyfy::ScraperPostProcess] {
        const EMPTY: &[crate::scrapyfy::ScraperPostProcess] = &[];
        EMPTY
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
            .map(|sub| sub as &dyn ScraperQuery)
            .collect()
    }

    fn sub_query_spec(&self) -> Option<&SubQuerySpec> {
        None
    }
}
