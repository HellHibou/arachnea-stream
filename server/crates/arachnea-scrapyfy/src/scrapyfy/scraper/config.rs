//! Common configuration types and defaults for all scraper query types.
//!
//! This module provides shared types and utilities used by both HTML and JSON
//! scraper configurations to reduce code duplication and ensure consistency.

use anyhow::Result;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;

use crate::scrapyfy::query_helpers::{self, QueryTemplateParamMapping};
use crate::scrapyfy::{ScraperAction, ScraperHttpConfig, ScraperPostProcess};
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;

// ---------------------------------------------------------------------------
// Default-value helpers
// ---------------------------------------------------------------------------

/// Defaults to [`HtmlScraperSelectMode::All`].
pub fn default_select_mode() -> HtmlScraperSelectMode {
    HtmlScraperSelectMode::All
}

/// Defaults to [`ScraperRequestMethod::Get`].
pub fn default_request_method() -> ScraperRequestMethod {
    ScraperRequestMethod::Get
}

// ---------------------------------------------------------------------------
// Shared HTTP request types
// ---------------------------------------------------------------------------

/// Supported HTTP methods for scraper requests.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScraperRequestMethod {
    /// GET request.
    Get,
    /// POST request.
    Post,
}

impl ScraperRequestMethod {
    /// Converts this enum variant into the corresponding [`http::Method`].
    pub fn as_http_method(self) -> http::Method {
        match self {
            Self::Get => http::Method::GET,
            Self::Post => http::Method::POST,
        }
    }
}

/// Raw configuration definition of one header added to a scraper request.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScraperRequestHeaderRaw {
    /// Header name.
    pub name: String,
    /// Optional JSON pointer selecting the header value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    /// Selection mode for the header pointer.
    #[serde(default = "default_select_mode")]
    pub select: HtmlScraperSelectMode,
    /// Actions applied to the header value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ScraperAction>,
}

/// Runtime definition of one header added to a scraper request.
#[derive(Clone, Debug)]
pub struct ScraperRequestHeader {
    /// Header name.
    pub name: String,
    /// Optional JSON pointer selecting the header value.
    pub pointer: Option<String>,
    /// Selection mode for the header pointer.
    pub select: HtmlScraperSelectMode,
    /// Actions applied to the header value.
    pub actions: Vec<ScraperAction>,
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
    /// Returns an error if any action fails validation for the header context.
    pub fn validate_actions(name: &str, actions: &[ScraperAction]) -> Result<()> {
        for action in actions {
            action.validate(name, "header")?;
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
    pub fn resolve(
        &self,
        row: &serde_json::Value,
        params: &HashMap<String, String>,
        request_url: &str,
    ) -> Option<(String, String)> {
        use crate::scrapyfy::scraper_json::entry::{json_value_to_strings, select_json_values};

        let mut values = Vec::new();

        for selected in select_json_values(row, self.pointer.as_deref(), self.select) {
            let mut selected_values = json_value_to_strings(selected);
            for action in &self.actions {
                selected_values =
                    action.apply(&None, selected_values, params, request_url, None, None);
            }
            values.extend(selected_values);
        }

        let resolved_values: Vec<String> = values
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();

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
            anyhow::bail!("Invalid scraper header: name cannot be empty");
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

// ---------------------------------------------------------------------------
// Traits communs
// ---------------------------------------------------------------------------

/// Trait commun pour toutes les configurations de requêtes (HTML, JSON).
pub trait ScraperQueryRaw {
    /// Retourne le nom de la requête.
    fn name(&self) -> String;

    /// Résout les placeholders de collection dans cette configuration de requête.
    ///
    /// # Arguments
    ///
    /// * `params` - Paramètres template au niveau de la collection.
    ///
    /// # Errors
    ///
    /// Retourne une erreur si un placeholder requis est manquant.
    fn resolve_collection_params(&mut self, params: &HashMap<String, String>) -> Result<()>;

    /// Fusionne la configuration HTTP de la collection dans cette requête.
    ///
    /// # Arguments
    ///
    /// * `collection_http` - Configuration HTTP héritée de la collection parente.
    fn apply_collection_http(&mut self, collection_http: &ScraperHttpConfig);
}

// ---------------------------------------------------------------------------
// Common sub-query configuration
// ---------------------------------------------------------------------------

/// Common configuration fields shared by all sub-query types (HTML, JSON).
///
/// Used in [`crate::scrapyfy::scraper_json::config::EntrySubQueryRaw`] variants
/// to reduce code duplication.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SubQueryCommon {
    /// Post-processing steps applied to each extracted row.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_process: Vec<ScraperPostProcess>,
    /// Optional pointer selecting the context rows that seed follow-up requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_pointer: Option<String>,
    /// Selection mode for the context pointer.
    #[serde(default = "default_select_mode")]
    pub context_select: HtmlScraperSelectMode,
    /// Filters applied to the context row before issuing the follow-up request.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub filters: HashMap<String, Vec<String>>,
    /// Filters applied to the fetched rows (after the HTTP response).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub row_filters: HashMap<String, Vec<String>>,
    /// Path where the sub-query result is nested (default: root level).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Pointer selecting the request URL from the context row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_pointer: Option<String>,
    /// Selection mode for the request pointer.
    #[serde(default = "default_select_mode")]
    pub request_select: HtmlScraperSelectMode,
    /// Actions applied to the selected request URLs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_actions: Vec<ScraperAction>,
    /// HTTP method used for the follow-up request.
    #[serde(default = "default_request_method")]
    pub request_method: ScraperRequestMethod,
    /// HTTP headers attached to the follow-up request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_headers: Vec<ScraperRequestHeaderRaw>,
    /// HTTP client configuration for the follow-up request.
    #[serde(default, skip_serializing_if = "ScraperHttpConfig::is_empty")]
    pub http: ScraperHttpConfig,
}

impl SubQueryCommon {
    /// Propagates the parent HTTP configuration into this sub-query's HTTP config.
    ///
    /// # Arguments
    ///
    /// * `parent_http` - HTTP configuration inherited from the parent query.
    pub fn apply_parent_http(&mut self, parent_http: &ScraperHttpConfig) {
        self.http = parent_http.merge(&self.http);
    }

    /// Resolves collection-level placeholders in this sub-query's HTTP config.
    ///
    /// # Arguments
    ///
    /// * `scraper_type` - Type of scraper for diagnostic messages.
    /// * `parent_name` - Parent query name used in diagnostic messages.
    /// * `params` - Collection-level template parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if a required placeholder is missing from `params`.
    pub fn resolve_collection_params(
        &mut self,
        scraper_type: &str,
        parent_name: &str,
        params: &HashMap<String, String>,
    ) -> Result<()> {
        self.http
            .resolve_collection_params(scraper_type, parent_name, params)
    }
}

// ---------------------------------------------------------------------------
// Common query configuration
// ---------------------------------------------------------------------------

/// Common configuration fields shared by all scraper query types (HTML, JSON).
///
/// This struct contains fields that are identical between HTML and JSON scrapers,
/// reducing code duplication and ensuring consistency.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScraperQueryCommon {
    /// Query identifier used as the lookup key in a collection.
    pub name: String,
    /// Base URL of the source, also available as `{base_url}` in templates.
    pub base_url: String,
    /// Resolved base URL after collection-level placeholder substitution.
    #[serde(skip)]
    pub resolved_base_url: Option<String>,
    /// Content types this query produces.
    pub media_types: Vec<String>,
    /// URL template used to build the request.
    pub query_url: String,
    /// HTTP method used to issue the request.
    #[serde(default = "default_request_method")]
    pub request_method: ScraperRequestMethod,
    /// Optional JSON pointer selecting the request body from runtime params.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_body_pointer: Option<String>,
    /// Selection mode for the request body pointer.
    #[serde(default = "default_select_mode")]
    pub request_body_select: HtmlScraperSelectMode,
    /// Actions applied to the request body before the HTTP call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_body_actions: Vec<ScraperAction>,
    /// HTTP headers attached to the request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_headers: Vec<ScraperRequestHeaderRaw>,
    /// HTTP client configuration (mode, user agent, max redirects).
    #[serde(default, skip_serializing_if = "ScraperHttpConfig::is_empty")]
    pub http: ScraperHttpConfig,
    /// Source-to-target parameter mappings applied before template resolution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_param_mappings: Vec<QueryTemplateParamMapping>,
    /// Optional group field whose items should become query rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_item_field: Option<String>,
    /// Post-processing steps applied to each extracted row.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_process: Vec<ScraperPostProcess>,
}

impl ScraperQueryCommon {
    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// Resolves collection-level placeholders used by this query config.
    ///
    /// # Arguments
    ///
    /// * `scraper_type` - Type of scraper ("HTML query", "JSON query", etc.) for diagnostics.
    /// * `params` - Collection-level template parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if a required placeholder is missing from `params`.
    pub fn resolve_collection_params(
        &mut self,
        scraper_type: &str,
        params: &HashMap<String, String>,
    ) -> Result<()> {
        self.resolved_base_url = Some(query_helpers::resolve_required_template(
            scraper_type,
            &self.name,
            "base_url",
            &self.base_url,
            params,
        )?);
        self.http
            .resolve_collection_params(scraper_type, &self.name, params)?;
        Ok(())
    }

    /// Merges the collection-level HTTP configuration into this query's own config.
    ///
    /// # Arguments
    ///
    /// * `collection_http` - HTTP configuration inherited from the parent collection.
    pub fn apply_collection_http(&mut self, collection_http: &ScraperHttpConfig) {
        self.http = collection_http.merge(&self.http);
    }
}
