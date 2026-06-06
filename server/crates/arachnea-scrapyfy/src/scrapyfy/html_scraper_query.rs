use anyhow::Result;
use async_trait::async_trait;
use futures::stream::{self, StreamExt};
use scraper::Selector;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;

use super::*;
use crate::scrapyfy::html_scraper_entry::HtmlScraperSelectMode;
use crate::scrapyfy::json_scraper_entry::{json_value_to_strings, select_json_values};
use crate::scrapyfy::json_scraper_query::{
    ScraperRequestHeader, ScraperRequestHeaderRaw, ScraperRequestMethod,
};
use crate::scrapyfy::query_helpers::{self, QueryTemplateParamMapping};

/// Raw configuration definition of one query endpoint.
#[derive(Serialize, Deserialize)]
pub struct HtmlScraperQueryRaw {
    name: String,
    base_url: String,
    #[serde(skip)]
    resolved_base_url: Option<String>,
    media_types: Vec<String>,
    query_url: String,
    #[serde(default = "default_html_request_method")]
    request_method: ScraperRequestMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request_body_pointer: Option<String>,
    #[serde(default = "default_html_select_mode")]
    request_body_select: HtmlScraperSelectMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    request_body_actions: Vec<ScraperAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    request_headers: Vec<ScraperRequestHeaderRaw>,
    #[serde(default, skip_serializing_if = "ScraperHttpConfig::is_empty")]
    http: ScraperHttpConfig,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    query_param_mappings: Vec<QueryTemplateParamMapping>,
    #[serde(default = "default_html_row_concurrency")]
    row_concurrency: usize,
    row_selector: String,
    #[serde(skip)]
    resolved_row_selector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result_item_field: Option<String>,
    entries: Vec<HtmlScraperEntryRaw>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    post_process: Vec<ScraperPostProcess>,
}

impl HtmlScraperQueryRaw {
    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// Resolves collection-level placeholders used by this HTML query config.
    pub(crate) fn resolve_collection_params(
        &mut self,
        params: &HashMap<String, String>,
    ) -> Result<()> {
        self.resolved_base_url = Some(query_helpers::resolve_required_template(
            "HTML query",
            &self.name,
            "base_url",
            &self.base_url,
            params,
        )?);
        self.resolved_row_selector = Some(query_helpers::resolve_required_template(
            "HTML query",
            &self.name,
            "row_selector",
            &self.row_selector,
            params,
        )?);

        for entry in &mut self.entries {
            entry.resolve_collection_params(params)?;
        }
        self.http
            .resolve_collection_params("HTML query", &self.name, params)?;

        Ok(())
    }

    /// Merges the collection-level HTTP configuration into this query's own config.
    pub(crate) fn apply_collection_http(&mut self, collection_http: &ScraperHttpConfig) {
        self.http = collection_http.merge(&self.http);
    }
}

/// A query definition for one source and one result shape.
#[derive(Deserialize)]
#[serde(try_from = "HtmlScraperQueryRaw")]
pub struct HtmlScraperQuery {
    name: String,
    base_url: String,
    base_url_template: String,
    media_types: Vec<String>,
    query_url: String,
    request_method: ScraperRequestMethod,
    request_body_pointer: Option<String>,
    request_body_select: HtmlScraperSelectMode,
    request_body_actions: Vec<ScraperAction>,
    request_headers: Vec<ScraperRequestHeader>,
    http_config: ScraperHttpConfig,
    query_param_mappings: Vec<QueryTemplateParamMapping>,
    row_concurrency: usize,
    row_selector_template: String,
    row_selector_compiled: Selector,
    result_item_field: Option<String>,
    scraper_entries: Vec<HtmlScraperEntry>,
    post_processes: Vec<ScraperPostProcess>,

    http_client: HttpClient,
}

/// Defaults to [`HtmlScraperSelectMode::All`].
fn default_html_select_mode() -> HtmlScraperSelectMode {
    HtmlScraperSelectMode::All
}

/// Defaults to [`ScraperRequestMethod::Get`].
fn default_html_request_method() -> ScraperRequestMethod {
    ScraperRequestMethod::Get
}

/// Defaults to `4`.
fn default_html_row_concurrency() -> usize {
    4
}

/// Restores the original row order after collecting indexed async results.
fn collect_ordered_results<T>(results: Vec<Result<(usize, T)>>) -> Result<Vec<T>> {
    let mut ordered = Vec::with_capacity(results.len());

    for result in results {
        ordered.push(result?);
    }

    ordered.sort_by_key(|(index, _)| *index);

    Ok(ordered.into_iter().map(|(_, value)| value).collect())
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
    fn validate_request_actions(name: &str, actions: &[ScraperAction]) -> Result<()> {
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
            default_html_row_concurrency(),
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

        let row_selector_compiled = Selector::parse(row_selector)
            .map_err(|err| anyhow::anyhow!("Invalid row selector {}: {:?}", row_selector, err))?;

        let mut query = HtmlScraperQuery {
            name: name.to_string(),
            base_url: base_url.to_string(),
            base_url_template: base_url.to_string(),
            media_types,
            http_client: HttpClient::new(base_url),
            http_config: ScraperHttpConfig::default(),
            query_url: query_url.to_string(),
            request_method: default_html_request_method(),
            request_body_pointer: None,
            request_body_select: default_html_select_mode(),
            request_body_actions: Vec::new(),
            request_headers: Vec::new(),
            query_param_mappings: Vec::new(),
            row_concurrency,
            row_selector_template: row_selector.to_string(),
            row_selector_compiled,
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
    async fn process_root(
        &self,
        root_index: usize,
        mut root: ScraperDataNode,
        params: &HashMap<String, String>,
        query_url: &str,
        html: &str,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
    ) -> Result<(usize, Option<HashMap<String, ScraperDataNode>>)> {
        for post_process in &self.post_processes {
            post_process
                .apply(
                    &mut root,
                    &ScraperPostProcessContext {
                        params,
                        request_url: query_url,
                        response_body: Some(html),
                        http_client: &self.http_client,
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

    /// Returns the optional group field whose items should become query rows.
    pub fn result_item_field(&self) -> Option<&str> {
        self.result_item_field.as_deref()
    }
}

#[async_trait]
impl ScraperQuery for HtmlScraperQuery {
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
        let roots: Vec<ScraperDataNode> = {
            let doc = scraper::Html::parse_document(&html);
            let mut roots: Vec<ScraperDataNode> = Vec::new();

            for card in doc.select(&self.row_selector_compiled) {
                let mut root = ScraperDataNode::default();
                for scraper in self.scraper_entries() {
                    scraper.apply_to(&mut root, card, &resolved_params, &query_url, Some(&html));
                }
                roots.push(root);
            }

            roots
        };
        let mut jobs = Vec::with_capacity(roots.len());
        for (root_index, root) in roots.into_iter().enumerate() {
            jobs.push(self.process_root(
                root_index,
                root,
                &resolved_params,
                &query_url,
                &html,
                fields_filters,
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

impl TryFrom<HtmlScraperQueryRaw> for HtmlScraperQuery {
    type Error = anyhow::Error;

    /// Converts a raw YAML query definition into a validated runtime query.
    ///
    /// # Errors
    ///
    /// Returns an error if the row selector, entries, or request headers are invalid.
    fn try_from(config: HtmlScraperQueryRaw) -> Result<Self> {
        let HtmlScraperQueryRaw {
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
            row_concurrency,
            row_selector,
            resolved_row_selector,
            result_item_field,
            entries,
            post_process,
        } = config;

        let entries = entries
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;
        let request_headers = request_headers
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;
        let resolved_base_url = query_helpers::resolved_or_template(&base_url, resolved_base_url);
        let resolved_row_selector =
            query_helpers::resolved_or_template(&row_selector, resolved_row_selector);

        HtmlScraperQuery::validate_request_actions(&name, &request_body_actions)?;

        let mut query = HtmlScraperQuery::try_new(
            &name,
            &resolved_base_url,
            media_types,
            &query_url,
            row_concurrency,
            &resolved_row_selector,
            entries,
        )?;
        query.base_url_template = base_url;
        query.request_method = request_method;
        query.request_body_pointer = request_body_pointer;
        query.request_body_select = request_body_select;
        query.request_body_actions = request_body_actions;
        query.request_headers = request_headers;
        query.http_config = http.clone();
        query.http_client = HttpClient::with_http_config(http);
        query.row_selector_template = row_selector;
        query.query_param_mappings = query_param_mappings;
        query.result_item_field = result_item_field;

        for post_process in post_process {
            post_process.validate(&name)?;
            query.post_processes.push(post_process);
        }

        Ok(query)
    }
}

impl From<&HtmlScraperQuery> for HtmlScraperQueryRaw {
    /// Converts a runtime query back into its raw YAML-compatible representation.
    fn from(query: &HtmlScraperQuery) -> Self {
        Self {
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
                .map(ScraperRequestHeaderRaw::from)
                .collect(),
            http: query.http_config.clone(),
            query_param_mappings: query.query_param_mappings.clone(),
            row_concurrency: query.row_concurrency,
            row_selector: query.row_selector_template.clone(),
            resolved_row_selector: None,
            result_item_field: query.result_item_field.clone(),
            entries: query
                .scraper_entries
                .iter()
                .map(HtmlScraperEntryRaw::from)
                .collect(),
            post_process: query.post_processes.clone(),
        }
    }
}

impl Serialize for HtmlScraperQuery {
    /// Serializes the query through its raw YAML representation.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        HtmlScraperQueryRaw::from(self).serialize(serializer)
    }
}