//! [`ScraperQuery`] — uniform contract for root, sub-query, and entry-level
//! sub-queries across every scraper type.
//!
//! Implemented by every query type (HTML, JSON, Static) and exposed to the
//! unified [`execute_query`](super::query_executor::execute_query) executor
//! for polymorphic dispatch.

use crate::scrapyfy::actions::ScraperAction;
use crate::scrapyfy::post_processes::ScraperPostProcess;
use crate::scrapyfy::scraper::config::{ScraperRequestHeader, ScraperRequestMethod};
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;
use crate::scrapyfy::{HttpClient, ScraperHttpConfig};

use std::any::Any;

use super::entry_trait::ScraperEntrySpec;
use super::query_unified::ScraperQueryConfig;
use super::row_locator::RowLocator;

/// Implemented by all queries: root, sub-query, and entry-level sub-query,
/// in HTML, JSON, or Static form.
///
/// The unified execution engine
/// (see [`execute_query`](super::query_executor::execute_query))
/// consumes this trait to drive both root queries and recursive sub-queries
/// (sibling sub-queries and entry-level sub-queries).
///
/// # Migration to unified config
///
/// The [`config()`] method returns a unified [`ScraperQueryConfig`] that
/// will eventually replace the individual legacy getters.
pub trait ScraperQuery: Send + Sync {
    // --- Unified config (migration target) ---

    /// Returns the unified query configuration.
    ///
    /// Default returns `None` — concrete types should override this when
    /// they have been migrated to use [`ScraperQueryConfig`].
    fn config(&self) -> Option<&ScraperQueryConfig> {
        None
    }
    // --- Identification ---

    /// Returns the scraper type (html | json | static).
    ///
    /// This determines which parser is used to process the response.
    fn scraper_type(&self) -> super::ScraperType;

    /// Returns the query name used in diagnostic messages.
    ///
    /// This is the identifier assigned in the YAML configuration.
    fn name(&self) -> &str;

    /// Returns the media types associated with the query results.
    ///
    /// Used for filtering queries based on requested content types.
    fn media_types(&self) -> &[String];

    /// Returns whether the query matches at least one requested media type.
    ///
    /// # Arguments
    ///
    /// * `media_types` - List of media types to check against.
    ///
    /// # Returns
    ///
    /// `true` if the query produces at least one of the requested media types.
    fn is_media_type(&self, media_types: &[String]) -> bool {
        media_types
            .iter()
            .any(|media_type| self.media_types().contains(media_type))
    }

    // --- HTTP Request (common to root and sub_query) ---

    /// Returns the base URL exposed as `{base_url}` in templates.
    ///
    /// This is the root URL of the source being scraped.
    fn base_url(&self) -> &str;

    /// Returns the query URL template.
    ///
    /// This template is formatted with runtime parameters to build the actual request URL.
    fn query_url(&self) -> &str;

    /// Returns the HTTP method used to issue the request.
    ///
    /// Either GET or POST.
    fn request_method(&self) -> ScraperRequestMethod;

    /// Returns the optional pointer used to derive the request URL.
    ///
    /// For sub-queries, this extracts the URL from the parent row.
    /// Returns `None` for root queries.
    fn request_pointer(&self) -> Option<&str>;

    /// Returns the selection mode for the request pointer.
    ///
    /// Determines whether to use the first match or all matches.
    /// Defaults to [`HtmlScraperSelectMode::All`].
    fn request_select(&self) -> HtmlScraperSelectMode {
        HtmlScraperSelectMode::All
    }

    /// Returns the request actions applied before the HTTP call.
    ///
    /// These actions transform the request URL or body before making the request.
    fn request_actions(&self) -> &[ScraperAction];

    /// Returns the request headers applied to the HTTP call.
    ///
    /// These headers are added to the HTTP request.
    fn request_headers(&self) -> &[ScraperRequestHeader];

    /// Returns the HTTP configuration (mode, user agent, max redirects).
    ///
    /// Contains settings like client mode, user agent, max redirects, etc.
    fn http_config(&self) -> &ScraperHttpConfig;

    /// Returns a reference to the shared HTTP client configured for this query.
    ///
    /// The client is pre-configured with the query's HTTP settings.
    fn http_client(&self) -> &HttpClient;

    /// Returns whether the response is parsed as `__NEXT_DATA__`.
    ///
    /// Used for Next.js applications that embed data in `__NEXT_DATA__` scripts.
    fn extract_next_data(&self) -> bool;

    // --- Row extraction ---

    /// Returns the row locator (CSS selector, JSON pointer, or single row).
    ///
    /// Defines how rows are extracted from the scraper response.
    fn row_locator(&self) -> RowLocator;

    // --- Post-processing ---

    /// Returns the post-processing steps applied to each extracted row.
    ///
    /// Empty for query types that do not support post-processing at the
    /// query level (e.g. JSON sub-queries).
    fn post_processes(&self) -> &[ScraperPostProcess];

    /// Returns the optional group field whose items should be promoted to
    /// top-level entries.
    ///
    /// When set, items from the specified field are hoisted to the top level
    /// instead of being nested under the field name.
    /// Defaults to `None`.
    fn result_item_field(&self) -> Option<&str> {
        None
    }

    // --- Entries ---

    /// Returns the configured entries (field extractors).
    ///
    /// Entries define how data is extracted from each row.
    fn entries(&self) -> Vec<&dyn ScraperEntrySpec>;

    // --- Sibling sub-queries (recursion) ---

    /// Returns the sibling sub-queries attached to this query.
    ///
    /// Sibling sub-queries are executed alongside the parent query and their
    /// results are merged at the same level.
    fn sub_queries(&self) -> Vec<&dyn ScraperQuery>;

    // --- Query-level sub-query detection (legacy) ---

    /// Returns the optional context pointer used by query-level sub-queries
    /// to scope execution to N contexts inside the parent row.
    ///
    /// Default returns `None` (entry-level sub-queries and root queries).
    /// Query-level sub-queries (`JsonScraperSubQuery` / `HtmlScraperSubQuery`
    /// in the legacy query-level API) override this to expose their
    /// `context_pointer` field so the unified executor can dispatch on the
    /// legacy semantics (iterating over context rows of the parent row).
    fn context_pointer(&self) -> Option<&str> {
        None
    }

    /// Returns the context selection mode for query-level sub-queries.
    ///
    /// Default returns [`HtmlScraperSelectMode::All`] (the safe default
    /// for callers that ignore the value when `context_pointer()` is `None`).
    fn context_select(&self) -> HtmlScraperSelectMode {
        HtmlScraperSelectMode::All
    }

    /// Returns the context entries (entries applied to the context row
    /// before issuing the follow-up request).
    ///
    /// Default returns an empty list. Query-level sub-queries override this
    /// to expose their `context_entries` field.
    fn context_entries(&self) -> Vec<&dyn ScraperEntrySpec> {
        Vec::new()
    }

    /// Returns the target path where query-level sub-query results are
    /// nested. `None` means the results are merged at the top level.
    ///
    /// Default returns `None` (results are merged at the top level).
    fn target(&self) -> Option<&str> {
        None
    }

    /// Returns a reference to the concrete type as `&dyn Any`.
    ///
    /// The unified executor uses this to downcast back to the concrete
    /// query type when a query-level sub-query needs to be executed via
    /// its legacy `execute` path (which keeps the existing semantics
    /// around `context_pointer` / `context_entries` / `row_filters`).
    fn as_any(&self) -> &dyn Any;
}