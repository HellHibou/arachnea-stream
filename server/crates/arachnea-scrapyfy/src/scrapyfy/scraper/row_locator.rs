//! [`RowLocator`] — describes how rows are extracted from a scraper response.

use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;

/// Top-level scraper kind, used to dispatch the right response parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScraperType {
    /// HTML page parsed via CSS selectors.
    Html,
    /// JSON payload parsed via JSON pointers.
    Json,
    /// Static, YAML-declared rows with no HTTP request.
    Static,
}

/// Describes how rows are extracted from a scraper response.
///
/// The variant is chosen at runtime based on the parent query's
/// [`ScraperType`](super::ScraperType).
pub enum RowLocator {
    /// HTML response: apply a CSS selector on the parsed document.
    Selector {
        /// CSS selector used to enumerate rows.
        selector: String,
        /// Whether to pick the first match or all matches.
        select: HtmlScraperSelectMode,
    },
    /// JSON response: apply a JSON pointer on the parsed payload.
    Pointer(String),
    /// Single row (e.g. static query, or a payload that maps 1:1 to a row).
    Single,
}
