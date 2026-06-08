//! [`ScraperEntrySpec`] — contract implemented by HTML and JSON entries.

use crate::scrapyfy::actions::ScraperAction;
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;

/// Implémenté par [`HtmlScraperEntry`](crate::scrapyfy::scraper_html::entry::HtmlScraperEntry) et
/// [`JsonScraperEntry`](crate::scrapyfy::scraper_json::entry::JsonScraperEntry).
///
/// Permet au moteur d'exécution d'itérer sur les entries sans connaître
/// leur type concret.
pub trait ScraperEntrySpec: Send + Sync {
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
