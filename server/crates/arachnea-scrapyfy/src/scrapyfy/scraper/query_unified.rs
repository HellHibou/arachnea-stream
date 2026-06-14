//! [`ScraperQueryConfig`] — unified configuration shared by all query types.
//!
//! This module replaces the previous split between `SubQuerySpec`, `BaseQueryConfig`,
//! `ScraperQueryCommon`, `SubQueryCommon`, and the per-type query structs.
//!
//! A single [`ScraperQueryConfig`] holds every field that a root query, query-level
//! sub-query, or entry-level sub-query may need.  Fields that are only relevant for
//! sub-queries (e.g. `context_pointer`, `request_pointer`, `target`) are `Option<T>`.
//! Fields that are only relevant for root queries (e.g. `media_types`) are present
//! everywhere but may be empty on sub-queries.
//!
//! # YAML format
//!
//! Every query (root or sub) is tagged by `scraper_type: html | json | static`.
//! Root queries were already tagged in the YAML `queries:` list.  Sub-queries
//! are now also tagged, matching the [`EntrySubQueryRaw`] format that entry-level
//! sub-queries already use.
//!
//! ```yaml
//! queries:
//!   - name: get_season
//!     scraper_type: json
//!     base_url: "{base_url}"
//!     query_url: "{query_url}"
//!     row_pointer: "/"
//!     entries: [...]
//!     sub_queries:
//!       - scraper_type: json
//!         row_pointer: "/data"
//!         context_pointer: "/items/*"
//!         target: items
//!         request_pointer: "/url"
//!         entries: [...]
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::scrapyfy::actions::ScraperAction;
use crate::scrapyfy::post_processes::ScraperPostProcess;
use crate::scrapyfy::query_helpers::QueryTemplateParamMapping;
use crate::scrapyfy::scraper::config::{
    default_request_method, default_select_mode, ScraperRequestHeaderRaw, ScraperRequestMethod,
};
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;
use crate::scrapyfy::{HttpClient, ScraperHttpConfig};

// ---------------------------------------------------------------------------
// ScraperQueryConfig — unified configuration
// ---------------------------------------------------------------------------

/// Unified configuration for every query type (root, sub-query, entry sub-query).
///
/// All fields relevant to any query type are collected here.  Fields that are
/// only meaningful for sub-queries (request_pointer, context_pointer, etc.)
/// are `Option` and should be `None` on root queries.  Fields that are only
/// meaningful for a specific scraper type (e.g. `row_selector` for HTML) are
/// also `Option` and are validated at runtime.
///
/// The struct is annotated for YAML deserialization via the `scraper_type` tag
/// so that every YAML node in a `queries:` or `sub_queries:` list can be
/// deserialised directly into this type.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ScraperQueryConfig {
    // --- Identification ---
    /// Scraper type: `html`, `json`, or `static`.
    ///
    /// Determines which parser is used to process the HTTP response.
    /// This is the tag field in YAML.
    pub scraper_type: ScraperTypeConfig,

    /// Query identifier used as the lookup key in a collection.
    ///
    /// Only meaningful on root queries; may be empty on sub-queries.
    pub name: String,

    /// Content types this query produces.
    ///
    /// Only meaningful on root queries; may be empty on sub-queries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media_types: Vec<String>,

    // --- HTTP (root queries only, optional on sub-queries) ---
    /// Base URL of the source, available as `{base_url}` in templates.
    ///
    /// Optional: sub-queries that derive their URL from the parent via
    /// `request_pointer` do not need this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// Resolved base URL after collection-level parameter substitution.
    ///
    /// Computed at runtime from `base_url` and collection parameters.
    /// Not serialized.
    #[serde(skip)]
    pub resolved_base_url: Option<String>,

    /// URL template used to build the request.
    ///
    /// Optional: sub-queries that derive their URL from the parent via
    /// `request_pointer` do not need this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_url: Option<String>,

    /// HTTP method used to issue the request (GET or POST).
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

    /// When `true`, the response is parsed as Next.js `__NEXT_DATA__` payload.
    #[serde(default)]
    pub extract_next_data: bool,

    // --- Row extraction ---
    /// JSON pointer matching each result row (JSON scraper).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_pointer: Option<String>,

    /// CSS selector matching each result row (HTML scraper).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_selector: Option<String>,

    /// Optional group field whose items should become query rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_item_field: Option<String>,

    // --- Context (sub-queries only) ---
    /// JSON pointer in the parent row to scope execution to N contexts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_pointer: Option<String>,

    /// Selection mode for the context pointer.
    #[serde(default = "default_select_mode")]
    pub context_select: HtmlScraperSelectMode,

    // --- Request URL derivation (sub-queries only) ---
    /// JSON pointer to the entry value used as the request URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_pointer: Option<String>,

    /// Selection mode for the request pointer.
    #[serde(default = "default_select_mode")]
    pub request_select: HtmlScraperSelectMode,

    /// Actions applied to request URL values before the HTTP fetch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_actions: Vec<ScraperAction>,

    // --- Filtering ---
    /// Filter on the context row (before fetch).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub filters: HashMap<String, Vec<String>>,

    /// Filter on each fetched row (after fetch).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub row_filters: HashMap<String, Vec<String>>,

    // --- Merge behavior (sub-queries only) ---
    /// Path where the sub-query result is nested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,

    // --- Post-processing ---
    /// Post-processing steps applied to each extracted row.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_process: Vec<ScraperPostProcess>,

    // --- HTML-specific concurrency ---
    /// Maximum number of rows whose async post-process steps may run together.
    #[serde(default)]
    pub row_concurrency: usize,

    // --- JSON-specific concurrency ---
    /// Maximum number of sibling sub-queries executed concurrently.
    #[serde(default)]
    pub sibling_sub_query_concurrency: usize,

    /// Maximum number of sub-query contexts executed concurrently.
    #[serde(default)]
    pub sub_query_context_concurrency: usize,

    /// Maximum number of follow-up HTTP requests executed concurrently.
    #[serde(default)]
    pub sub_query_fetch_concurrency: usize,

    /// Root-level filters applied to the entire JSON response.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub json_filters: HashMap<String, Vec<String>>,
}

// ---------------------------------------------------------------------------
// ScraperTypeConfig — YAML-tagged scraper type
// ---------------------------------------------------------------------------

/// Scraper type tag used in YAML configuration.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScraperTypeConfig {
    /// HTML scraper — uses CSS selectors to extract data.
    Html,
    /// JSON scraper — uses JSON pointers to extract data.
    #[default]
    Json,
    /// Static scraper — returns YAML-declared data without an HTTP request.
    Static,
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

impl ScraperQueryConfig {
    /// Returns the resolved base URL (taking `resolved_base_url` or `base_url`).
    pub fn resolved_base_url(&self) -> &str {
        self.resolved_base_url
            .as_deref()
            .or(self.base_url.as_deref())
            .unwrap_or("")
    }

    /// Returns the query URL or empty string if not set.
    pub fn query_url_or_empty(&self) -> &str {
        self.query_url.as_deref().unwrap_or("")
    }

    /// Returns true when this config represents a sub-query (has context or request pointer).
    pub fn is_sub_query(&self) -> bool {
        self.context_pointer.is_some() || self.request_pointer.is_some() || self.target.is_some()
    }

    /// Returns true when this config represents a root query (has base_url and query_url).
    pub fn is_root_query(&self) -> bool {
        self.base_url.is_some() || self.query_url.is_some()
    }

    /// Resolves collection-level placeholders in this config.
    pub fn resolve_collection_params(
        &mut self,
        scraper_type_label: &str,
        parent_name: &str,
        params: &HashMap<String, String>,
    ) -> anyhow::Result<()> {
        use crate::scrapyfy::query_helpers;

        if let Some(base_url) = &self.base_url {
            self.resolved_base_url = Some(query_helpers::resolve_required_template(
                scraper_type_label,
                parent_name,
                "base_url",
                base_url,
                params,
            )?);
        }
        self.http
            .resolve_collection_params(scraper_type_label, parent_name, params)?;
        Ok(())
    }

    /// Merges the collection-level HTTP configuration into this config.
    pub fn apply_collection_http(&mut self, collection_http: &ScraperHttpConfig) {
        self.http = collection_http.merge(&self.http);
    }

    /// Propagates the parent HTTP configuration into this config (for sub-queries).
    pub fn apply_parent_http(&mut self, parent_http: &ScraperHttpConfig) {
        self.http = parent_http.merge(&self.http);
    }
}
