//! HTML query definitions and executors.
//!
//! An HTML query fetches an HTML page from a remote endpoint (or a sibling
//! sub-query context) and extracts typed rows via CSS selectors.
//!
//! - [`config`] — Raw YAML configuration types and validated runtime conversions.
//! - [`entry`] — Runtime and raw configuration types of [`HtmlScraperEntry`].
//! - [`query`] — Runtime query type and [`ScraperManagerQuery`] implementation.

pub mod config;
pub mod entry;
pub mod query;

pub use entry::{HtmlScraperEntry, HtmlScraperEntryRaw, HtmlScraperSelectMode};
pub use config::HtmlScraperQueryRaw;
pub use query::HtmlScraperQuery;