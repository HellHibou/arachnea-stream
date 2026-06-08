//! [`ScraperQuery`] — uniform contract for root, sub-query, and entry-level
//! sub-queries across every scraper type.
//!
//! Implemented by every query type (HTML, JSON, Static) and exposed to the
//! unified [`execute_query`](super::query_executor::execute_query) executor
//! for polymorphic dispatch.

use crate::scrapyfy::actions::ScraperAction;
use crate::scrapyfy::post_processes::ScraperPostProcess;
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;
use crate::scrapyfy::scraper_json::query::{ScraperRequestHeader, ScraperRequestMethod};
use crate::scrapyfy::ScraperHttpConfig;

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
    fn is_media_type(&self, media_types: &[String]) -> bool;

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
    fn request_select(&self) -> HtmlScraperSelectMode;

    /// Returns the request actions applied before the HTTP call.
    fn request_actions(&self) -> &[ScraperAction];

    /// Returns the request headers applied to the HTTP call.
    fn request_headers(&self) -> &[ScraperRequestHeader];

    /// Returns the HTTP configuration (mode, user agent, max redirects).
    fn http_config(&self) -> &ScraperHttpConfig;

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
    fn result_item_field(&self) -> Option<&str>;

    // --- Entries ---

    /// Returns the configured entries (field extractors).
    fn entries(&self) -> Vec<&dyn ScraperEntrySpec>;

    // --- Sub-queries siblings (récursion) ---

    /// Returns the sibling sub-queries attached to this query.
    fn sub_queries(&self) -> Vec<&dyn ScraperQuery>;

    // --- Spec sub-query (None pour racines) ---

    /// Returns the sub-query spec (or `None` for root queries).
    fn sub_query_spec(&self) -> Option<&SubQuerySpec>;
}
