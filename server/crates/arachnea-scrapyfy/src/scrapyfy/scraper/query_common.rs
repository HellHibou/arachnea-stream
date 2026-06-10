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
    ///
    /// This is the name assigned in the YAML configuration.
    pub name: String,

    /// Resolved base URL of the source.
    ///
    /// This is the base URL after template placeholders have been resolved.
    pub base_url: String,

    /// Original base URL template preserved from YAML.
    ///
    /// This is the raw template string before placeholder resolution.
    pub base_url_template: String,

    /// Content types this query produces.
    ///
    /// Used for filtering queries based on requested media types.
    pub media_types: Vec<String>,

    /// URL template used to build the request.
    ///
    /// This template is formatted with runtime parameters to build the actual request URL.
    pub query_url: String,

    /// HTTP method used to issue the request.
    ///
    /// Either GET or POST.
    pub request_method: ScraperRequestMethod,

    /// Optional JSON pointer selecting the request body from runtime params.
    ///
    /// When set, the request body is extracted from the specified JSON pointer
    /// in the runtime parameters.
    pub request_body_pointer: Option<String>,

    /// Selection mode for the request body pointer.
    ///
    /// Determines whether to use the first match or all matches:
    /// - [`HtmlScraperSelectMode::First`]: Only the first matching value.
    /// - [`HtmlScraperSelectMode::All`]: All matching values.
    pub request_body_select: HtmlScraperSelectMode,

    /// Actions applied to the request body before the HTTP call.
    ///
    /// These actions transform the request body before sending it.
    pub request_body_actions: Vec<ScraperAction>,

    /// HTTP headers attached to the request.
    ///
    /// These headers are added to every HTTP request made by this query.
    pub request_headers: Vec<ScraperRequestHeader>,

    /// HTTP client configuration (mode, user agent, max redirects).
    ///
    /// Contains settings like client mode, user agent, max redirects, etc.
    pub http_config: ScraperHttpConfig,

    /// HTTP client used to issue requests.
    ///
    /// The client is pre-configured with the query's HTTP settings.
    pub http_client: HttpClient,

    /// Optional group field whose items should become query rows.
    ///
    /// When set, items from the specified field are hoisted to become individual rows.
    pub result_item_field: Option<String>,

    /// Post-processing steps applied to each extracted row.
    ///
    /// These steps transform and process the extracted data after extraction.
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
    ///
    /// # Returns
    ///
    /// A new `BaseQueryConfig` with all fields initialized to sensible defaults.
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
    /// Validation ensures that actions are appropriate for use in request body processing.
    pub fn validate_request_actions(&self, context: &str) -> anyhow::Result<()> {
        use crate::scrapyfy::query_helpers::validate_request_actions;

        validate_request_actions(context, &self.name, &self.request_body_actions)
    }

    /// Resolves request headers from the context row and template parameters.
    ///
    /// Applies header pointer resolution and action pipelines to each header
    /// to produce the final header values for the HTTP request.
    ///
    /// # Arguments
    ///
    /// * `context_row` - JSON value used for header pointer resolution.
    /// * `params` - Runtime template parameters.
    /// * `context_request_url` - URL of the parent request for action pipelines.
    ///
    /// # Returns
    ///
    /// A hash map of header names to their resolved values.
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
    /// Applies the body pointer and action pipelines to extract and transform
    /// the request body value.
    ///
    /// # Arguments
    ///
    /// * `context_row` - JSON value used for body pointer resolution.
    /// * `params` - Runtime template parameters.
    /// * `context_request_url` - URL of the parent request for action pipelines.
    ///
    /// # Returns
    ///
    /// The resolved request body as a string, or `None` if no body is configured.
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
    ///
    /// # Arguments
    ///
    /// * `media_types` - List of media types to check against.
    ///
    /// # Returns
    ///
    /// `true` if the query produces at least one of the requested media types.
    pub fn is_media_type(&self, media_types: &[String]) -> bool {
        media_types
            .iter()
            .any(|media_type| self.media_types.contains(media_type))
    }

    /// Returns the query name.
    ///
    /// This is the identifier assigned in the YAML configuration.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the base URL.
    ///
    /// This is the root URL of the source being scraped.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Returns the media types.
    ///
    /// These are the content types this query produces.
    pub fn media_types(&self) -> &[String] {
        &self.media_types
    }

    /// Returns the query URL.
    ///
    /// This is the URL template used to build the request.
    pub fn query_url(&self) -> &str {
        &self.query_url
    }

    /// Returns the request method.
    ///
    /// Either GET or POST.
    pub fn request_method(&self) -> ScraperRequestMethod {
        self.request_method
    }

    /// Returns the request body pointer.
    ///
    /// The JSON pointer used to extract the request body from runtime parameters.
    pub fn request_body_pointer(&self) -> Option<&str> {
        self.request_body_pointer.as_deref()
    }

    /// Returns the request body selection mode.
    ///
    /// Determines whether to use the first match or all matches from the body pointer.
    pub fn request_body_select(&self) -> HtmlScraperSelectMode {
        self.request_body_select
    }

    /// Returns the request body actions.
    ///
    /// Actions applied to transform the request body before sending.
    pub fn request_body_actions(&self) -> &[ScraperAction] {
        &self.request_body_actions
    }

    /// Returns the request headers.
    ///
    /// Headers attached to every HTTP request made by this query.
    pub fn request_headers(&self) -> &[ScraperRequestHeader] {
        &self.request_headers
    }

    /// Returns the HTTP configuration.
    ///
    /// Contains settings like client mode, user agent, max redirects, etc.
    pub fn http_config(&self) -> &ScraperHttpConfig {
        &self.http_config
    }

    /// Returns the HTTP client.
    ///
    /// The client is pre-configured with the query's HTTP settings.
    pub fn http_client(&self) -> &HttpClient {
        &self.http_client
    }

    /// Returns the result item field.
    ///
    /// The optional group field whose items should become query rows.
    pub fn result_item_field(&self) -> Option<&str> {
        self.result_item_field.as_deref()
    }

    /// Returns the post processes.
    ///
    /// Post-processing steps applied to each extracted row.
    pub fn post_processes(&self) -> &[ScraperPostProcess] {
        &self.post_processes
    }
}
