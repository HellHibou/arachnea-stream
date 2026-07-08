//! [`ScraperEntrySpec`] — contract implemented by HTML and JSON entries.

use std::any::Any;

use crate::scrapyfy::actions::ScraperAction;
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;

/// Trait implemented by [`HtmlScraperEntry`](crate::scrapyfy::scraper_html::entry::HtmlScraperEntry) and
/// [`JsonScraperEntry`](crate::scrapyfy::scraper_json::entry::JsonScraperEntry).
///
/// Allows the execution engine to iterate over entries without knowing their concrete type.
///
/// The `as_any()` mechanism allows the unified executor to downcast to the concrete
/// entry type when applying entries to rows.
pub trait ScraperEntrySpec: Send + Sync {
    /// Returns a reference to the concrete type as `&dyn Any`.
    ///
    /// Used by the unified executor to downcast from `&dyn ScraperEntrySpec`
    /// to the concrete entry type (`HtmlScraperEntry` or `JsonScraperEntry`).
    fn as_any(&self) -> &dyn Any;

    /// Returns a mutable reference to the concrete type as `&mut dyn Any`.
    ///
    /// Used by the unified executor when mutable access to the concrete type is needed.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Returns the entry name.
    ///
    /// This is the identifier used in diagnostic messages and as the field name
    /// in the extracted data (unless overridden by other configurations).
    fn name(&self) -> &str;

    /// Returns the entry type discriminant.
    ///
    /// Returns [`ScraperType::Html`] for HTML entries or [`ScraperType::Json`] for JSON entries.
    fn entry_type(&self) -> super::ScraperType;

    /// Returns the JSON pointer (only set for JSON entries).
    ///
    /// Returns `None` for HTML entries.
    /// The pointer is a JSON Pointer (RFC 6901) used to extract values from JSON responses.
    fn pointer(&self) -> Option<&str>;

    /// Returns the CSS selector (only set for HTML entries).
    ///
    /// Returns `None` for JSON entries.
    /// The selector is used to extract values from HTML documents.
    fn selector(&self) -> Option<&str>;

    /// Returns the selection mode for pointer/selector.
    ///
    /// Determines whether to select the first match or all matches:
    /// - [`HtmlScraperSelectMode::First`]: Only the first matching element/value
    /// - [`HtmlScraperSelectMode::All`]: All matching elements/values
    fn select(&self) -> HtmlScraperSelectMode;

    /// Returns the ordered extraction actions.
    ///
    /// Actions are applied in sequence to transform and process the extracted values.
    /// Returns an empty slice if no actions are configured.
    fn actions(&self) -> &[ScraperAction];

    /// Returns the nested sub-entries (groups).
    ///
    /// Sub-entries allow grouping multiple extractions under a common parent field.
    /// Returns an empty vector if this entry has no sub-entries.
    fn sub_entries(&self) -> Vec<&dyn ScraperEntrySpec>;

    /// Returns `true` when this entry is a group entry.
    ///
    /// A group entry is one that contains sub-entries.
    fn is_group(&self) -> bool;

    /// Returns the sub-queries attached to this entry.
    ///
    /// Sub-queries are follow-up requests that are executed for each extracted value
    /// or for the entry context. Returns an empty vector if no sub-queries are attached.
    fn sub_queries(&self) -> Vec<&dyn super::ScraperQuery>;

    /// Returns the 1-based field index for text-mode entries, if configured.
    ///
    /// When `Some(n)`, the entry selects the `n`-th field (1-based) after splitting
    /// a text row by the query's `field_delimiter`. Returns `None` for entries that
    /// are not text-mode (HTML/JSON entries).
    fn field(&self) -> Option<usize> {
        None
    }
}
