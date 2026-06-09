use std::collections::HashMap;

/// Tree-shaped output node model used by all scraper pipelines.
pub mod scraper_data_node;

pub use scraper_data_node::ScraperDataNode;

/// HTTP client helpers used by query executors and tests.
pub mod http_client;
pub use http_client::HttpClient;
pub use http_client::{ScraperHttpConfig, ScraperHttpMode, ScraperHttpUserAgentProfile};

/// Text extraction and normalization actions.
pub mod actions;
pub use actions::{GetDateSource, GetDateSources, ScraperAction};

/// Post-process transformations applied after raw extraction.
pub mod post_processes;
pub use post_processes::{ScraperPostProcess, ScraperPostProcessContext};

/// HTML query definitions, executors, and field extractors.
pub mod scraper_html;
pub use scraper_html::entry::{HtmlScraperEntry, HtmlScraperEntryRaw, HtmlScraperSelectMode};
pub use scraper_html::query::HtmlScraperQuery;
pub use scraper_html::config::HtmlScraperQueryRaw;

/// JSON query definitions, executors, and field extractors.
pub mod scraper_json;
pub use scraper_json::entry::{JsonScraperEntry, JsonScraperEntryRaw};
pub use scraper_json::query::{JsonScraperQuery, JsonScraperQueryRaw};

/// Static query definitions and executors.
pub mod scraper_static;
pub use scraper_static::query::{StaticScraperEntryRaw, StaticScraperQuery, StaticScraperQueryRaw};

/// Shared traits, spec types, and execution engine for scraper queries.
pub(crate) mod scraper;

/// Shared helpers for query URL formatting and filtering.
pub mod query_helpers;

/// Query collection models and file loading helpers.
pub mod scraper_query_collection;
pub use scraper_query_collection::{
    ScraperQueryCollection, ScraperQueryCollectionParameter, ScraperQueryCollectionRaw,
};

/// Multi-source query aggregator.
pub mod scraper_agregator;
pub use scraper_agregator::{ScraperAgregator, ScraperSourceParams};

/// Scraper manager trait used by runtime and test harnesses.
pub mod scraper_manager;
pub use scraper_manager::ScraperManager;
