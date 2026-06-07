//! Raw YAML configuration types and validated runtime conversions for HTML queries.
//!
//! This module defines the deserialization target ([`HtmlScraperQueryRaw`]) and the
//! bidirectional conversion between raw YAML config and the validated runtime type
//! ([`HtmlScraperQuery`]).

use anyhow::Result;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;

use crate::scrapyfy::scraper_html::entry::{HtmlScraperEntryRaw, HtmlScraperSelectMode};
use crate::scrapyfy::scraper_html::query::HtmlScraperQuery;
use crate::scrapyfy::scraper_json::query::{
    ScraperRequestHeaderRaw, ScraperRequestMethod,
};
use crate::scrapyfy::query_helpers::{self, QueryTemplateParamMapping};
use crate::scrapyfy::{HttpClient, ScraperAction, ScraperHttpConfig, ScraperPostProcess};

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

/// Raw configuration definition of one HTML query endpoint.
///
/// Deserialized from YAML and converted into [`HtmlScraperQuery`] via
/// [`TryFrom`].
#[derive(Serialize, Deserialize)]
pub struct HtmlScraperQueryRaw {
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
    /// HTTP method used to issue the request.
    #[serde(default = "default_html_request_method")]
    request_method: ScraperRequestMethod,
    /// Optional JSON pointer selecting the request body from runtime params.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request_body_pointer: Option<String>,
    /// Selection mode for the request body pointer.
    #[serde(default = "default_html_select_mode")]
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
    /// Maximum number of rows whose async post-process steps may run together.
    #[serde(default = "default_html_row_concurrency")]
    row_concurrency: usize,
    /// CSS selector matching each result row in the fetched page.
    row_selector: String,
    /// Resolved row selector after collection-level placeholder substitution.
    #[serde(skip)]
    resolved_row_selector: Option<String>,
    /// Optional group field whose items should become query rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result_item_field: Option<String>,
    /// Field extractors executed for every matched row.
    entries: Vec<HtmlScraperEntryRaw>,
    /// Post-processing steps applied to each extracted row.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    post_process: Vec<ScraperPostProcess>,
}

impl HtmlScraperQueryRaw {

    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// Resolves collection-level placeholders used by this HTML query config.
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
    ///
    /// # Arguments
    ///
    /// * `collection_http` - HTTP configuration inherited from the parent collection.
    pub(crate) fn apply_collection_http(&mut self, collection_http: &ScraperHttpConfig) {
        self.http = collection_http.merge(&self.http);
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
    ///
    /// # Arguments
    ///
    /// * `serializer` - Serializer target.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        HtmlScraperQueryRaw::from(self).serialize(serializer)
    }
}