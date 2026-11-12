#![warn(missing_docs)]
//! Arachnea stream service facade and player resolvers.

/// Stream-specific hooks and compatibility paths for Scrapyfy administration.
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
/// WebVTT document processing, subtitle descriptors and translation planning.
pub mod subtitles;
/// Effective public translation catalog built from Scrapyfy metadata.
pub mod translation_catalog;
/// Stream-backed adapter for the generic translation bridge.
pub mod translation_port;
/// Compatibility path for Scrapyfy's typed encrypted source credentials store.
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
pub use arachnea_proxy::PROXY_LOOPBACK_DEFAULT_PORT as DEFAULT_LOOPBACK_PROXY_PORT;

/// Shared proxy configuration option names, retained for compatibility.
pub use arachnea_scrapyfy::{PROXY_ENABLED_KEY, PROXY_KEYS_KEY, PROXY_NETWORK_KEY, PROXY_PORT_KEY};

// Stream facade and generic resolver

/// Default group name used by the stream scraper crate.
pub const STREAM_SERVICE_GROUP_NAME: &str = "arachnea-stream";

/// Persistence namespace containing administrator service activation overrides.
pub use arachnea_scrapyfy::SERVICES_STORE_NAME as STREAM_SERVICES_STORE_NAME;

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

/// Command listing the effective public translation services.
pub(crate) const STREAM_GET_TRANSLATION_SERVICES_COMMAND: &str = "get_translation_services";

/// Stream command translating subtitles directly with conditional validation.
pub(crate) const STREAM_TRANSLATE_SUBTITLE_COMMAND: &str = "translate_subtitle";

/// Stream resolver max embed html bytes byte limit.
pub(crate) const STREAM_RESOLVER_MAX_EMBED_HTML_BYTES: usize = 1_048_576;

/// Group name used by the translation service configuration.
pub const TRANSLATION_SERVICE_GROUP_NAME: &str = "arachnea-translation";

/// Default path used by the translation services configuration.
pub const DEFAULT_TRANSLATION_SERVICE_CONFIG_PATH: &str = const_format::concatcp!(
    arachnea_scrapyfy::DEFAULT_SERVICES_DIRECTORY,
    "/",
    TRANSLATION_SERVICE_GROUP_NAME,
    "/services.json"
);

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

// Subtitle document and cache contract identifiers.

/// Media type of a complete WebVTT document.
pub const WEBVTT_MEDIA_TYPE: &str = "text/vtt";

/// Media type of a SubRip document.
pub const SRT_MEDIA_TYPE: &str = "application/x-subrip";

/// Legacy revision identifier of the subtitle translation pipeline; not included in ETags.
pub const SUBTITLE_TRANSLATION_PIPELINE_REVISION: &str = "subtitles-1";

/// Legacy revision identifier of the WebVTT parser and serializer; not included in ETags.
pub const WEBVTT_PARSER_REVISION: &str = "webvtt-parser-1";

/// Latest supported HTTP date in an untrusted subtitle entity tag, as Unix seconds.
pub const SUBTITLE_ETAG_MAX_DATE_UNIX_SECONDS: u64 = 253_402_300_799;

/// Maximum encoded entity-tag length accepted from the client, in bytes.
pub const SUBTITLE_ETAG_MAX_LENGTH: usize = 4096;

/// Default maximum decompressed source document size, in bytes.
pub const DEFAULT_MAX_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;

/// Default maximum number of cues accepted in one document.
pub const DEFAULT_MAX_CUES: u64 = 10_000;

/// Default maximum translatable units accepted in one document.
pub const DEFAULT_MAX_TRANSLATABLE_UNITS: u64 = 500_000;

/// Maximum encoded proxy options length accepted for a subtitle source, in bytes.
pub const SUBTITLE_PROXY_OPTIONS_MAX_LENGTH: usize = 4096;

/// Maximum redirects followed when retrieving a subtitle source.
pub const SUBTITLE_SOURCE_MAX_REDIRECTS: usize = 5;

/// Timeout for each subtitle source request, in seconds.
pub const SUBTITLE_SOURCE_TIMEOUT_SECONDS: u64 = 30;

/// Maximum number of segments assembled for one finite HLS subtitle track.
pub(crate) const SUBTITLE_HLS_MAX_SEGMENTS: usize = 4096;

/// Maximum total retrieval and assembly time for an HLS subtitle rendition, in seconds.
pub(crate) const SUBTITLE_HLS_TIMEOUT_SECONDS: u64 = 120;

/// Default cache policy shared by translated subtitle file responses.
pub const DEFAULT_SUBTITLE_CACHE_CONTROL: &str = "private, no-cache";

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

/// Runtime parameter controlling the administrator selection order.
pub const TRANSLATION_ORDER_PARAMETER: &str = "translation_selection_order";

/// Runtime parameter marking the preferred translation provider.
pub const TRANSLATION_PREFERRED_PARAMETER: &str = "translation_preferred";

/// Runtime parameter bounding concurrent provider requests.
pub const TRANSLATION_MAX_CONCURRENT_PARAMETER: &str = "translation_max_concurrent_requests";

/// Runtime parameter bounding provider units per quota window.
pub const TRANSLATION_MAX_UNITS_PER_WINDOW_PARAMETER: &str = "translation_max_units_per_window";

/// Runtime parameter declaring the quota window in seconds.
pub const TRANSLATION_BUDGET_WINDOW_SECONDS_PARAMETER: &str = "translation_budget_window_seconds";

/// Default selection order when a service does not declare one.
pub const DEFAULT_TRANSLATION_ORDER: u64 = 1_000;

/// Default maximum concurrent provider requests.
pub const DEFAULT_TRANSLATION_MAX_CONCURRENT_REQUESTS: usize = 1;

/// Default account quota window: 30 days.
pub const DEFAULT_TRANSLATION_BUDGET_WINDOW_SECONDS: u64 = 2_592_000;
