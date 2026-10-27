#![warn(missing_docs)]
//! Generic scraping engine used by the Arachnea stream backend.

/// Administration service: routes `admin/*`, sessions, authentication, and
/// multi-service-store source management.
pub mod admin;

/// Scraper query models, executors, and post-processors.
pub mod scrapyfy;

pub use scrapyfy::*;

// Crate defaults and stable configuration contracts.

// Service and validation contracts

/// The default directory for scraper service configuration files.
pub const DEFAULT_SERVICES_DIRECTORY: &str = "services";

/// Separator between the `name=fragment` blocks of the global ETag.
pub const GLOBAL_ETAG_SEPARATOR: char = ';';

/// Prefix marking an ETag-based validator fragment.
pub const ETAG_FRAGMENT_PREFIX: &str = "E:";

/// Prefix marking a content-hash validator fragment.
pub const CONTENT_FRAGMENT_PREFIX: &str = "C:";

/// Prefix marking a source that does not support conditional validation.
pub const NO_VALIDATION_PREFIX: &str = "N:";

/// Reserved prefix of request-scoped dynamic template variables.
pub const DYNAMIC_TEMPLATE_VARIABLE_PREFIX: char = '@';

/// Dependency log targets configured by the scraper manager.
pub(crate) const SCRAPER_LOGGER_TARGETS: [&str; 5] = [
    "selectors::matching",
    "html5ever::tree_builder",
    "html5ever::tokenizer",
    "hyper::proto::h1::io",
    "hyper::proto::h1::conn",
];

/// Whether a manifest source is enabled when its flag is omitted.
pub(crate) const SCRAPER_SOURCE_DEFAULT_ENABLED: bool = true;

// Server cache and settings

/// Configuration key of the explicit current-country override.
pub const CURRENT_COUNTRY_KEY: &str = "current-country";

/// Configuration key of the maximum on-disk server cache size, in K (kilobytes).
pub const CACHE_MAX_DISK_BYTES_KEY: &str = "cache-max-disk-bytes";

/// Configuration key of the maximum in-memory server cache size, in K (kilobytes).
pub const CACHE_MAX_MEMORY_BYTES_KEY: &str = "cache-max-memory-bytes";

/// Configuration key of the cache block size, in K (kilobytes).
pub const CACHE_BLOCK_SIZE_KEY: &str = "cache-block-size";

/// Safety bound on the age of a server-cache entry. Freshness is normally
/// driven by ETag revalidation; entries older than this are dropped.
pub const SERVER_CACHE_ENTRY_TTL: std::time::Duration =
    std::time::Duration::from_secs(365 * 24 * 60 * 60);

/// Default maximum on-disk size of the server cache, in K (kilobytes): 100 MiB.
pub const DEFAULT_CACHE_MAX_DISK_KIB: u64 = 100 * 1024;

/// Default maximum in-memory size of the server cache, in K (kilobytes): 32 MiB.
pub const DEFAULT_CACHE_MAX_MEMORY_KIB: u64 = 32 * 1024;

/// Default cache block size, in K (kilobytes): 16 MiB (foyer 0.22 default).
pub const DEFAULT_CACHE_BLOCK_SIZE_KIB: u64 = 16 * 1024;

/// Default maximum on-disk size of the server cache, derived from
/// [`DEFAULT_CACHE_MAX_DISK_KIB`].
pub const DEFAULT_CACHE_MAX_DISK_BYTES: u64 = DEFAULT_CACHE_MAX_DISK_KIB * 1024;

/// Default maximum in-memory size of the server cache, derived from
/// [`DEFAULT_CACHE_MAX_MEMORY_KIB`].
pub const DEFAULT_CACHE_MAX_MEMORY_BYTES: u64 = DEFAULT_CACHE_MAX_MEMORY_KIB * 1024;

/// Persisted cache-key component separator; changing it invalidates existing keys.
pub(crate) const SERVER_CACHE_KEY_SEPARATOR: &str = "\u{1}";

/// Persisted cache-key name/value separator; changing it invalidates existing keys.
pub(crate) const SERVER_CACHE_KEY_PAIR_SEPARATOR: &str = "\u{2}";

/// Number of bytes in one kibibyte; the cache configuration unit contract.
pub(crate) const SCRAPER_KIB: u64 = 1024;

// Administration

/// Name of the session cookie sent to authenticated administrators.
pub const ADMIN_SESSION_COOKIE: &str = "arachnea_admin_session";

/// Lifetime of one administrator session.
pub(crate) const ADMIN_SESSION_TTL: std::time::Duration =
    std::time::Duration::from_secs(2 * 60 * 60);

/// Number of failed logins accepted per peer inside the rate-limit window.
pub(crate) const ADMIN_MAX_FAILED_LOGINS: usize = 10;

/// Window during which failed logins are counted per peer.
pub(crate) const ADMIN_RATE_LIMIT_WINDOW: std::time::Duration =
    std::time::Duration::from_secs(10 * 60);

/// Length of the temporary administrator password generated when no permanent
/// hash is configured.
pub(crate) const ADMIN_TEMPORARY_PASSWORD_LENGTH: usize = 24;

/// Delay between the `update-settings` response and the hot application of
/// the new server settings, so the response flows on the listener still alive
/// before it is stopped and re-bound.
pub(crate) const ADMIN_APPLY_SETTINGS_DELAY: std::time::Duration =
    std::time::Duration::from_millis(1000);

// HTTP and browser configuration

/// Runtime parameter containing the public path of the generic HTTP proxy.
pub const HTTP_PROXY_PUBLIC_PATH_PARAM: &str = "__arachnea_http_proxy_public_path";

/// Maximum number of characters included in invalid JSON payload logs.
pub(crate) const HTTP_INVALID_JSON_LOG_PREVIEW_LIMIT: usize = 8_192;

/// Default scraper request timeout, independent of localized proxy discovery budgets.
pub(crate) const SCRAPER_HTTP_REQUEST_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(25);

/// Default request mode for scraper HTTP configuration without a mode override.
pub(crate) const SCRAPER_DEFAULT_HTTP_MODE: crate::scrapyfy::http_client::ScraperHttpMode =
    crate::scrapyfy::http_client::ScraperHttpMode::Auto;

/// Default retry policy for browser callback tokens.
pub(crate) const BROWSER_DEFAULT_TOKEN_RETRY:
    crate::scrapyfy::http_client::ScraperBrowserTokenRetry =
    crate::scrapyfy::http_client::ScraperBrowserTokenRetry::Never;

// JavaScript execution

/// Maximum injected HTML/configured scripts executed before the main pipeline text.
pub(crate) const JS_MAX_SCRIPTS: usize = 50;

/// Default JavaScript runtime loop iteration limit.
pub(crate) const JS_DEFAULT_LOOP_LIMIT: u64 = 10_000_000;

/// Default JavaScript action timeout in milliseconds.
pub(crate) const JS_DEFAULT_TIMEOUT_MS: u64 = 500;

/// JavaScript loop iterations allowed per configured millisecond before applying the floor.
pub(crate) const JS_LOOP_ITERATIONS_PER_MS: u64 = 20_000;

// Query and post-process execution

/// Maximum number of in-flight HTTP requests issued by a single
/// `fetch_regex_items_from_items` post-process step.
pub(crate) const POST_PROCESS_FETCH_CONCURRENCY: usize = 8;

/// Sentinel value used as `request_pointer` default for entry-level sub-queries,
/// signalling that the entry's value should be used directly as the fetch URL.
pub(crate) const QUERY_PARENT_SENTINEL: &str = "parent";

/// Maximum number of follow-up requests executed together (sane default for
/// both HTML and JSON; legacy code used 8 for JSON, 4 for HTML siblings).
///
/// This controls the concurrency level for parallel HTTP requests during
/// sub-query execution.
pub(crate) const QUERY_DEFAULT_FETCH_CONCURRENCY: usize = 8;

/// Maximum number of isolated browser pages opened for one entry sub-query.
///
/// Browser pages are substantially more expensive than direct HTTP requests,
/// so this limit is intentionally lower than [`QUERY_DEFAULT_FETCH_CONCURRENCY`].
pub(crate) const QUERY_DEFAULT_ISOLATED_PAGE_CLICK_CONCURRENCY: usize = 4;

/// Default concurrent HTML row extraction limit.
pub(crate) const HTML_DEFAULT_ROW_CONCURRENCY: usize = 4;

/// Default concurrent sibling JSON sub-query limit.
pub(crate) const JSON_DEFAULT_SIBLING_SUB_QUERY_CONCURRENCY: usize = 4;

/// Default concurrent contexts within one JSON sub-query.
pub(crate) const JSON_DEFAULT_SUB_QUERY_CONTEXT_CONCURRENCY: usize = 4;

/// Default concurrent follow-up fetches within one JSON sub-query.
pub(crate) const JSON_DEFAULT_SUB_QUERY_FETCH_CONCURRENCY: usize = 8;

/// Default query cache policy, preserving client-driven conditional validation.
pub(crate) const SCRAPER_DEFAULT_CACHE_TYPE: crate::scrapyfy::scraper_cache::CacheType =
    crate::scrapyfy::scraper_cache::CacheType::ClientCache;

/// Default serialized HTTP method for scraper queries.
pub(crate) const SCRAPER_DEFAULT_REQUEST_METHOD:
    crate::scrapyfy::scraper::config::ScraperRequestMethod =
    crate::scrapyfy::scraper::config::ScraperRequestMethod::Get;

/// Default HTML selection mode when the YAML selector mode is omitted.
pub(crate) const SCRAPER_DEFAULT_SELECT_MODE:
    crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode =
    crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode::All;

/// Default entries field in pagination post-processing.
pub(crate) const PAGINATION_DEFAULT_ENTRIES_FIELD: &str = "entries";

/// Default current page field in pagination post-processing.
pub(crate) const PAGINATION_DEFAULT_CURRENT_PAGE_FIELD: &str = "current_page";

/// Default have more field in pagination post-processing.
pub(crate) const PAGINATION_DEFAULT_HAVE_MORE_FIELD: &str = "have_more";

/// Default next param in pagination post-processing.
pub(crate) const PAGINATION_DEFAULT_NEXT_PARAM: &str = "page";

/// Default source params target in pagination post-processing.
pub(crate) const PAGINATION_DEFAULT_SOURCE_PARAMS_TARGET: &str = "source_params";

// Proxy providers

/// The group name used for the scrapyfy proxy query collection.
#[cfg(feature = "arachnea-proxy")]
pub(crate) const SCRAPER_PROXIES_GROUP_NAME: &str = "arachnea-proxies";

/// The group name used for the IP-country query collection.
#[cfg(feature = "arachnea-proxy")]
pub(crate) const SCRAPER_IP_COUNTRY_GROUP_NAME: &str = "arachnea-ip-countries";

/// Default relative path of the IP-country JSON store.
#[cfg(feature = "arachnea-proxy")]
pub(crate) const IP_COUNTRY_DEFAULT_STORE_PATH: &str = "data/ip-countries.json";

/// Successful provider IP-country lookup cache lifetime.
#[cfg(feature = "arachnea-proxy")]
pub(crate) const PROXY_GEO_SUCCESS_TTL: std::time::Duration = std::time::Duration::from_secs(900);

/// Failed provider IP-country lookup cache lifetime.
#[cfg(feature = "arachnea-proxy")]
pub(crate) const PROXY_GEO_FAILURE_TTL: std::time::Duration = std::time::Duration::from_secs(60);

/// Maximum wait for a provider IP-country query.
#[cfg(feature = "arachnea-proxy")]
pub(crate) const PROXY_GEO_QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Maximum concurrent country loads from scraper proxy sources.
#[cfg(feature = "arachnea-proxy")]
pub(crate) const PROXY_COUNTRY_LOAD_CONCURRENCY: usize = 2;

/// Maximum concurrent provider IP-country lookup jobs.
#[cfg(feature = "arachnea-proxy")]
pub(crate) const PROXY_GEO_QUERY_CONCURRENCY: usize = 4;
