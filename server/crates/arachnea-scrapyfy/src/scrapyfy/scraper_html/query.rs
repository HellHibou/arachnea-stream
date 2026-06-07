//! Runtime query type for HTML scraper endpoints.
//!
//! An [`HtmlScraperQuery`] wraps a validated CSS selector, entry definitions,
//! and HTTP configuration. Execution is delegated to [`ScraperManagerQuery`].
//!
//! Raw YAML deserialization and bidirectional conversion live in [`config`].
//! HTML response parsing lives in [`response_parser`].
//! Row-level post-processing lives in [`row_extractor`].

use anyhow::Result;
use async_trait::async_trait;
use futures::stream::{self, StreamExt};
use serde::Deserialize;
use std::collections::HashMap;

use crate::scrapyfy::*;
use crate::scrapyfy::scraper_html::config::HtmlScraperQueryRaw;
use crate::scrapyfy::scraper_html::entry::{HtmlScraperEntry, HtmlScraperSelectMode};
use crate::scrapyfy::scraper_json::entry::{json_value_to_strings, select_json_values};
use crate::scrapyfy::scraper_json::query::{ScraperRequestHeader, ScraperRequestMethod};
use crate::scrapyfy::query_helpers::{self, QueryTemplateParamMapping};

use crate::scrapyfy::scraper_data_node::ScraperDataNode;
use crate::scrapyfy::{HttpClient, ScraperPostProcess, ScraperPostProcessContext};
use ::scraper::Selector;

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
    pub(crate) fn validate_request_actions(
        name: &str,
        actions: &[ScraperAction],
    ) -> Result<()> {
        for action in actions {
            action.validate(name, "query")?;
        }

        Ok(())
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
        self.scraper_entries()
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

    /// Resolves the full set of request headers from templates and collection params.
    ///
    /// Each header value is derived from its JSON pointer against the params context,
    /// then processed through the header's action pipeline.
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

    /// Returns the optional group field whose items should become query rows.
    pub fn result_item_field(&self) -> Option<&str> {
        self.result_item_field.as_deref()
    }
}

#[async_trait]
impl ScraperManagerQuery for HtmlScraperQuery {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn media_types(&self) -> &Vec<String> {
        &self.media_types
    }

    fn query_url(&self) -> &str {
        &self.query_url
    }

    fn scraper_entries(&self) -> &Vec<HtmlScraperEntry> {
        &self.scraper_entries
    }

    fn is_media_type(&self, media_types: &[String]) -> bool {
        media_types
            .iter()
            .any(|media_type| self.media_types().contains(media_type))
    }

    #[cfg(any(test, feature = "test-support"))]
    fn get_field_names(&self) -> Vec<String> {
        self.scraper_entries()
            .iter()
            .flat_map(HtmlScraperEntry::field_names)
            .collect()
    }

    async fn execute_query(
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
        let html = self
            .http_client
            .query_http_for_request(
                self.request_method.as_http_method(),
                &query_url,
                &request_headers,
                request_body.as_deref(),
            )
            .await?;

        let roots = parse_html_rows(
            &html,
            &self.row_selector_compiled,
            self.scraper_entries(),
            &resolved_params,
            &query_url,
        );

        let mut jobs = Vec::with_capacity(roots.len());
        for (root_index, root) in roots.into_iter().enumerate() {
            jobs.push(process_root(
                root_index,
                root,
                &resolved_params,
                &query_url,
                &html,
                fields_filters,
                &self.post_processes,
                &self.http_client,
            ));
        }

        let processed_roots = stream::iter(jobs)
            .buffer_unordered(self.row_concurrency)
            .collect::<Vec<_>>()
            .await;

        let mut results: Vec<HashMap<String, ScraperDataNode>> = Vec::new();
        for maybe_root in collect_ordered_results(processed_roots)? {
            if let Some(root) = maybe_root {
                results.push(root);
            }
        }

        Ok(results)
    }
}

/// Runs the post-process pipeline on a single root node and optionally filters it.
///
/// Returns `None` when the node is excluded by `fields_filters`.
///
/// # Arguments
///
/// * `root_index` - Original index used to restore ordering after async processing.
/// * `root` - Extracted data node to post-process.
/// * `params` - Runtime template parameters forwarded to post-processors.
/// * `query_url` - Fully resolved request URL forwarded to post-processors.
/// * `html` - Raw HTML response body available to post-processors.
/// * `fields_filters` - Optional per-field filter list; when present the root is
///   discarded if it does not match.
/// * `post_processes` - Ordered post-processing steps applied to the root.
/// * `http_client` - HTTP client available for post-processors that need to fetch.
///
/// # Errors
///
/// Returns an error if any post-process step fails.
async fn process_root(
    root_index: usize,
    mut root: ScraperDataNode,
    params: &HashMap<String, String>,
    query_url: &str,
    html: &str,
    fields_filters: Option<&HashMap<String, Vec<String>>>,
    post_processes: &[ScraperPostProcess],
    http_client: &HttpClient,
) -> Result<(usize, Option<HashMap<String, ScraperDataNode>>)> {
    for post_process in post_processes {
        post_process
            .apply(
                &mut root,
                &ScraperPostProcessContext {
                    params,
                    request_url: query_url,
                    response_body: Some(html),
                    http_client,
                },
            )
            .await?;
    }

    if fields_filters.is_none()
        || !query_helpers::is_root_filtered(&root, fields_filters.unwrap())
    {
        Ok((root_index, Some(root.children)))
    } else {
        Ok((root_index, None))
    }
}


/// Restores the original row order after collecting indexed async results.
///
/// # Arguments
///
/// * `results` - Indexed results produced by an async stream; may arrive out of order.
///
/// # Errors
///
/// Returns the first error encountered while unwrapping the results.
fn collect_ordered_results<T>(results: Vec<Result<(usize, T)>>) -> Result<Vec<T>> {
    let mut ordered = Vec::with_capacity(results.len());

    for result in results {
        ordered.push(result?);
    }

    ordered.sort_by_key(|(index, _)| *index);

    Ok(ordered.into_iter().map(|(_, value)| value).collect())
}

/// Parses an HTML response body and extracts [`ScraperDataNode`] rows using
/// the given CSS selector and entry definitions.
///
/// Each matched element is passed to every entry extractor, and the resulting
/// data nodes are returned in document order.
///
/// # Arguments
///
/// * `html` - Raw HTML response body to parse.
/// * `row_selector` - Compiled CSS selector matching each result row.
/// * `entries` - Field extractors applied to every matched row.
/// * `params` - Runtime template parameters forwarded to entry actions.
/// * `query_url` - Fully resolved request URL forwarded to entry actions.
fn parse_html_rows(
    html: &str,
    row_selector: &Selector,
    entries: &[HtmlScraperEntry],
    params: &HashMap<String, String>,
    query_url: &str,
) -> Vec<ScraperDataNode> {
    let doc = ::scraper::Html::parse_document(html);
    let mut roots = Vec::new();

    for card in doc.select(row_selector) {
        let mut root = ScraperDataNode::default();
        for entry in entries {
            entry.apply_to(&mut root, card, params, query_url, Some(html));
        }
        roots.push(root);
    }

    roots
}
