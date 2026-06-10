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
//! - [`query_trait`]: The [`ScraperQuery`] contract that all query types must implement.
//! - [`entry_trait`]: The [`ScraperEntrySpec`] contract for field extractors.
//! - [`row_locator`]: The [`RowLocator`] enum defining how rows are extracted from responses.
//! - [`sub_query_spec`]: The [`SubQuerySpec`] aggregate containing sub-query configuration.
//! - [`config`]: Common configuration types and defaults for all scraper query types.
//! - [`query_common`]: Base configuration structures shared by HTML and JSON scrapers.
//!
//! # Architecture
//!
//! The unified execution engine in [`query_executor`] uses these traits and types
//! to drive both root queries and recursive sub-queries through a common interface.

pub mod config;
pub mod entry_trait;
pub mod query_common;
pub mod row_locator;
pub mod sub_query_spec;

pub(crate) mod query_executor;
pub(crate) mod query_trait;

// Re-export key types for convenience
pub use config::{
    ScraperQueryCommon, ScraperQueryRaw, ScraperRequestHeader, ScraperRequestHeaderRaw,
    ScraperRequestMethod, SubQueryCommon,
};
pub use entry_trait::ScraperEntrySpec;
pub use query_common::BaseQueryConfig;
pub use query_trait::ScraperQuery;
pub use row_locator::{RowLocator, ScraperType};
pub use sub_query_spec::SubQuerySpec;
