//! Common query configuration structures shared by HTML and JSON scrapers.
//!
//! This module provides reusable components to reduce duplication between
//! [`HtmlScraperQuery`] and [`JsonScraperQuery`] implementations.

use std::collections::HashMap;

use crate::scrapyfy::actions::ScraperAction;
use crate::scrapyfy::post_processes::ScraperPostProcess;
use crate::scrapyfy::scraper::config::{ScraperRequestHeader, ScraperRequestMethod};
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;
use crate::scrapyfy::{HttpClient, ScraperHttpConfig};

/// Base configuration shared by HTML and JSON scraper queries.
///
/// Contains all fields that are common between [`super::scraper_html::query::HtmlScraperQuery`]
/// and [`super::scraper_json::query::JsonScraperQuery`], excluding type-specific
/// fields like row selectors or pointers.
///
/// This struct is embedded as a field in both query types to enable code reuse
/// and maintain a single source of truth for common configuration.
#[derive(Clone)]
pub struct BaseQueryConfig {
    /// Query identifier used as the lookup key in a collection.
    pub name: String,
    /// Resolved base URL of the source.
    pub base_url: String,
    /// Original base URL template preserved from YAML.
    pub base_url_template: String,
    /// Content types this query produces.
    pub media_types: Vec<String>,
    /// URL template used to build the request.
    pub query_url: String,
    /// HTTP method used to issue the request.
    pub request_method: ScraperRequestMethod,
    /// Optional JSON pointer selecting the request body from runtime params.
    pub request_body_pointer: Option<String>,
    /// Selection mode for the request body pointer.
    pub request_body_select: HtmlScraperSelectMode,
    /// Actions applied to the request body before the HTTP call.
    pub request_body_actions: Vec<ScraperAction>,
    /// HTTP headers attached to the request.
    pub request_headers: Vec<ScraperRequestHeader>,
    /// HTTP client configuration (mode, user agent, max redirects).
    pub http_config: ScraperHttpConfig,
    /// HTTP client used to issue requests.
    pub http_client: HttpClient,
    /// Optional group field whose items should become query rows.
    pub result_item_field: Option<String>,
    /// Post-processing steps applied to each extracted row.
    pub post_processes: Vec<ScraperPostProcess>,
}

impl BaseQueryConfig {
    /// Creates a new base configuration with default values.
    ///
    /// # Arguments
    ///
    /// * `name` - Query identifier.
    /// * `base_url` - Base URL of the source.
    /// * `media_types` - Content types this query produces.
    /// * `query_url` - URL template used to build the request.
    pub fn new(
        name: String,
        base_url: String,
        media_types: Vec<String>,
        query_url: String,
    ) -> Self {
        let http_client = HttpClient::new(&base_url);
        Self {
            name,
            base_url: base_url.clone(),
            base_url_template: base_url,
            media_types,
            query_url,
            request_method: ScraperRequestMethod::Get,
            request_body_pointer: None,
            request_body_select: HtmlScraperSelectMode::All,
            request_body_actions: Vec::new(),
            request_headers: Vec::new(),
            http_config: ScraperHttpConfig::default(),
            http_client,
            result_item_field: None,
            post_processes: Vec::new(),
        }
    }

    /// Validates every request-body action against the query-level contract.
    ///
    /// # Arguments
    ///
    /// * `context` - Context identifier used in diagnostic messages (e.g., "query").
    ///
    /// # Errors
    ///
    /// Returns an error if any action fails validation for the given context.
    pub fn validate_request_actions(&self, context: &str) -> anyhow::Result<()> {
        use crate::scrapyfy::query_helpers::validate_request_actions;

        validate_request_actions(context, &self.name, &self.request_body_actions)
    }

    /// Resolves request headers from the context row and template parameters.
    ///
    /// # Arguments
    ///
    /// * `context_row` - JSON value used for header pointer resolution.
    /// * `params` - Runtime template parameters.
    /// * `context_request_url` - URL of the parent request for action pipelines.
    pub fn resolve_request_headers(
        &self,
        context_row: &serde_json::Value,
        params: &HashMap<String, String>,
        context_request_url: &str,
    ) -> HashMap<String, String> {
        use crate::scrapyfy::query_helpers::resolve_request_headers;

        resolve_request_headers(
            &self.request_headers,
            context_row,
            params,
            context_request_url,
        )
    }

    /// Resolves the request body from the context row and template parameters.
    ///
    /// # Arguments
    ///
    /// * `context_row` - JSON value used for body pointer resolution.
    /// * `params` - Runtime template parameters.
    /// * `context_request_url` - URL of the parent request for action pipelines.
    pub fn resolve_request_body(
        &self,
        context_row: &serde_json::Value,
        params: &HashMap<String, String>,
        context_request_url: &str,
    ) -> Option<String> {
        use crate::scrapyfy::query_helpers::resolve_request_body;

        resolve_request_body(
            self.request_body_pointer.as_deref(),
            self.request_body_select,
            &self.request_body_actions,
            context_row,
            params,
            context_request_url,
        )
    }

    /// Returns whether the query matches at least one requested media type.
    pub fn is_media_type(&self, media_types: &[String]) -> bool {
        media_types
            .iter()
            .any(|media_type| self.media_types.contains(media_type))
    }

    /// Returns the query name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Returns the media types.
    pub fn media_types(&self) -> &[String] {
        &self.media_types
    }

    /// Returns the query URL.
    pub fn query_url(&self) -> &str {
        &self.query_url
    }

    /// Returns the request method.
    pub fn request_method(&self) -> ScraperRequestMethod {
        self.request_method
    }

    /// Returns the request body pointer.
    pub fn request_body_pointer(&self) -> Option<&str> {
        self.request_body_pointer.as_deref()
    }

    /// Returns the request body selection mode.
    pub fn request_body_select(&self) -> HtmlScraperSelectMode {
        self.request_body_select
    }

    /// Returns the request body actions.
    pub fn request_body_actions(&self) -> &[ScraperAction] {
        &self.request_body_actions
    }

    /// Returns the request headers.
    pub fn request_headers(&self) -> &[ScraperRequestHeader] {
        &self.request_headers
    }

    /// Returns the HTTP configuration.
    pub fn http_config(&self) -> &ScraperHttpConfig {
        &self.http_config
    }

    /// Returns the HTTP client.
    pub fn http_client(&self) -> &HttpClient {
        &self.http_client
    }

    /// Returns the result item field.
    pub fn result_item_field(&self) -> Option<&str> {
        self.result_item_field.as_deref()
    }

    /// Returns the post processes.
    pub fn post_processes(&self) -> &[ScraperPostProcess] {
        &self.post_processes
    }
}
