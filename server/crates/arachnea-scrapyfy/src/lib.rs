#![warn(missing_docs)]
//! Generic scraping engine used by the Arachnea stream backend.

/// Administration service: routes `admin/*`, sessions, authentication, and
/// multi-service-store source management.
pub mod admin;

/// Scraper query models, executors, and post-processors.
pub mod scrapyfy;

pub use scrapyfy::*;
