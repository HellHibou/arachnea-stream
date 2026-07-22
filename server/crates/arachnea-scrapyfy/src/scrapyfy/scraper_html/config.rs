//! Raw YAML configuration types and validated runtime conversions for HTML queries.
//!
//! This module defines the deserialization target ([`HtmlScraperQueryRaw`]) and the
//! bidirectional conversion between raw YAML config and the validated runtime type
//! ([`HtmlScraperQuery`]).

use anyhow::Result;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;

use crate::scrapyfy::query_helpers;
use crate::scrapyfy::scraper::config::{
    ScraperQueryCommon, ScraperRequestHeaderRaw, SubQueryCommon,
};
use crate::scrapyfy::scraper_html::entry::HtmlScraperEntryRaw;
use crate::scrapyfy::scraper_html::query::HtmlScraperQuery;
use crate::scrapyfy::{HttpClient, ScraperHttpConfig};

/// Defaults to `4` for the maximum number of concurrent row post-processing operations.
///
/// This default value is used when `row_concurrency` is not specified in the
/// HTML scraper query configuration. It controls how many extracted rows can
/// have their async post-process steps running simultaneously.
///
/// # Returns
///
/// The default concurrency limit of 4.
fn default_html_row_concurrency() -> usize {
    4
}

/// Raw configuration definition of one HTML query endpoint.
///
/// Deserialized from YAML and converted into [`HtmlScraperQuery`] via
/// [`TryFrom`].
#[derive(Serialize, Deserialize)]
pub struct HtmlScraperQueryRaw {
    /// Common configuration fields shared with JSON scraper.
    #[serde(flatten)]
    pub common: ScraperQueryCommon,
    /// Maximum number of rows whose async post-process steps may run together.
    #[serde(default = "default_html_row_concurrency")]
    pub row_concurrency: usize,
    /// CSS selector matching each result row in the fetched page.
    pub row_selector: String,
    /// Resolved row selector after collection-level placeholder substitution.
    #[serde(skip)]
    pub resolved_row_selector: Option<String>,
    /// Optional template providing HTML content directly, bypassing the HTTP
    /// fetch. When set, the template is resolved with runtime parameters
    /// (typically `{html}`) and the resulting HTML is parsed directly without
    /// an HTTP request. `query_url` is still used as the context URL for
    /// actions, headers, and error messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_html: Option<String>,
    /// Field extractors executed for every matched row.
    pub entries: Vec<HtmlScraperEntryRaw>,
}

impl HtmlScraperQueryRaw {
    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        self.common.name.clone()
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
        self.common
            .resolve_collection_params("HTML query", params)?;

        self.resolved_row_selector = Some(query_helpers::resolve_required_template(
            "HTML query",
            &self.common.name,
            "row_selector",
            &self.row_selector,
            params,
        )?);

        for entry in &mut self.entries {
            entry.resolve_collection_params(params)?;
        }

        Ok(())
    }

    /// Merges the collection-level HTTP configuration into this query's own config.
    ///
    /// # Arguments
    ///
    /// * `collection_http` - HTTP configuration inherited from the parent collection.
    pub(crate) fn apply_collection_http(&mut self, collection_http: &ScraperHttpConfig) {
        self.common.apply_collection_http(collection_http);
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
            common,
            row_concurrency,
            row_selector,
            resolved_row_selector,
            input_html,
            entries,
        } = config;

        let ScraperQueryCommon {
            name,
            base_url,
            resolved_base_url,
            media_types,
            query_url,
            request_url_actions,
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

        let media_types = media_types.unwrap_or_default();

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
        HtmlScraperQuery::validate_request_actions(&name, &request_url_actions)?;

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
        query.request_url_actions = request_url_actions;
        query.request_body_pointer = request_body_pointer;
        query.request_body_select = request_body_select;
        query.request_body_actions = request_body_actions;
        query.request_headers = request_headers;
        query.http_config = http.clone();
        query.http_client = HttpClient::with_http_config(http);
        query.row_selector_template = row_selector;
        query.input_html = input_html;
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
    ///
    /// Serializes the validated runtime query back to a form that can be
    /// serialized to YAML. This preserves the configuration structure but loses
    /// runtime-only information like compiled selectors.
    ///
    /// # Arguments
    ///
    /// * `query` - Runtime query to convert.
    ///
    /// # Returns
    ///
    /// A raw query struct suitable for YAML serialization.
    fn from(query: &HtmlScraperQuery) -> Self {
        Self {
            common: ScraperQueryCommon {
                name: query.name.clone(),
                base_url: query.base_url_template.clone(),
                resolved_base_url: None,
                media_types: Some(query.media_types.clone()),
                query_url: query.query_url.clone(),
                request_url_actions: query.request_url_actions.clone(),
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
            row_concurrency: query.row_concurrency,
            row_selector: query.row_selector_template.clone(),
            resolved_row_selector: None,
            input_html: query.input_html.clone(),
            entries: query
                .scraper_entries
                .iter()
                .map(HtmlScraperEntryRaw::from)
                .collect(),
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

// ---------------------------------------------------------------------------
// Sub-query raw type for HTML (symmetrical with JsonScraperSubQueryRaw)
// ---------------------------------------------------------------------------

/// Raw configuration definition of one chained HTML follow-up request.
///
/// Same field set as [`crate::scrapyfy::scraper_json::config::EntrySubQueryRaw::Html`]
/// but used at the query level for symmetry with JSON scraper.
/// Currently HTML queries don't have root-level sub-queries, but this type
/// is provided for future extensibility and code consistency.
#[derive(Serialize, Deserialize)]
pub struct HtmlScraperSubQueryRaw {
    /// Common configuration fields shared with EntrySubQueryRaw.
    ///
    /// Includes settings like context pointer, filters, target path, request configuration,
    /// and HTTP settings that are common across all sub-query types.
    #[serde(flatten)]
    pub common: SubQueryCommon,
    /// CSS selector matching each result row in the follow-up response.
    ///
    /// This selector is used to find individual data rows in the HTML response
    /// fetched by the sub-query.
    pub row_selector: String,
    /// Field extractors executed for every matched row.
    ///
    /// These entries extract data from each row matched by the `row_selector`
    /// in the follow-up response.
    pub entries: Vec<HtmlScraperEntryRaw>,
    /// Entries extracted from the context row rather than from fetched rows.
    ///
    /// These entries extract data from the parent context row that triggered
    /// the sub-query, rather than from the fetched HTML response.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_entries: Vec<HtmlScraperEntryRaw>,
    /// Nested sub-queries (recursion).
    ///
    /// Child sub-queries that are executed for each row matched by this sub-query,
    /// allowing for arbitrary levels of nested data fetching.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub_queries: Vec<HtmlScraperSubQueryRaw>,
}

impl TryFrom<HtmlScraperSubQueryRaw> for crate::scrapyfy::scraper_html::query::HtmlScraperSubQuery {
    type Error = anyhow::Error;

    /// Converts a raw YAML sub-query definition into a validated runtime sub-query.
    ///
    /// Validates the sub-query configuration and compiles the CSS selector
    /// for efficient matching. Also converts all child entries and nested sub-queries.
    ///
    /// # Arguments
    ///
    /// * `config` - Raw YAML sub-query definition to convert.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The row selector is empty
    /// - The target path is empty
    /// - The row selector is not a valid CSS selector
    /// - Any entry or sub-query fails validation
    fn try_from(config: HtmlScraperSubQueryRaw) -> Result<Self> {
        let HtmlScraperSubQueryRaw {
            common,
            row_selector,
            entries,
            context_entries,
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

        if row_selector.is_empty() {
            anyhow::bail!("Invalid HTML sub-query: row_selector cannot be empty");
        }
        if target
            .as_deref()
            .is_some_and(|target| target.trim().is_empty())
        {
            anyhow::bail!("Invalid HTML sub-query: target cannot be empty");
        }

        let selector = ::scraper::Selector::parse(&row_selector)
            .map_err(|e| anyhow::anyhow!("Invalid row_selector '{}': {}", row_selector, e))?;
        let entries: Vec<crate::scrapyfy::scraper_html::entry::HtmlScraperEntry> = entries
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;
        let context_entries: Vec<crate::scrapyfy::scraper_html::entry::HtmlScraperEntry> =
            context_entries
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>>>()?;
        let request_headers: Vec<crate::scrapyfy::scraper::config::ScraperRequestHeader> =
            request_headers
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>>>()?;
        let sub_queries: Vec<Box<dyn crate::scrapyfy::scraper::query_trait::ScraperQuery>> =
            sub_queries
                .into_iter()
                .map(|raw| -> Result<_> {
                    let runtime: crate::scrapyfy::scraper_html::query::HtmlScraperSubQuery =
                        raw.try_into()?;
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
            request_headers,
            http_config: http,
            row_selector: row_selector.clone(),
            row_selector_compiled: selector,
            entries,
            post_processes: post_process,
            sub_queries,
            http_client: HttpClient::new(""),
        })
    }
}

impl From<&crate::scrapyfy::scraper_html::query::HtmlScraperSubQuery> for HtmlScraperSubQueryRaw {
    /// Converts a runtime sub-query back into its raw YAML-compatible representation.
    ///
    /// Serializes the validated runtime sub-query back to a form that can be
    /// serialized to YAML. This preserves the configuration structure but loses
    /// runtime-only information like compiled selectors.
    ///
    /// # Arguments
    ///
    /// * `sub_query` - Runtime sub-query to convert.
    ///
    /// # Returns
    ///
    /// A raw sub-query struct suitable for YAML serialization.
    ///
    /// # Panics
    ///
    /// Panics if a nested sub-query is not a [`HtmlScraperSubQuery`].
    /// This should not occur during normal operation as HTML sub-queries
    /// should only contain other HTML sub-queries.
    fn from(sub_query: &crate::scrapyfy::scraper_html::query::HtmlScraperSubQuery) -> Self {
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
            row_selector: sub_query.row_selector.clone(),
            entries: sub_query
                .entries
                .iter()
                .map(HtmlScraperEntryRaw::from)
                .collect(),
            context_entries: sub_query
                .context_entries
                .iter()
                .map(HtmlScraperEntryRaw::from)
                .collect(),
            sub_queries: sub_query
                .sub_queries
                .iter()
                .map(|child| {
                    child
                        .as_any()
                        .downcast_ref::<crate::scrapyfy::scraper_html::query::HtmlScraperSubQuery>()
                        .map(HtmlScraperSubQueryRaw::from)
                        .unwrap_or_else(|| {
                            panic!(
                                "HtmlScraperSubQuery::from: nested sub-query '{}' is not a HtmlScraperSubQuery",
                                child.name()
                            )
                        })
                })
                .collect(),
        }
    }
}
