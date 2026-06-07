//! HTML query definitions and executors.
//!
//! An HTML query fetches an HTML page from a remote endpoint (or a sibling
//! sub-query context) and extracts typed rows via CSS selectors.
//!
//! See [`query`] for the runtime and raw configuration types of
//! [`HtmlScraperQuery`], and [`entry`] for the runtime and raw configuration
//! types of [`HtmlScraperEntry`].

pub mod entry;
pub mod query;

pub use entry::{HtmlScraperEntry, HtmlScraperEntryRaw, HtmlScraperSelectMode};
pub use query::{HtmlScraperQuery, HtmlScraperQueryRaw};
