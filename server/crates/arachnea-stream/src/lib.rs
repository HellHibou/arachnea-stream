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

#[cfg(test)]
#[path = "../tests/support/application.rs"]
mod test_support;

// Crate-owned application and generic facade contracts.

// Application

/// Fallback credentials encryption key; changing these bytes prevents decrypting data written with this key.
pub const DEFAULT_SERVER_CREDENTIALS_KEY: [u8; 32] = *b"hell_hibou-arachnea-key-20260422";

/// Relative configuration path in the writable application data directory.
pub const CONFIGURATION_PATH: &str = "data/config.json";

/// Default application loopback proxy port; zero requests an ephemeral port.
pub const DEFAULT_LOOPBACK_PROXY_PORT: u16 = 0;

// Stream facade and generic resolver

/// Default group name used by the stream scraper crate.
pub const STREAM_SERVICE_GROUP_NAME: &str = "arachnea-stream";

/// Persistence namespace containing administrator service activation overrides.
pub const STREAM_SERVICES_STORE_NAME: &str = "arachnea-services";

/// Root directory, relative to the application data directory, of the SQLite
/// persistence stores.
pub const PERSISTENCE_DATA_ROOT: &str = "data";

/// Default path used by the services
pub const DEFAULT_SERVICES_CONFIG_PATH: &str = const_format::concatcp!(
    arachnea_scrapyfy::DEFAULT_SERVICES_DIRECTORY,
    "/",
    STREAM_SERVICE_GROUP_NAME,
    "/services.json"
);

/// Stream http proxy command identifier used by routing, persistence or provider requests.
pub(crate) const STREAM_HTTP_PROXY_COMMAND: &str = "proxy";

/// Stream drm license proxy command identifier used by routing, persistence or provider requests.
pub(crate) const STREAM_DRM_LICENSE_PROXY_COMMAND: &str = "get_drm_license";

/// Stream resolver max embed html bytes byte limit.
pub(crate) const STREAM_RESOLVER_MAX_EMBED_HTML_BYTES: usize = 1_048_576;

/// Group name used by the stream hoster configuration.
pub const STREAM_RESOLVER_GROUP_NAME: &str = "arachnea-stream-hoster";

/// Default path used by the stream resolver services configuration.
pub const STREAM_RESOLVER_CONFIG_PATH: &str = const_format::concatcp!(
    arachnea_scrapyfy::DEFAULT_SERVICES_DIRECTORY,
    "/",
    STREAM_RESOLVER_GROUP_NAME,
    "/services.json"
);

/// Name of the `resolve_stream` YAML query.
pub(crate) const STREAM_RESOLVER_RESOLVE_STREAM_QUERY_NAME: &str = "resolve_stream";

/// Name of the optional `can_resolve_url` YAML query.
pub(crate) const STREAM_RESOLVER_CAN_RESOLVE_URL_QUERY_NAME: &str = "can_resolve_url";

/// Name of the optional `can_resolve_html` YAML query.
pub(crate) const STREAM_RESOLVER_CAN_RESOLVE_HTML_QUERY_NAME: &str = "can_resolve_html";

/// Resolver identifier used by the flat YAML player descriptor.
pub const GENERIC_STREAM_RESOLVER_ID: &str = "stream-resolver";

/// Resolver identifier for a service-owned YAML `resolve_stream` query.
pub const SCRAPER_QUERY_STREAM_RESOLVER_ID: &str = "scraper-query";

/// Default first page used when a stream catalog request omits pagination.
pub(crate) const STREAM_DEFAULT_PAGE: usize = 1;

// Defaults shared by several service resolvers.

/// Alphanumeric character count of opaque in-memory DRM license proxy tokens.
pub(crate) const PLAYER_PROXY_TOKEN_LENGTH: usize = 32;

/// Default browser identity shared by resolvers without a provider-specific user-agent.
pub(crate) const PLAYER_DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:148.0) Gecko/20100101 Firefox/148.0";

/// Maximum diagnostic response preview length in Unicode characters across service resolvers.
pub(crate) const PLAYER_ERROR_BODY_PREVIEW_LENGTH: usize = 300;

// Test-only source-check contract and runner bounds.

/// Supported version of the independent source-check YAML schema.
#[cfg(test)]
pub(crate) const SOURCE_CHECK_SCHEMA_VERSION: u32 = 1;
/// Maximum number of facade steps in one source contract.
#[cfg(test)]
pub(crate) const SOURCE_CHECK_MAX_STEPS: usize = 32;
/// Default per-step deadline in seconds.
#[cfg(test)]
pub(crate) const SOURCE_CHECK_STEP_TIMEOUT_SECONDS: u64 = 60;
/// Maximum per-step deadline in seconds.
#[cfg(test)]
pub(crate) const SOURCE_CHECK_MAX_STEP_TIMEOUT_SECONDS: u64 = 120;
/// Default per-source deadline in seconds.
#[cfg(test)]
pub(crate) const SOURCE_CHECK_TIMEOUT_SECONDS: u64 = 300;
/// Maximum per-source deadline in seconds.
#[cfg(test)]
pub(crate) const SOURCE_CHECK_MAX_TIMEOUT_SECONDS: u64 = 900;

/// Crate-relative location of independent source-check contracts.
#[cfg(test)]
pub(crate) const SOURCE_CHECK_FIXTURES_DIRECTORY: &str = "tests/fixtures/services";

/// Maximum cache shutdown wait in seconds after a source-check operation ends.
#[cfg(test)]
pub(crate) const SOURCE_CHECK_CLEANUP_TIMEOUT_SECONDS: u64 = 5;
