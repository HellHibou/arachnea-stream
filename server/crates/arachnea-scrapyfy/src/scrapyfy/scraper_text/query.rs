//! Runtime query type for text scraper endpoints.
//!
//! A [`TextScraperQuery`] fetches a text payload and splits it into rows using
//! a `row_delimiter`, then into fields using an optional `field_delimiter`.

use std::any::Any;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::scrapyfy::query_helpers::QueryTemplateParamMapping;
use crate::scrapyfy::scraper::config::{
    ScraperQueryCommon, ScraperRequestHeader, ScraperRequestHeaderRaw, ScraperRequestMethod,
};
use crate::scrapyfy::scraper::entry_trait::ScraperEntrySpec;
use crate::scrapyfy::scraper::query_trait::ScraperQuery;
use crate::scrapyfy::scraper::row_locator::{RowLocator, ScraperType};
use crate::scrapyfy::scraper_text::entry::TextScraperEntryRaw;
use crate::scrapyfy::*;

/// A query definition for one text source and one result shape.
///
/// Represents a validated text scraper query that fetches a text payload and
/// extracts structured data using row/field delimiters.
#[derive(Deserialize)]
#[serde(try_from = "TextScraperQueryRaw")]
pub struct TextScraperQuery {
    /// Query identifier used as the lookup key in a collection.
    pub(crate) name: String,
    /// Resolved base URL of the source.
    pub(crate) base_url: String,
    /// Content types this query produces.
    pub(crate) media_types: Vec<String>,
    /// URL template used to build the request.
    pub(crate) query_url: String,
    /// HTTP method used to issue the request.
    pub(crate) request_method: ScraperRequestMethod,
    /// HTTP headers attached to the request.
    pub(crate) request_headers: Vec<ScraperRequestHeader>,
    /// HTTP client configuration (mode, user agent, max redirects).
    pub(crate) http_config: ScraperHttpConfig,
    /// Source-to-target parameter mappings applied before template resolution.
    pub(crate) query_param_mappings: Vec<QueryTemplateParamMapping>,
    /// Delimiter splitting the body into rows.
    pub(crate) row_delimiter: String,
    /// Optional delimiter splitting each row into fields.
    pub(crate) field_delimiter: Option<String>,
    /// Optional group field whose items should become query rows.
    pub(crate) result_item_field: Option<String>,
    /// Field extractors executed for every row.
    pub(crate) scraper_entries: Vec<crate::scrapyfy::scraper_text::entry::TextScraperEntry>,
    /// HTTP client used to issue requests.
    pub(crate) http_client: HttpClient,
}

impl TextScraperQuery {
    /// Validates every entry and stores the configured field extractors.
    ///
    /// # Arguments
    ///
    /// * `name` - Query identifier used to look up this query in a collection.
    /// * `base_url` - Base URL of the source, also available as `{base_url}` in templates.
    /// * `media_types` - Content type result.
    /// * `query_url` - URL template used to build the request.
    /// * `row_delimiter` - Delimiter splitting the body into rows.
    /// * `field_delimiter` - Optional delimiter splitting each row into fields.
    /// * `result_item_field` - Optional group field hoisted to top level.
    /// * `scraper_entries` - Field extractors executed for every row.
    ///
    /// # Errors
    ///
    /// Returns an error if any entry fails validation.
    pub fn try_new(
        name: &str,
        base_url: &str,
        media_types: Vec<String>,
        query_url: &str,
        row_delimiter: &str,
        field_delimiter: Option<String>,
        result_item_field: Option<String>,
        scraper_entries: Vec<crate::scrapyfy::scraper_text::entry::TextScraperEntry>,
    ) -> Result<Self> {
        for entry in &scraper_entries {
            entry.validate()?;
        }

        Ok(Self {
            name: name.to_string(),
            base_url: base_url.to_string(),
            media_types,
            query_url: query_url.to_string(),
            request_method: ScraperRequestMethod::Get,
            request_headers: Vec::new(),
            http_config: ScraperHttpConfig::default(),
            query_param_mappings: Vec::new(),
            row_delimiter: row_delimiter.to_string(),
            field_delimiter,
            result_item_field,
            scraper_entries,
            http_client: HttpClient::new(base_url),
        })
    }

    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// Returns the flattened list of leaf field names produced by this query.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_field_names(&self) -> Vec<String> {
        self.scraper_entries
            .iter()
            .flat_map(|e| {
                let mut names = Vec::new();
                collect_text_field_names(e, None, &mut names);
                names
            })
            .collect()
    }
}

#[cfg(any(test, feature = "test-support"))]
fn collect_text_field_names(
    entry: &crate::scrapyfy::scraper_text::entry::TextScraperEntry,
    prefix: Option<&str>,
    names: &mut Vec<String>,
) {
    let full_name = match prefix {
        Some(prefix) => format!("{} > {}", prefix, entry.name()),
        None => entry.name().to_string(),
    };
    match entry {
        crate::scrapyfy::scraper_text::entry::TextScraperEntry::Field { .. } => {
            names.push(full_name)
        }
        crate::scrapyfy::scraper_text::entry::TextScraperEntry::Group { entries, .. } => {
            for child in entries {
                collect_text_field_names(child, Some(&full_name), names);
            }
        }
    }
}

impl ScraperQuery for TextScraperQuery {
    fn scraper_type(&self) -> ScraperType {
        ScraperType::Text
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
        None
    }

    fn request_actions(&self) -> &[crate::scrapyfy::actions::ScraperAction] {
        &[]
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
        RowLocator::Single
    }

    fn row_delimiter(&self) -> Option<&str> {
        Some(&self.row_delimiter)
    }

    fn field_delimiter(&self) -> Option<&str> {
        self.field_delimiter.as_deref()
    }

    fn post_processes(&self) -> &[crate::scrapyfy::ScraperPostProcess] {
        &[]
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
        Vec::new()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Raw configuration definition of one text query endpoint.
#[derive(Serialize, Deserialize)]
pub struct TextScraperQueryRaw {
    /// Common configuration fields shared with other scraper types.
    #[serde(flatten)]
    pub common: ScraperQueryCommon,
    /// Delimiter splitting the body into rows.
    pub row_delimiter: String,
    /// Optional delimiter splitting each row into fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_delimiter: Option<String>,
    /// Field extractors executed for every row.
    pub entries: Vec<TextScraperEntryRaw>,
}

impl TextScraperQueryRaw {
    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        self.common.name.clone()
    }

    /// Resolves collection-level placeholders used by this text query config.
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
            .resolve_collection_params("text query", params)?;
        for entry in &mut self.entries {
            entry.resolve_collection_params(params)?;
        }
        Ok(())
    }

    /// Merges the collection-level HTTP configuration into this query's own config.
    pub(crate) fn apply_collection_http(&mut self, collection_http: &ScraperHttpConfig) {
        self.common.apply_collection_http(collection_http);
    }
}

impl TryFrom<TextScraperQueryRaw> for TextScraperQuery {
    type Error = anyhow::Error;

    /// Converts a raw YAML query definition into a validated runtime query.
    ///
    /// # Errors
    ///
    /// Returns an error if any entry fails validation or placeholders are missing.
    fn try_from(config: TextScraperQueryRaw) -> Result<Self> {
        let TextScraperQueryRaw {
            common,
            row_delimiter,
            field_delimiter,
            entries,
        } = config;

        let ScraperQueryCommon {
            name,
            base_url,
            resolved_base_url,
            media_types,
            query_url,
            http,
            result_item_field,
            ..
        } = common;

        let media_types = media_types.unwrap_or_default();

        let resolved_base_url =
            crate::scrapyfy::query_helpers::resolved_or_template(&base_url, resolved_base_url);
        let entries = entries
            .into_iter()
            .map(|e| e.try_into())
            .collect::<Result<Vec<_>>>()?;

        let mut query = TextScraperQuery::try_new(
            &name,
            &resolved_base_url,
            media_types,
            &query_url,
            &row_delimiter,
            field_delimiter,
            result_item_field,
            entries,
        )?;
        query.http_config = http.clone();
        query.http_client = HttpClient::with_http_config(http);
        Ok(query)
    }
}

impl From<&TextScraperQuery> for TextScraperQueryRaw {
    /// Converts a runtime query back into its raw YAML-compatible representation.
    fn from(query: &TextScraperQuery) -> Self {
        Self {
            common: ScraperQueryCommon {
                name: query.name.clone(),
                base_url: query.base_url.clone(),
                resolved_base_url: None,
                media_types: Some(query.media_types.clone()),
                query_url: query.query_url.clone(),
                request_method: query.request_method,
                request_body_pointer: None,
                request_body_select:
                    crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode::All,
                request_body_actions: Vec::new(),
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
                post_process: Vec::new(),
            },
            row_delimiter: query.row_delimiter.clone(),
            field_delimiter: query.field_delimiter.clone(),
            entries: query
                .scraper_entries
                .iter()
                .map(TextScraperEntryRaw::from)
                .collect(),
        }
    }
}
