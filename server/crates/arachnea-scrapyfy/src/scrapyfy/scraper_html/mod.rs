//! HTML query definitions and executors.
//!
//! An HTML query fetches an HTML page from a remote endpoint (or a sibling
//! sub-query context) and extracts typed rows via CSS selectors.
//!
//! This module provides the complete implementation for HTML scraping:
//!
//! - [`config`] — Raw YAML configuration types and validated runtime conversions
//!   for HTML queries and sub-queries.
//! - [`entry`] — Runtime and raw configuration types of [`HtmlScraperEntry`],
//!   including field and group entries with CSS selector support.
//! - [`query`] — Runtime query type and [`ScraperQuery`] trait implementation
//!   for executing HTML scraping operations.

pub mod config;
pub mod entry;
pub mod query;

/// Re-exported raw HTML query configuration type.
pub use config::HtmlScraperQueryRaw;


/// Re-exported runtime HTML scraper entry type.
pub use entry::HtmlScraperEntry;

/// Re-exported raw HTML scraper entry configuration type.
pub use entry::HtmlScraperEntryRaw;

/// Re-exported selection mode enum for HTML scraper entries.
pub use entry::HtmlScraperSelectMode;

/// Re-exported runtime HTML scraper query type.
pub use query::HtmlScraperQuery;