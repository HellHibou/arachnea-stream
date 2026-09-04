#![warn(missing_docs)]
//! Arachnea stream service facade and player resolvers.

/// Composition adapter between the generic Scrapyfy administration service and
/// this executable's configuration, credentials and stream runtime.
pub mod admin_composition;
/// Global ETag construction and decoding for aggregated responses.
pub mod stream_etag;
pub use stream_etag::{build_global_etag, decode_client_fragments};

/// Stable route facade with validated, atomic scraper reloads.
pub mod reloadable_stream_scraper;
/// Stream service implementations and player resolvers.
pub mod services;
/// Generic YAML-driven stream resolver facade.
pub mod stream_resolver;
/// Stream scraper facade and main entry point.
pub mod stream_scraper;
/// Credentials adapter over the typed `arachnea-services` store.
pub mod typed_service_credentials_store;

pub use reloadable_stream_scraper::ReloadableStreamScraper;
pub use stream_scraper::{StreamScraper, StreamScraperBuildOptions};
pub use typed_service_credentials_store::TypedServiceCredentialsStore;
