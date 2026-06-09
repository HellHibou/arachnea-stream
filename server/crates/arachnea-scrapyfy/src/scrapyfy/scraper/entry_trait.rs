//! [`ScraperEntrySpec`] — contract implemented by HTML and JSON entries.

use std::any::Any;

use crate::scrapyfy::actions::ScraperAction;
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;

/// Implémenté par [`HtmlScraperEntry`](crate::scrapyfy::scraper_html::entry::HtmlScraperEntry) et
/// [`JsonScraperEntry`](crate::scrapyfy::scraper_json::entry::JsonScraperEntry).
///
/// Permet au moteur d'exécution d'itérer sur les entries sans connaître
/// leur type concret.
///
/// Le mécanisme `as_any()` permet au moteur unifié de downcaster vers le
/// type concret lors de l'application des entries sur les rows.
pub trait ScraperEntrySpec: Send + Sync {
    /// Returns a reference to the concrete type as `&dyn Any`.
    ///
    /// Used by the unified executor to downcast from `&dyn ScraperEntrySpec`
    /// to the concrete entry type (`HtmlScraperEntry` or `JsonScraperEntry`).
    fn as_any(&self) -> &dyn Any;

    /// Returns a mutable reference to the concrete type as `&mut dyn Any`.
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Returns the entry name.
    fn name(&self) -> &str;

    /// Returns the entry type discriminant.
    fn entry_type(&self) -> super::ScraperType;

    /// Returns the JSON pointer (only set for JSON entries).
    fn pointer(&self) -> Option<&str>;

    /// Returns the CSS selector (only set for HTML entries).
    fn selector(&self) -> Option<&str>;

    /// Returns the selection mode for pointer/selector.
    fn select(&self) -> HtmlScraperSelectMode;

    /// Returns the ordered extraction actions.
    fn actions(&self) -> &[ScraperAction];

    /// Returns the nested sub-entries (groups).
    fn sub_entries(&self) -> Vec<&dyn ScraperEntrySpec>;

    /// Returns `true` when this entry is a group entry.
    fn is_group(&self) -> bool;

    /// Returns the sub-queries attached to this entry.
    fn sub_queries(&self) -> Vec<&dyn super::ScraperQuery>;
}
