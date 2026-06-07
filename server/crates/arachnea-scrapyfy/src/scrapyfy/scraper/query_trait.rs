//! [`ScraperQuery`] — uniform contract for root, sub-query, and entry-level
//! sub-queries across every scraper type.
//!
//! Step 5 ships the skeleton of the trait. Steps 6+ will add the accessors
//! needed by the unified executor (`request_method`, `request_headers`,
//! `http_config`, ...) and provide concrete implementations for
//! `JsonScraperQuery`, `JsonScraperSubQuery`, `HtmlScraperQuery`, and
//! `StaticScraperQuery`.

use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;

use super::entry_trait::ScraperEntrySpec;
use super::row_locator::RowLocator;
use super::sub_query_spec::SubQuerySpec;

/// Implémenté par toute query : racine, sub-query, sub-query d'entry,
/// en HTML, JSON ou Static.
pub trait ScraperQuery: Send + Sync {
    // --- Identification ---

    /// Returns the scraper type (html | json | static).
    fn scraper_type(&self) -> super::ScraperType;

    /// Returns the query name used in diagnostic messages.
    fn name(&self) -> &str;

    // --- Requête HTTP (commun racine et sub_query) ---

    /// Returns the base URL exposed as `{base_url}` in templates.
    fn base_url(&self) -> &str;

    /// Returns the query URL template.
    fn query_url(&self) -> &str;

    /// Returns the selection mode for the request pointer.
    fn request_select(&self) -> HtmlScraperSelectMode;

    /// Returns the request actions applied before the HTTP call.
    fn request_actions(&self) -> &[crate::scrapyfy::actions::ScraperAction];

    /// Returns the optional pointer used to derive the request URL.
    fn request_pointer(&self) -> Option<&str>;

    /// Returns whether the response is parsed as `__NEXT_DATA__`.
    fn extract_next_data(&self) -> bool;

    // --- Row extraction ---

    /// Returns the row locator (CSS selector, JSON pointer, or single row).
    fn row_locator(&self) -> RowLocator;

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
