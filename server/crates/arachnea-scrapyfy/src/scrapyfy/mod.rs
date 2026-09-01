use std::collections::HashMap;

/// The default directory for scraper service configuration files.
pub const DEFAULT_SERVICES_DIRECTORY: &str = "services";

/// Conditional request validation primitives (ETag fragments).
pub mod conditional;
pub use conditional::{
    conditional_from_fragment, fragment_from_response, fragment_yaml_hash, hash62,
    ConditionalRequest, RootFetchOutcome, ValidationSlot, CONTENT_FRAGMENT_PREFIX,
    ETAG_FRAGMENT_PREFIX, NO_VALIDATION_PREFIX,
};

/// Global ETag construction and decoding for aggregated responses.
pub mod global_etag;
pub use global_etag::{build_global_etag, decode_client_fragments, normalize_client_etag};

/// Runtime options controlling how a query is executed.
pub mod query_parameters;
pub use query_parameters::QueryParameters;

pub mod scraper_cache;
pub use scraper_cache::{
    server_cache_key, CacheType, ScraperCacheConfig, ScraperServerCache, ServerCacheInteraction,
};

/// Tree-shaped output node model used by all scraper pipelines.
pub mod scraper_data_node;

pub use scraper_data_node::{ScraperDataNode, ScraperOutputType};

/// HTTP client helpers used by query executors and tests.
pub mod http_client;
pub use arachnea_http::{ArachneaHttpError, HttpProxyConfig};
pub use http_client::HttpClient;
pub use http_client::{
    ScraperBrowserClickConfig, ScraperBrowserContext, ScraperBrowserTokenCacheScope,
    ScraperBrowserTokenConfig, ScraperBrowserTokenRetry, ScraperBrowserTokenSource,
    ScraperHttpConfig, ScraperHttpExecution, ScraperHttpMode, ScraperHttpUserAgentProfile,
    SharedProxyConfigHandle,
};

/// Text extraction and normalization actions.
pub mod actions;
pub use actions::{GetDateSource, GetDateSources, ScraperAction, HTTP_PROXY_PUBLIC_PATH_PARAM};

/// Post-process transformations applied after raw extraction.
pub mod post_processes;
pub use post_processes::{ScraperPostProcess, ScraperPostProcessContext};

/// Response-body transformations applied before parsing and content validation.
pub mod pre_processes;
pub use pre_processes::{apply_pre_processes, PreProcessAction};

/// HTML query definitions, executors, and field extractors.
pub mod scraper_html;
pub use scraper_html::config::HtmlScraperQueryRaw;
pub use scraper_html::entry::{HtmlScraperEntry, HtmlScraperEntryRaw, HtmlScraperSelectMode};
pub use scraper_html::query::HtmlScraperQuery;

/// JSON query definitions, executors, and field extractors.
pub mod scraper_json;
pub use scraper_json::config::EntrySubQueryRaw;
pub use scraper_json::entry::{JsonScraperEntry, JsonScraperEntryRaw};
pub use scraper_json::query::{JsonScraperQuery, JsonScraperQueryRaw};

/// Static query definitions and executors.
pub mod scraper_static;
pub use scraper_static::query::{StaticScraperEntryRaw, StaticScraperQuery, StaticScraperQueryRaw};

/// Text scraper — parses a text payload split by row and field delimiters.
pub mod scraper_text;
pub use scraper_text::entry::{TextScraperEntry, TextScraperEntryRaw};
pub use scraper_text::query::{TextScraperQuery, TextScraperQueryRaw};

/// Shared traits, spec types, and execution engine for scraper queries.
pub(crate) mod scraper;

/// Shared helpers for query URL formatting and filtering.
pub mod query_helpers;

/// Shared local-country state for geo-targeted proxy bypass decisions.
pub mod local_country;
pub use local_country::SharedLocalCountry;

/// Query collection models and file loading helpers.
pub mod scraper_query_collection;
pub use scraper_query_collection::{
    ScraperQueryCollection, ScraperQueryCollectionParameter, ScraperQueryCollectionRaw,
    ScraperServiceCredentials,
};

/// Proxy data provider placeholder for dynamic proxy loading.
#[cfg(feature = "arachnea-proxy")]
pub mod proxy_provider;
#[cfg(feature = "arachnea-proxy")]
pub use proxy_provider::{
    default_scrapyfy_ip_country_resolver, default_scrapyfy_proxy_core,
    default_scrapyfy_proxy_inventory, ScrapyfyProxyDataProvider,
};

/// IP-to-country resolution provider.
#[cfg(feature = "arachnea-proxy")]
pub mod ip_country_provider;
#[cfg(feature = "arachnea-proxy")]
pub use ip_country_provider::{
    refresh_ip_country_store, IpCountryRefreshConfig, ScrapyfyIpCountryDataProvider,
};

/// Multi-source query aggregator.
pub mod scraper_agregator;
pub use scraper_agregator::{resolve_manifest_sources, ScraperAgregator, ScraperSourceParams};
/// Source activation policy contracts and persistence-backed implementation.
pub mod source_enabled;
pub use source_enabled::{
    PersistenceSourceEnabled, ScraperSourceDescriptor, ScraperSourceEnabled,
    SourceEnabledRepository, SourceServiceRecord, TypedSourceEnabledRepository,
};
/// Complete read-only service catalog loading.
pub mod service_catalog;
pub use service_catalog::{
    load_service_catalog, load_service_catalog_detailed, ScraperServiceCatalogEntry,
    ServiceCatalogFailure, ServiceCatalogFailureReason, ServiceCatalogLoad,
};

/// Source-scoped request parameter parsing helpers.
pub mod source_params;
pub use source_params::{source_params_from_entries, ScraperSourceParamsRequestEntry};

/// Scraper manager trait used by runtime and test harnesses.
pub mod scraper_manager;
pub use scraper_manager::ScraperManager;

/// Serializable envelope for JSON command responses with per-source errors.
pub mod scraper_result;
