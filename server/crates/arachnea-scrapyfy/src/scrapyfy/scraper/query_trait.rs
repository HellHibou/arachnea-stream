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
use super::row_locator::RowLocator;
use super::sub_query_spec::SubQuerySpec;

/// Implémenté par toute query : racine, sub-query, sub-query d'entry,
/// en HTML, JSON ou Static.
///
/// Le moteur d'exécution unifié
/// (voir [`execute_query`](super::query_executor::execute_query))
/// consomme ce trait pour piloter aussi bien les queries racines que les
/// sub-queries récursives (sub-queries siblings, sub-queries d'entry).
pub trait ScraperQuery: Send + Sync {
    // --- Identification ---

    /// Returns the scraper type (html | json | static).
    fn scraper_type(&self) -> super::ScraperType;

    /// Returns the query name used in diagnostic messages.
    fn name(&self) -> &str;

    /// Returns the media types associated with the query results.
    fn media_types(&self) -> &[String];

    /// Returns whether the query matches at least one requested media type.
    fn is_media_type(&self, media_types: &[String]) -> bool {
        media_types
            .iter()
            .any(|media_type| self.media_types().contains(media_type))
    }

    // --- Requête HTTP (commun racine et sub_query) ---

    /// Returns the base URL exposed as `{base_url}` in templates.
    fn base_url(&self) -> &str;

    /// Returns the query URL template.
    fn query_url(&self) -> &str;

    /// Returns the HTTP method used to issue the request.
    fn request_method(&self) -> ScraperRequestMethod;

    /// Returns the optional pointer used to derive the request URL.
    fn request_pointer(&self) -> Option<&str>;

    /// Returns the selection mode for the request pointer.
    fn request_select(&self) -> HtmlScraperSelectMode {
        HtmlScraperSelectMode::All
    }

    /// Returns the request actions applied before the HTTP call.
    fn request_actions(&self) -> &[ScraperAction];

    /// Returns the request headers applied to the HTTP call.
    fn request_headers(&self) -> &[ScraperRequestHeader];

    /// Returns the HTTP configuration (mode, user agent, max redirects).
    fn http_config(&self) -> &ScraperHttpConfig;

    /// Returns a reference to the shared HTTP client configured for this query.
    fn http_client(&self) -> &HttpClient;

    /// Returns whether the response is parsed as `__NEXT_DATA__`.
    fn extract_next_data(&self) -> bool;

    // --- Row extraction ---

    /// Returns the row locator (CSS selector, JSON pointer, or single row).
    fn row_locator(&self) -> RowLocator;

    // --- Post-processing ---

    /// Returns the post-processing steps applied to each extracted row.
    ///
    /// Empty for query types that do not support post-processing at the
    /// query level (e.g. JSON sub-queries).
    fn post_processes(&self) -> &[ScraperPostProcess];

    /// Returns the optional group field whose items should be promoted to
    /// top-level entries.
    fn result_item_field(&self) -> Option<&str> {
        None
    }

    // --- Entries ---

    /// Returns the configured entries (field extractors).
    fn entries(&self) -> Vec<&dyn ScraperEntrySpec>;

    // --- Sub-queries siblings (récursion) ---

    /// Returns the sibling sub-queries attached to this query.
    fn sub_queries(&self) -> Vec<&dyn ScraperQuery>;

    // --- Spec sub-query (None pour racines) ---

    /// Returns the sub-query spec (or `None` for root queries).
    fn sub_query_spec(&self) -> Option<&SubQuerySpec> {
        None
    }

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
