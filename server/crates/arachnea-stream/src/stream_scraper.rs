use anyhow::{bail, Result};
use const_format::concatcp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use urlencoding::encode;

use arachnea_core::{
    controler::{
        ControlerService, ControlerServiceExt, ControlerStreamInput, ControlerStreamOutput,
        ResponseBody,
    },
    persistence::{CredentialsStore, FileCredentialsStore, FilePersistenceStore, PersistenceStore},
};
use arachnea_proxy::core::{ArachneaProxyCore, ProxyConfig};
use arachnea_scrapyfy::{
    scraper_result::{ScraperAggregationResult, ScraperSourceStatus},
    *,
};

use crate::services::{
    francetv_resolver::FrancetvResolver,
    m6play_resolver::M6PlayResolver,
    player_resolver::{PlayerResolverEndpoints, PlayerStreamResolver},
    rtbf_auvio_resolver::RtbfAuvioResolver,
    rtlplay_resolver::RtlPlayResolver,
    tf1_resolver::Tf1Resolver,
};
use crate::stream_etag::{build_global_etag, decode_client_fragments, normalize_client_etag};
use crate::stream_resolver::{
    ResolvedStream, StreamResolver, GENERIC_STREAM_RESOLVER_ID, STREAM_RESOLVER_CONFIG_PATH,
    STREAM_RESOLVER_GROUP_NAME,
};

/// Default group name used by the stream scraper crate.
pub const STREAM_SERVICE_GROUP_NAME: &str = "arachnea-stream";

/// Default path used by the services
pub const DEFAULT_SERVICES_CONFIG_PATH: &str = concatcp!(
    DEFAULT_SERVICES_DIRECTORY,
    "/",
    STREAM_SERVICE_GROUP_NAME,
    "/services.json"
);

const HTTP_PROXY_COMMAND: &str = "proxy";
const DRM_LICENSE_PROXY_COMMAND: &str = "get_drm_license";

static M6PLAY_RESOLVER: M6PlayResolver = M6PlayResolver;
static RTBF_AUVIO_RESOLVER: RtbfAuvioResolver = RtbfAuvioResolver;
static RTLPLAY_RESOLVER: RtlPlayResolver = RtlPlayResolver;
static TF1_RESOLVER: Tf1Resolver = Tf1Resolver;
static FRANCETV_RESOLVER: FrancetvResolver = FrancetvResolver;

fn default_page() -> usize {
    1
}

/// Returns `true` — conditional ETag validation is enabled by default.
fn default_enable_etag() -> bool {
    true
}

/// Selects the explicit validation value or the HTTP conditional validator.
///
/// REST callers normally provide `If-None-Match`; the scraper engine also
/// needs that value to decode its per-source fragments before it schedules
/// follow-up queries. Tauri callers have no HTTP headers and keep using the
/// explicit `arachneaEtag` field.
fn request_arachnea_etag(
    explicit_etag: Option<String>,
    headers: &HashMap<String, String>,
) -> Option<String> {
    explicit_etag.or_else(|| {
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("if-none-match"))
            .map(|(_, value)| value.clone())
    })
}

#[derive(Serialize, Deserialize)]
struct SearchRequest {
    query: String,
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default, alias = "mediaTypes")]
    media_types: Vec<String>,
    #[serde(default)]
    themes: Vec<String>,
    #[serde(default, alias = "sourceParams")]
    source_params: Vec<SourceParamsRequestEntry>,
    /// Client-provided global ETag (`If-None-Match`) for conditional validation.
    #[serde(default, alias = "arachneaEtag")]
    arachnea_etag: Option<String>,
    /// Whether conditional ETag validation is enabled for this request.
    #[serde(default = "default_enable_etag", alias = "enableEtag")]
    enable_etag: bool,
}

#[derive(Serialize, Deserialize)]
struct GetEntryRequest {
    entry: String,
    source: String,
    /// Client-provided global ETag (`If-None-Match`) for conditional validation.
    #[serde(default, alias = "arachneaEtag")]
    arachnea_etag: Option<String>,
    /// Whether conditional ETag validation is enabled for this request.
    #[serde(default = "default_enable_etag", alias = "enableEtag")]
    enable_etag: bool,
}

#[derive(Serialize, Deserialize)]
struct GetSeasonRequest {
    season: String,
    source: String,
    #[serde(default = "default_page")]
    page: usize,
    /// Client-provided global ETag (`If-None-Match`) for conditional validation.
    #[serde(default, alias = "arachneaEtag")]
    arachnea_etag: Option<String>,
    /// Whether conditional ETag validation is enabled for this request.
    #[serde(default = "default_enable_etag", alias = "enableEtag")]
    enable_etag: bool,
}

#[derive(Serialize, Deserialize)]
struct ListLivesRequest {
    /// Client-provided global ETag (`If-None-Match`) for conditional validation.
    #[serde(default, alias = "arachneaEtag")]
    arachnea_etag: Option<String>,
    /// Whether conditional ETag validation is enabled for this request.
    #[serde(default = "default_enable_etag", alias = "enableEtag")]
    enable_etag: bool,
}

#[derive(Serialize, Deserialize)]
struct GetLiveRequest {
    channel: String,
    source: String,
    /// Client-provided global ETag (`If-None-Match`) for conditional validation.
    #[serde(default, alias = "arachneaEtag")]
    arachnea_etag: Option<String>,
    /// Whether conditional ETag validation is enabled for this request.
    #[serde(default = "default_enable_etag", alias = "enableEtag")]
    enable_etag: bool,
}

#[derive(Serialize, Deserialize)]
struct GetStreamRequest {
    resolver: String,
    target: String,
}

#[derive(Default, Serialize, Deserialize)]
struct LoadHomeRequest {
    /// Client-provided global ETag (`If-None-Match`) for conditional validation.
    #[serde(default, alias = "arachneaEtag")]
    arachnea_etag: Option<String>,
    /// Whether conditional ETag validation is enabled for this request.
    #[serde(default = "default_enable_etag", alias = "enableEtag")]
    enable_etag: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct GetServiceRequest {
    /// Client-provided global ETag (`If-None-Match`) for conditional validation.
    #[serde(default, alias = "arachneaEtag")]
    arachnea_etag: Option<String>,
    /// Whether conditional ETag validation is enabled for this request.
    #[serde(default = "default_enable_etag", alias = "enableEtag")]
    enable_etag: bool,
}

#[derive(Serialize, Deserialize)]
struct GetCategoryRequest {
    #[serde(default)]
    source: HashMap<String, String>,
    #[serde(default)]
    sources: Vec<HashMap<String, String>>,
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default, alias = "sourceParams")]
    source_params: Vec<SourceParamsRequestEntry>,
    /// Client-provided global ETag (`If-None-Match`) for conditional validation.
    #[serde(default, alias = "arachneaEtag")]
    arachnea_etag: Option<String>,
    /// Whether conditional ETag validation is enabled for this request.
    #[serde(default = "default_enable_etag", alias = "enableEtag")]
    enable_etag: bool,
}

#[derive(Serialize, Deserialize)]
struct GetSectionRequest {
    #[serde(default)]
    source: String,
    #[serde(default)]
    link: String,
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default, alias = "sourceParams")]
    source_params: Vec<SourceParamsRequestEntry>,
    /// Client-provided global ETag (`If-None-Match`) for conditional validation.
    #[serde(default, alias = "arachneaEtag")]
    arachnea_etag: Option<String>,
    /// Whether conditional ETag validation is enabled for this request.
    #[serde(default = "default_enable_etag", alias = "enableEtag")]
    enable_etag: bool,
}

#[derive(Serialize, Deserialize)]
struct GetBannersRequest {
    #[serde(default)]
    source: String,
    #[serde(default)]
    link: String,
    /// Client-provided global ETag (`If-None-Match`) for conditional validation.
    #[serde(default, alias = "arachneaEtag")]
    arachnea_etag: Option<String>,
    /// Whether conditional ETag validation is enabled for this request.
    #[serde(default = "default_enable_etag", alias = "enableEtag")]
    enable_etag: bool,
}

#[derive(Serialize, Deserialize)]
struct GetPlayersRequest {
    #[serde(default)]
    source: String,
    #[serde(default)]
    link: String,
    /// Client-provided global ETag (`If-None-Match`) for conditional validation.
    #[serde(default, alias = "arachneaEtag")]
    arachnea_etag: Option<String>,
    /// Whether conditional ETag validation is enabled for this request.
    #[serde(default = "default_enable_etag", alias = "enableEtag")]
    enable_etag: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct SourceParamsRequestEntry {
    source: String,
    #[serde(flatten)]
    params: HashMap<String, Value>,
}

/// High-level facade exposing scraper operations used by controllers and tests.
pub struct StreamScraper {
    pub(crate) scraper_agregator: Box<ScraperAgregator>,
    credentials_store: Arc<dyn CredentialsStore>,
    proxy_handle: SharedProxyConfigHandle,
    proxy_http_core: Option<ArachneaProxyCore>,
    player_resolver_endpoints: PlayerResolverEndpoints,
}

impl Default for StreamScraper {
    /// Creates a default instance of the scraper facade using a file-based credentials store and the default services configuration.
    ///
    /// # Returns
    /// A configured scraper facade with default configuration.
    ///
    /// # Errors
    /// Returns an error if the default configuration file cannot be loaded or parsed.
    fn default() -> Self {
        Self::new(FileCredentialsStore::default().as_arc())
    }
}

impl StreamScraper {
    /// Creates a scraper facade backed by the provided credentials store.
    ///
    /// # Arguments
    /// * `path` - Path to the services configuration JSON file relative to the application data directory.
    /// * `credentials_store` - Shared credentials persistence used by service resolvers.
    ///
    /// # Returns
    /// A configured scraper facade.
    ///
    /// # Errors
    /// Returns an error if the configuration file cannot be loaded or parsed.
    #[allow(clippy::too_many_arguments)]
    pub fn new(credentials_store: Arc<dyn CredentialsStore>) -> Self {
        Self::new_with_persistence_store(
            credentials_store,
            Arc::new(FilePersistenceStore::default_data_dir()),
        )
    }

    /// Creates a scraper facade backed by the provided credentials store and
    /// a shared persistence store.
    ///
    /// # Arguments
    ///
    /// * `credentials_store` - Shared credentials persistence used by service resolvers.
    /// * `persistence_store` - Shared persistence store used by created HTTP clients.
    ///
    /// # Returns
    ///
    /// A configured scraper facade.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration file cannot be loaded or parsed.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_persistence_store(
        credentials_store: Arc<dyn CredentialsStore>,
        persistence_store: Arc<dyn PersistenceStore>,
    ) -> Self {
        let mut agregator = Box::new(ScraperAgregator::new_with_persistence_store(
            persistence_store.clone(),
        ));
        agregator.ensure_proxy_core();
        let proxy_handle = agregator.get_proxy_handle();
        let proxy_http_core = agregator.proxy_core().cloned();

        StreamScraper {
            scraper_agregator: agregator,
            credentials_store,
            proxy_handle,
            proxy_http_core,
            player_resolver_endpoints: PlayerResolverEndpoints::default(),
        }
    }

    /// Creates a instance of the scraper facade using a file-based credentials store and the default services configuration.
    ///
    /// # Returns
    /// A configured scraper facade with default configuration.
    ///
    /// # Errors
    /// Returns an error if the default configuration file cannot be loaded or parsed.
    pub fn from_json(json_path: Option<&str>) -> Result<Self> {
        let mut instance = Self::default();

        instance
            .scraper_agregator
            .add_query_collection_from_config_json(
                STREAM_SERVICE_GROUP_NAME,
                json_path.unwrap_or(DEFAULT_SERVICES_CONFIG_PATH),
            )?;
        instance
            .scraper_agregator
            .add_query_collection_from_config_json(
                STREAM_RESOLVER_GROUP_NAME,
                STREAM_RESOLVER_CONFIG_PATH,
            )?;
        instance.configure_proxy_insecure_tls_hosts();

        Ok(instance)
    }

    /// Returns the mutable proxy handle shared by this scraper instance.
    pub fn proxy_handle(&self) -> SharedProxyConfigHandle {
        self.proxy_handle.clone()
    }

    /// Stores the explicitly configured local country used by geo proxy decisions.
    ///
    /// # Arguments
    /// * `country` - ISO country code for the current outbound location.
    pub async fn set_current_country(&self, country: impl AsRef<str>) {
        let country = country.as_ref();
        self.scraper_agregator.set_explicit_local_country(country);
        if let Some(proxy_core) = self.scraper_agregator.proxy_core() {
            proxy_core.set_current_country(country).await;
        }
        if let Some(proxy_core) = &self.proxy_http_core {
            proxy_core.set_current_country(country).await;
        }
    }

    /// Replaces the proxy configuration used by this scraper instance.
    pub fn set_proxy(&self, proxy: HttpProxyConfig) {
        self.proxy_handle.set_proxy(proxy);
    }

    /// Disables the proxy override used by this scraper instance.
    pub fn clear_proxy(&self) {
        self.proxy_handle.clear_proxy();
    }

    /// Stores the generic HTTP proxy core registered with the controller.
    ///
    /// # Arguments
    /// * `proxy_http_core` - Optional proxy core used by the `/proxy` stream command.
    pub fn set_proxy_http_core(&mut self, proxy_http_core: Option<ArachneaProxyCore>) {
        self.proxy_http_core = proxy_http_core;
        self.configure_proxy_insecure_tls_hosts();
    }

    /// Synchronizes trusted resolver TLS-bypass hosts into the local proxy core.
    fn configure_proxy_insecure_tls_hosts(&self) {
        if let Some(proxy_core) = &self.proxy_http_core {
            proxy_core.set_insecure_tls_hosts(
                self.scraper_agregator
                    .group_proxy_insecure_tls_hosts(STREAM_RESOLVER_GROUP_NAME),
            );
        }
    }

    fn enrich_runtime_params(&self, params: &mut HashMap<String, String>) {
        if let Some(proxy_public_path) = self
            .player_resolver_endpoints
            .http_proxy_public_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            params.insert(
                HTTP_PROXY_PUBLIC_PATH_PARAM.to_string(),
                proxy_public_path.to_string(),
            );
        }
    }

    /// Executes an aggregated query with optional conditional ETag validation.
    ///
    /// When enabled, phase 1 validates every involved source in parallel using
    /// the client fragments decoded from the incoming global ETag; unchanged
    /// sources skip parsing entirely. A second pass re-fetches stale sources
    /// without conditional headers so the aggregated payload stays complete.
    /// The returned global ETag is always built from the phase-1 fragments.
    ///
    /// # Arguments
    ///
    /// * `arachnea_etag` - Incoming client global ETag (`If-None-Match`), when any.
    /// * `enable_etag` - Whether conditional validation is enabled for this request.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag (`None`
    /// when validation is disabled or no service is involved).
    #[allow(clippy::too_many_arguments)]
    async fn execute_query_with_etag(
        &self,
        group_name: &str,
        query_name: &str,
        params: &HashMap<String, String>,
        source_params: Option<&ScraperSourceParams>,
        scrapper_list: Option<&Vec<String>>,
        query_media_type_filter: Option<&Vec<String>>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
        source_field_name: Option<&str>,
        operation: &str,
        arachnea_etag: Option<String>,
        enable_etag: bool,
    ) -> Result<(
        ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>,
        Option<String>,
    )> {
        let arachnea_etag = arachnea_etag
            .as_deref()
            .map(normalize_client_etag)
            .filter(|etag| !etag.is_empty())
            .map(str::to_string);

        if !enable_etag {
            let result = self
                .scraper_agregator
                .execute_query_async(
                    group_name,
                    query_name,
                    params,
                    source_params,
                    scrapper_list,
                    query_media_type_filter,
                    fields_filters,
                    source_field_name,
                    operation,
                    None,
                )
                .await;
            return Ok((result, None));
        }

        // Deterministic ordered service list involved in this request.
        let mut services: Vec<String> = match scrapper_list {
            Some(list) => list.clone(),
            None => self.scraper_agregator.source_names_in_group(group_name),
        };
        services.retain(|name| !name.trim().is_empty());
        services.sort();
        services.dedup();
        let service_refs: Vec<&str> = services.iter().map(String::as_str).collect();

        // Phase 1: parallel conditional validation per source.
        let client_fragments = arachnea_etag
            .as_deref()
            .and_then(|etag| decode_client_fragments(etag, &service_refs));

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                group_name,
                query_name,
                params,
                source_params,
                scrapper_list,
                query_media_type_filter,
                fields_filters,
                source_field_name,
                operation,
                client_fragments.as_ref(),
            )
            .await;

        let etag_fragments: HashMap<String, String> = result
            .validations
            .iter()
            .map(|(name, validation)| (name.clone(), validation.etag.clone()))
            .collect();
        let global_etag = build_global_etag(&service_refs, &etag_fragments);

        // Full 304 short-circuit: every validated source is stale and the
        // rebuilt global ETag matches what the client sent — no data needed.
        let all_stale = !result.validations.is_empty()
            && result
                .validations
                .values()
                .all(|validation| validation.status == ScraperSourceStatus::Stale);
        if all_stale && global_etag.is_some() && global_etag == arachnea_etag {
            return Ok((result, global_etag));
        }

        // Phase 2: forced full GETs for stale sources only, merging their
        // freshly parsed rows into the aggregate. The global ETag keeps the
        // phase-1 fragments — they already describe the current remote state.
        let stale_sources: Vec<String> = result
            .validations
            .iter()
            .filter(|(_, validation)| validation.status == ScraperSourceStatus::Stale)
            .map(|(name, _)| name.clone())
            .collect();

        if !stale_sources.is_empty() {
            let catch_up = self
                .scraper_agregator
                .execute_query_async(
                    group_name,
                    query_name,
                    params,
                    source_params,
                    Some(&stale_sources),
                    query_media_type_filter,
                    fields_filters,
                    source_field_name,
                    operation,
                    None,
                )
                .await;
            result.data.extend(catch_up.data);
            result.errors.extend(catch_up.errors);
        }

        Ok((result, global_etag))
    }

    /// Executes the `search` query using URL-encoded terms.
    ///
    /// # Arguments
    ///
    /// * `query` - Raw search string that will be URL-encoded before execution.
    /// * `media_types` - Optional media type filters applied to query selection and result filtering.
    /// * `themes` - Optional theme filters applied to scraped results.
    /// * `page` - 1-based page number requested by the caller.
    /// * `source_params` - Source-specific runtime parameters returned by the previous page.
    /// * `arachnea_etag` - Incoming client global ETag for conditional validation.
    /// * `enable_etag` - Whether conditional ETag validation is enabled.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn search(
        &self,
        query: String,
        media_types: Vec<String>,
        themes: Vec<String>,
        page: usize,
        source_params: ScraperSourceParams,
        arachnea_etag: Option<String>,
        enable_etag: bool,
    ) -> Result<(
        ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>,
        Option<String>,
    )> {
        let page = page.max(1);
        let scrapper_list = if page > 1 && !source_params.is_empty() {
            Some(source_params.keys().cloned().collect::<Vec<_>>())
        } else {
            None
        };

        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("search_query".to_string(), query.clone());
        params.insert(
            "search_query_json".to_string(),
            serde_json::to_string(&query)?,
        );
        params.insert("search_terms".to_string(), encode(&query).to_string());
        params.insert("media_types".to_string(), media_types.join(","));
        params.insert("themes".to_string(), themes.join(","));
        params.insert("page".to_string(), page.to_string());
        self.enrich_runtime_params(&mut params);

        let media = if media_types.is_empty() {
            None
        } else {
            Some(&media_types)
        };

        self.execute_query_with_etag(
            STREAM_SERVICE_GROUP_NAME,
            "search",
            &params,
            Some(&source_params),
            scrapper_list.as_ref(),
            media,
            None,
            None,
            "search",
            arachnea_etag,
            enable_etag,
        )
        .await
    }

    /// Convenience helper for the `get_entry` query using the provided absolute entry URL.
    ///
    /// # Arguments
    ///
    /// * `query_source` - Query source name.
    /// * `query_url` - Absolute URL of the entry page to fetch.
    /// * `arachnea_etag` - Incoming client global ETag for conditional validation.
    /// * `enable_etag` - Whether conditional ETag validation is enabled.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    ///
    /// # Errors
    ///
    /// Returns an error if query execution cannot be started.
    pub async fn get_entry(
        &self,
        query_source: String,
        query_url: String,
        arachnea_etag: Option<String>,
        enable_etag: bool,
    ) -> Result<(
        ScraperAggregationResult<HashMap<String, ScraperDataNode>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("query_url".to_string(), query_url);
        self.enrich_runtime_params(&mut params);

        let scrapper_list = vec![query_source];

        let (result, global_etag) = self
            .execute_query_with_etag(
                STREAM_SERVICE_GROUP_NAME,
                "get_entry",
                &params,
                None,
                Some(&scrapper_list),
                None,
                None,
                None,
                "get_entry",
                arachnea_etag,
                enable_etag,
            )
            .await?;

        let mut root = ScraperDataNode {
            children: result.data.into_iter().next().unwrap_or_default(),
            ..Default::default()
        };
        populate_stream_resolver_web_links(&mut root);

        Ok((
            ScraperAggregationResult::new(root.children, result.errors)
                .with_validations(result.validations),
            global_etag,
        ))
    }

    /// Convenience helper for the `get_season` query using the provided absolute season URL.
    ///
    /// # Arguments
    ///
    /// * `query_source` - Query source name.
    /// * `query_url` - Absolute URL of the season payload to fetch.
    /// * `page` - 1-based page number requested from the backend source.
    /// * `arachnea_etag` - Incoming client global ETag for conditional validation.
    /// * `enable_etag` - Whether conditional ETag validation is enabled.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    ///
    /// # Errors
    ///
    /// Returns an error if query execution cannot be started.
    pub async fn get_season(
        &self,
        query_source: String,
        query_url: String,
        page: usize,
        arachnea_etag: Option<String>,
        enable_etag: bool,
    ) -> Result<(
        ScraperAggregationResult<HashMap<String, ScraperDataNode>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("query_url".to_string(), query_url);
        params.insert("page".to_string(), page.max(1).to_string());
        self.enrich_runtime_params(&mut params);

        let scrapper_list = vec![query_source];

        let (result, global_etag) = self
            .execute_query_with_etag(
                STREAM_SERVICE_GROUP_NAME,
                "get_season",
                &params,
                None,
                Some(&scrapper_list),
                None,
                None,
                None,
                "get_season",
                arachnea_etag,
                enable_etag,
            )
            .await?;

        Ok((
            ScraperAggregationResult::new(
                result.data.into_iter().next().unwrap_or_default(),
                result.errors,
            )
            .with_validations(result.validations),
            global_etag,
        ))
    }

    /// Loads the aggregated live catalog across every configured source.
    ///
    /// # Arguments
    ///
    /// * `arachnea_etag` - Incoming client global ETag for conditional validation.
    /// * `enable_etag` - Whether conditional ETag validation is enabled.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn list_lives(
        &self,
        arachnea_etag: Option<String>,
        enable_etag: bool,
    ) -> Result<(
        ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        self.enrich_runtime_params(&mut params);
        self.execute_query_with_etag(
            STREAM_SERVICE_GROUP_NAME,
            "list_lives",
            &params,
            None,
            None,
            None,
            None,
            Some("source"),
            "list_lives",
            arachnea_etag,
            enable_etag,
        )
        .await
    }

    /// Loads one live payload for the provided source and channel identifier.
    ///
    /// # Arguments
    ///
    /// * `query_source` - Query source name.
    /// * `channel` - Source-specific live channel identifier.
    /// * `arachnea_etag` - Incoming client global ETag for conditional validation.
    /// * `enable_etag` - Whether conditional ETag validation is enabled.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    ///
    /// # Errors
    ///
    /// Returns an error if query execution cannot be started.
    pub async fn get_live(
        &self,
        query_source: String,
        channel: String,
        arachnea_etag: Option<String>,
        enable_etag: bool,
    ) -> Result<(
        ScraperAggregationResult<HashMap<String, ScraperDataNode>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("channel".to_string(), channel);
        self.enrich_runtime_params(&mut params);

        let scrapper_list = vec![query_source];

        let (result, global_etag) = self
            .execute_query_with_etag(
                STREAM_SERVICE_GROUP_NAME,
                "get_live",
                &params,
                None,
                Some(&scrapper_list),
                None,
                None,
                None,
                "get_live",
                arachnea_etag,
                enable_etag,
            )
            .await?;

        Ok((
            ScraperAggregationResult::new(
                result.data.into_iter().next().unwrap_or_default(),
                result.errors,
            )
            .with_validations(result.validations),
            global_etag,
        ))
    }

    /// Loads the aggregated home catalog across every configured source.
    ///
    /// # Arguments
    ///
    /// * `arachnea_etag` - Incoming client global ETag for conditional validation.
    /// * `enable_etag` - Whether conditional ETag validation is enabled.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn load_home(
        &self,
        arachnea_etag: Option<String>,
        enable_etag: bool,
    ) -> Result<(
        ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        self.enrich_runtime_params(&mut params);
        self.execute_query_with_etag(
            STREAM_SERVICE_GROUP_NAME,
            "load_home",
            &params,
            None,
            None,
            None,
            None,
            Some("source"),
            "load_home",
            arachnea_etag,
            enable_etag,
        )
        .await
    }

    /// Loads display metadata for every configured source.
    ///
    /// # Arguments
    ///
    /// # Returns
    ///
    /// The aggregation result for every configured source.
    pub async fn get_service(
        &self,
    ) -> Result<ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>> {
        let (result, _) = self.get_service_with_etag(None, false).await?;
        Ok(result)
    }

    /// Loads display metadata for every configured source with optional ETag validation.
    async fn get_service_with_etag(
        &self,
        arachnea_etag: Option<String>,
        enable_etag: bool,
    ) -> Result<(
        ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        self.enrich_runtime_params(&mut params);

        self.execute_query_with_etag(
            STREAM_SERVICE_GROUP_NAME,
            "service_stream_metadata",
            &params,
            None,
            None,
            None,
            None,
            None,
            "service_stream_metadata",
            arachnea_etag,
            enable_etag,
        )
        .await
    }

    /// Loads category payloads for the provided YAML-defined source descriptors.
    ///
    /// # Arguments
    ///
    /// * `sources` - Source descriptors returned under `category.sources` by `load_home`.
    /// * `page` - 1-based page number requested from the backend source.
    /// * `source_params` - Optional per-source runtime parameters overriding global values.
    /// * `arachnea_etag` - Incoming client global ETag for conditional validation.
    /// * `enable_etag` - Whether conditional ETag validation is enabled.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    ///
    /// # Errors
    ///
    /// Returns an error if a required source name is missing from the request.
    pub async fn get_category(
        &self,
        sources: Vec<HashMap<String, String>>,
        page: usize,
        source_params: ScraperSourceParams,
        arachnea_etag: Option<String>,
        enable_etag: bool,
    ) -> Result<(
        ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>,
        Option<String>,
    )> {
        let page = page.max(1);
        let mut source_params = source_params;
        let mut scrapper_list = Vec::new();

        for mut source in sources {
            source.insert("page".to_string(), page.to_string());

            let query_source = source
                .remove("name")
                .or_else(|| source.remove("source"))
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("Missing category source name"))?;

            let existing_params = source_params.remove(&query_source).unwrap_or_default();
            source.extend(existing_params);
            source_params.insert(query_source.clone(), source);
            scrapper_list.push(query_source);
        }

        if scrapper_list.is_empty() {
            scrapper_list.extend(source_params.keys().cloned());
        }

        if scrapper_list.is_empty() {
            anyhow::bail!("Missing category source name");
        }

        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("page".to_string(), page.to_string());
        self.enrich_runtime_params(&mut params);

        self.execute_query_with_etag(
            STREAM_SERVICE_GROUP_NAME,
            "get_category",
            &params,
            Some(&source_params),
            Some(&scrapper_list),
            None,
            None,
            Some("source"),
            "get_category",
            arachnea_etag,
            enable_etag,
        )
        .await
    }

    /// Loads one paged section payload for the provided source and section link.
    ///
    /// # Arguments
    ///
    /// * `query_source` - Query source name.
    /// * `query_url` - Source-specific section URL or API endpoint.
    /// * `page` - 1-based page number requested from the backend source.
    /// * `source_params` - Optional per-source runtime parameters overriding global values.
    /// * `arachnea_etag` - Incoming client global ETag for conditional validation.
    /// * `enable_etag` - Whether conditional ETag validation is enabled.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn get_section(
        &self,
        query_source: String,
        query_url: String,
        page: usize,
        mut source_params: ScraperSourceParams,
        arachnea_etag: Option<String>,
        enable_etag: bool,
    ) -> Result<(
        ScraperAggregationResult<HashMap<String, ScraperDataNode>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("page".to_string(), page.max(1).to_string());
        self.enrich_runtime_params(&mut params);

        if !query_url.trim().is_empty() {
            params.insert("query_url".to_string(), query_url.clone());
            params.insert("link".to_string(), query_url);
        }

        for params_for_source in source_params.values_mut() {
            if let Some(link) = params_for_source.get("link").cloned() {
                params_for_source
                    .entry("query_url".to_string())
                    .or_insert(link);
            }
        }

        let scrapper_list = if !query_source.trim().is_empty() {
            Some(vec![query_source])
        } else if !source_params.is_empty() {
            Some(source_params.keys().cloned().collect())
        } else {
            None
        };

        let (result, global_etag) = self
            .execute_query_with_etag(
                STREAM_SERVICE_GROUP_NAME,
                "get_section",
                &params,
                Some(&source_params),
                scrapper_list.as_ref(),
                None,
                None,
                Some("source"),
                "get_section",
                arachnea_etag,
                enable_etag,
            )
            .await?;

        let mut root = ScraperDataNode::default();
        for row in result.data {
            root.merge_first(&ScraperDataNode {
                children: row,
                ..Default::default()
            });
        }
        root.keep_first_values();

        Ok((
            ScraperAggregationResult::new(root.children, result.errors)
                .with_validations(result.validations),
            global_etag,
        ))
    }

    /// Loads banners for the provided source and banner link.
    ///
    /// # Arguments
    ///
    /// * `query_source` - Query source name.
    /// * `query_url` - Source-specific banner URL or API endpoint.
    /// * `arachnea_etag` - Incoming client global ETag for conditional validation.
    /// * `enable_etag` - Whether conditional ETag validation is enabled.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn get_banners(
        &self,
        query_source: String,
        query_url: String,
        arachnea_etag: Option<String>,
        enable_etag: bool,
    ) -> Result<(
        ScraperAggregationResult<HashMap<String, ScraperDataNode>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        self.enrich_runtime_params(&mut params);

        if !query_url.trim().is_empty() {
            params.insert("query_url".to_string(), query_url.clone());
            params.insert("link".to_string(), query_url);
        }

        let scrapper_list = if query_source.trim().is_empty() {
            None
        } else {
            Some(vec![query_source])
        };

        let (result, global_etag) = self
            .execute_query_with_etag(
                STREAM_SERVICE_GROUP_NAME,
                "get_banners",
                &params,
                None,
                scrapper_list.as_ref(),
                None,
                None,
                Some("source"),
                "get_banners",
                arachnea_etag,
                enable_etag,
            )
            .await?;

        let mut root = ScraperDataNode::default();
        for row in result.data {
            root.merge_first(&ScraperDataNode {
                children: row,
                ..Default::default()
            });
        }
        root.keep_first_values();

        Ok((
            ScraperAggregationResult::new(root.children, result.errors)
                .with_validations(result.validations),
            global_etag,
        ))
    }

    /// Loads players for the provided source and player link.
    ///
    /// # Arguments
    ///
    /// * `query_source` - Query source name.
    /// * `query_url` - Source-specific player URL or API endpoint.
    /// * `arachnea_etag` - Incoming client global ETag for conditional validation.
    /// * `enable_etag` - Whether conditional ETag validation is enabled.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn get_players(
        &self,
        query_source: String,
        query_url: String,
        arachnea_etag: Option<String>,
        enable_etag: bool,
    ) -> Result<(
        ScraperAggregationResult<HashMap<String, ScraperDataNode>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        self.enrich_runtime_params(&mut params);

        if !query_url.trim().is_empty() {
            params.insert("query_url".to_string(), query_url.clone());
            params.insert("link".to_string(), query_url);
        }

        let scrapper_list = if query_source.trim().is_empty() {
            None
        } else {
            Some(vec![query_source])
        };

        let (result, global_etag) = self
            .execute_query_with_etag(
                STREAM_SERVICE_GROUP_NAME,
                "get_players",
                &params,
                None,
                scrapper_list.as_ref(),
                None,
                None,
                Some("source"),
                "get_players",
                arachnea_etag,
                enable_etag,
            )
            .await?;

        let mut root = ScraperDataNode::default();
        for row in result.data {
            root.merge_first(&ScraperDataNode {
                children: row,
                ..Default::default()
            });
        }
        root.keep_first_values();
        populate_stream_resolver_web_links(&mut root);

        Ok((
            ScraperAggregationResult::new(root.children, result.errors)
                .with_validations(result.validations),
            global_etag,
        ))
    }

    async fn get_stream(
        &self,
        resolver_id: String,
        target: String,
    ) -> Result<ScraperAggregationResult<ResolvedStream>> {
        let resolved = if resolver_id.trim() == GENERIC_STREAM_RESOLVER_ID {
            StreamResolver::new(&self.scraper_agregator, &self.player_resolver_endpoints)
                .get_stream(&target)
                .await?
        } else {
            let resolver = player_resolver_for_id(&resolver_id)
                .ok_or_else(|| anyhow::anyhow!("Unsupported player resolver `{}`.", resolver_id))?;
            let service_parameters = self
                .scraper_agregator
                .query_collection_parameters(STREAM_SERVICE_GROUP_NAME, resolver.source_id())
                .unwrap_or(&[]);

            ResolvedStream::Stream(
                resolver
                    .get_stream(
                        &self.scraper_agregator,
                        self.credentials_store.as_ref(),
                        &resolver_id,
                        &target,
                        service_parameters,
                        &self.player_resolver_endpoints,
                    )
                    .await?,
            )
        };

        Ok(ScraperAggregationResult::ok(resolved))
    }

    async fn get_drm_license(&self, input: ControlerStreamInput) -> Result<ControlerStreamOutput> {
        let stream_token = input.path.trim_matches('/').trim();
        if stream_token.is_empty() {
            bail!("Missing stream token.");
        }

        let (source, token) = stream_token
            .split_once('/')
            .unwrap_or(("m6play-fr", stream_token));
        let resolver = player_resolver_for_source(source)
            .ok_or_else(|| anyhow::anyhow!("Unsupported stream source `{}`.", source))?;
        let response = resolver
            .get_drm_license(&self.scraper_agregator, token, &input.body)
            .await?;

        Ok(ControlerStreamOutput {
            status: 200,
            body: ResponseBody::Buffered(response.body),
            content_type: response.content_type,
            headers: response.headers,
        })
    }
}

fn category_sources_from_request(
    source: HashMap<String, String>,
    sources: Vec<HashMap<String, String>>,
) -> Vec<HashMap<String, String>> {
    if !sources.is_empty() {
        return sources;
    }

    if source.is_empty() {
        Vec::new()
    } else {
        vec![source]
    }
}

fn player_resolver_for_source(source: &str) -> Option<&'static dyn PlayerStreamResolver> {
    match source.trim() {
        source if source == M6PLAY_RESOLVER.source_id() => Some(&M6PLAY_RESOLVER),
        source if source == RTBF_AUVIO_RESOLVER.source_id() => Some(&RTBF_AUVIO_RESOLVER),
        source if source == RTLPLAY_RESOLVER.source_id() => Some(&RTLPLAY_RESOLVER),
        source if source == TF1_RESOLVER.source_id() => Some(&TF1_RESOLVER),
        source if source == FRANCETV_RESOLVER.source_id() => Some(&FRANCETV_RESOLVER),
        _ => None,
    }
}

fn player_resolver_for_id(resolver_id: &str) -> Option<&'static dyn PlayerStreamResolver> {
    let resolver_id = resolver_id.trim();
    [
        &M6PLAY_RESOLVER as &dyn PlayerStreamResolver,
        &RTBF_AUVIO_RESOLVER,
        &RTLPLAY_RESOLVER,
        &TF1_RESOLVER,
        &FRANCETV_RESOLVER,
    ]
    .into_iter()
    .find(|resolver| resolver.resolver_ids().contains(&resolver_id))
}

/// Adds the original resolver target as `web-link` for generic stream-resolver players.
///
/// Scrapers may emit only a resolver descriptor for an embedded player. Keeping a
/// web link alongside it lets all API consumers open the original player page.
fn populate_stream_resolver_web_links(node: &mut ScraperDataNode) {
    for child in node.children.values_mut() {
        populate_stream_resolver_web_links(child);
    }

    for item in &mut node.items {
        populate_stream_resolver_web_links(item);

        let resolver = item.children.get("resolver");
        let kind = resolver
            .and_then(|resolver| resolver.children.get("kind"))
            .and_then(ScraperDataNode::value_as_string);
        let target_id = resolver
            .and_then(|resolver| resolver.children.get("target_id"))
            .and_then(ScraperDataNode::value_as_string)
            .map(str::to_string);
        let has_web_link = item
            .children
            .get("web-link")
            .and_then(ScraperDataNode::value_as_string)
            .is_some_and(|link| !link.trim().is_empty());

        if kind == Some(GENERIC_STREAM_RESOLVER_ID) && !has_web_link {
            if let Some(target_id) = target_id {
                item.children.insert(
                    "web-link".to_string(),
                    ScraperDataNode::from_values_typed(vec![target_id], ScraperOutputType::String),
                );
            }
        }
    }
}

impl ScraperManager for StreamScraper {
    fn get_scraper_agregator_mut(&mut self) -> &mut ScraperAgregator {
        &mut self.scraper_agregator
    }

    fn create_http_client(&self, http_config: ScraperHttpConfig) -> HttpClient {
        self.scraper_agregator.create_http_client(http_config)
    }

    fn register_service(mut self, controler: &mut dyn ControlerService) {
        self.player_resolver_endpoints.drm_license_public_path =
            controler.stream_public_path(DRM_LICENSE_PROXY_COMMAND);

        let proxy_core = match self.proxy_http_core.clone() {
            Some(proxy_core) => Some(proxy_core),
            None => match ArachneaProxyCore::new(ProxyConfig::default()) {
                Ok(proxy_core) => Some(proxy_core),
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "proxy_http handler not registered: failed to create default proxy core"
                    );
                    None
                }
            },
        };

        if let Some(proxy_core) = proxy_core.as_ref() {
            proxy_core.set_insecure_tls_hosts(
                self.scraper_agregator
                    .group_proxy_insecure_tls_hosts(STREAM_RESOLVER_GROUP_NAME),
            );
            let proxy_public_path = controler.stream_public_path(HTTP_PROXY_COMMAND);
            self.player_resolver_endpoints.http_proxy_public_path = Some(proxy_public_path);
            arachnea_proxy::core::http::register_service(controler, proxy_core, HTTP_PROXY_COMMAND);
        } else {
            tracing::warn!("proxy_http handler not registered: no proxy core available");
        }

        let connector = Arc::new(self);

        controler.register_etag_result_function_with_state(
            "search",
            Arc::clone(&connector),
            |scraper, input: SearchRequest, headers| async move {
                scraper
                    .search(
                        input.query,
                        input.media_types,
                        input.themes,
                        input.page,
                        source_params_from_entries(input.source_params),
                        request_arachnea_etag(input.arachnea_etag, &headers),
                        input.enable_etag,
                    )
                    .await
            },
        );

        controler.register_etag_result_function_with_state(
            "load_home",
            Arc::clone(&connector),
            |scraper, input: LoadHomeRequest, headers| async move {
                scraper
                    .load_home(
                        request_arachnea_etag(input.arachnea_etag, &headers),
                        input.enable_etag,
                    )
                    .await
            },
        );

        controler.register_etag_result_function_with_state(
            "get_service",
            Arc::clone(&connector),
            |scraper, input: GetServiceRequest, headers| async move {
                scraper
                    .get_service_with_etag(
                        request_arachnea_etag(input.arachnea_etag, &headers),
                        input.enable_etag,
                    )
                    .await
            },
        );

        controler.register_etag_result_function_with_state(
            "list_lives",
            Arc::clone(&connector),
            |scraper, input: ListLivesRequest, headers| async move {
                scraper
                    .list_lives(
                        request_arachnea_etag(input.arachnea_etag, &headers),
                        input.enable_etag,
                    )
                    .await
            },
        );

        controler.register_etag_result_function_with_state(
            "get_category",
            Arc::clone(&connector),
            |scraper, input: GetCategoryRequest, headers| async move {
                scraper
                    .get_category(
                        category_sources_from_request(input.source, input.sources),
                        input.page,
                        source_params_from_entries(input.source_params),
                        request_arachnea_etag(input.arachnea_etag, &headers),
                        input.enable_etag,
                    )
                    .await
            },
        );

        controler.register_etag_result_function_with_state(
            "get_section",
            Arc::clone(&connector),
            |scraper, input: GetSectionRequest, headers| async move {
                scraper
                    .get_section(
                        input.source,
                        input.link,
                        input.page,
                        source_params_from_entries(input.source_params),
                        request_arachnea_etag(input.arachnea_etag, &headers),
                        input.enable_etag,
                    )
                    .await
            },
        );

        controler.register_etag_result_function_with_state(
            "get_banners",
            Arc::clone(&connector),
            |scraper, input: GetBannersRequest, headers| async move {
                scraper
                    .get_banners(
                        input.source,
                        input.link,
                        request_arachnea_etag(input.arachnea_etag, &headers),
                        input.enable_etag,
                    )
                    .await
            },
        );

        controler.register_etag_result_function_with_state(
            "get_players",
            Arc::clone(&connector),
            |scraper, input: GetPlayersRequest, headers| async move {
                scraper
                    .get_players(
                        input.source,
                        input.link,
                        request_arachnea_etag(input.arachnea_etag, &headers),
                        input.enable_etag,
                    )
                    .await
            },
        );

        controler.register_etag_result_function_with_state(
            "get_entry",
            Arc::clone(&connector),
            |scraper, input: GetEntryRequest, headers| async move {
                scraper
                    .get_entry(
                        input.source,
                        input.entry,
                        request_arachnea_etag(input.arachnea_etag, &headers),
                        input.enable_etag,
                    )
                    .await
            },
        );

        controler.register_etag_result_function_with_state(
            "get_season",
            Arc::clone(&connector),
            |scraper, input: GetSeasonRequest, headers| async move {
                scraper
                    .get_season(
                        input.source,
                        input.season,
                        input.page,
                        request_arachnea_etag(input.arachnea_etag, &headers),
                        input.enable_etag,
                    )
                    .await
            },
        );

        controler.register_etag_result_function_with_state(
            "get_live",
            Arc::clone(&connector),
            |scraper, input: GetLiveRequest, headers| async move {
                scraper
                    .get_live(
                        input.source,
                        input.channel,
                        request_arachnea_etag(input.arachnea_etag, &headers),
                        input.enable_etag,
                    )
                    .await
            },
        );

        controler.register_result_function_with_state(
            "get_stream",
            Arc::clone(&connector),
            |scraper, input: GetStreamRequest| async move {
                scraper.get_stream(input.resolver, input.target).await
            },
        );

        controler.register_stream_function_with_state(
            DRM_LICENSE_PROXY_COMMAND,
            Arc::clone(&connector),
            |scraper, input| async move { scraper.get_drm_license(input).await },
        );
    }
}

fn source_params_from_entries(entries: Vec<SourceParamsRequestEntry>) -> ScraperSourceParams {
    let mut source_params = ScraperSourceParams::new();

    for entry in entries {
        let source = entry.source.trim().to_string();
        if source.is_empty() {
            continue;
        }

        let params = source_params.entry(source).or_default();
        for (key, value) in entry.params {
            if key == "source" {
                continue;
            }

            if let Some(value) = request_param_value_to_string(value) {
                params.insert(key, value);
            }
        }
    }

    source_params
}

fn request_param_value_to_string(value: Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Array(_) | Value::Object(_) => Some(value.to_string()),
    }
}

#[cfg(test)]
#[path = "stream_scraper_tests.rs"]
mod tests;
