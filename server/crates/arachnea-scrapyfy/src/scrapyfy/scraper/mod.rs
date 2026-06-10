//! Shared scraper primitives used by every query type (HTML, JSON, Static, ...).
//!
//! This module hosts the common types and the trait contracts that the
//! `scraper_html`, `scraper_json`, and `scraper_static` modules implement.
//! Keeping these definitions together ensures that any future query type
//! (GraphQL, RSS, XML, ...) only needs to implement the trait surface to
//! plug into the executor.
//!
//! See [`query_trait`] for the [`ScraperQuery`] contract, [`entry_trait`] for
//! the [`ScraperEntrySpec`] contract, [`row_locator`] for the [`RowLocator`]
//! enum, and [`sub_query_spec`] for the [`SubQuerySpec`] aggregate.

pub mod config;
pub mod entry_trait;
pub mod query_common;
pub mod row_locator;
pub mod sub_query_spec;

pub(crate) mod query_executor;
pub(crate) mod query_trait;

pub use config::{ScraperQueryCommon, ScraperRequestHeader, ScraperRequestHeaderRaw, ScraperRequestMethod, ScraperQueryRaw, SubQueryCommon};
pub use entry_trait::ScraperEntrySpec;
pub use query_common::BaseQueryConfig;
pub use query_trait::ScraperQuery;
pub use row_locator::{RowLocator, ScraperType};
pub use sub_query_spec::SubQuerySpec;
