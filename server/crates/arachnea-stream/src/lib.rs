#![warn(missing_docs)]
//! Arachnea stream service facade and player resolvers.

/// Persistent application configuration.
pub mod configuration;
/// Administration API (state, security, operations).
pub mod admin;
/// Global ETag construction and decoding for aggregated responses.
pub mod stream_etag;
pub use stream_etag::{build_global_etag, decode_client_fragments};

/// Stream service implementations and player resolvers.
pub mod services;
/// Generic YAML-driven stream resolver facade.
pub mod stream_resolver;
/// Stream scraper facade and main entry point.
pub mod stream_scraper;
/// Stable route facade with validated, atomic scraper reloads.
pub mod reloadable_stream_scraper;

pub use reloadable_stream_scraper::{
    ReloadableStreamScraper, StreamReloadReport, StreamReloadSkippedSource, StreamReloadSourceError,
};
pub use stream_scraper::{StreamScraper, StreamScraperBuildOptions};
