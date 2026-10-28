//! Shared scraper primitives used by every query type (HTML, JSON, Static, ...).
//!
//! This module hosts the common types and the trait contracts that the
//! `scraper_html`, `scraper_json`, and `scraper_static` modules implement.
//! Keeping these definitions together ensures that any future query type
//! (GraphQL, RSS, XML, ...) only needs to implement the trait surface to
//! plug into the executor.
//!
//! # Key Components
//!
//! - `query_trait`: The [`crate::scrapyfy::scraper::ScraperQuery`] contract that all query types must implement.
//! - [`crate::scrapyfy::scraper::entry_trait`]: The [`crate::scrapyfy::scraper::ScraperEntrySpec`] contract for field extractors.
//! - [`crate::scrapyfy::scraper::row_locator`]: The [`crate::scrapyfy::scraper::RowLocator`] enum defining how rows are extracted from responses.
//! - [`crate::scrapyfy::scraper::config`]: Common configuration types and defaults for all scraper query types.
//!
//! # Architecture
//!
//! The unified execution engine in `query_executor` uses these traits and types
//! to drive both root queries and recursive sub-queries through a common interface.

pub mod config;
pub mod entry_trait;
pub mod post_build;
pub mod query_unified;
pub mod row_locator;

pub(crate) mod query_executor;
pub(crate) mod query_trait;

// Re-export key types for convenience
pub use entry_trait::ScraperEntrySpec;
pub use query_trait::ScraperQuery;
pub use row_locator::{RowLocator, ScraperType};
