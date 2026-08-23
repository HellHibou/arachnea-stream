#![warn(missing_docs)]
//! Arachnea stream service facade and player resolvers.

/// Global ETag construction and decoding for aggregated responses.
pub mod stream_etag;
pub use stream_etag::{build_global_etag, decode_client_fragments, services_hash};

/// Stream service implementations and player resolvers.
pub mod services;
/// Generic YAML-driven stream resolver facade.
pub mod stream_resolver;
/// Stream scraper facade and main entry point.
pub mod stream_scraper;

pub use stream_scraper::StreamScraper;
