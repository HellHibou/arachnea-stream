#![warn(missing_docs)]
//! Arachnea stream service facade and player resolvers.

/// Stream service implementations and player resolvers.
pub mod services;
/// Stream scraper facade and main entry point.
pub mod stream_scraper;
/// Generic YAML-driven stream resolver facade.
pub mod stream_resolver;

pub use stream_scraper::StreamScraper;
