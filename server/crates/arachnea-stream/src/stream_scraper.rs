use anyhow::{bail, Result};
use const_format::concatcp;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use urlencoding::encode;

use arachnea_core::{
    controler::{
        ControlerService, ControlerServiceExt, ControlerStreamInput, ControlerStreamOutput,
        RequestControlerContext, ResponseBody,
    },
    persistence::{CredentialsStore, FileCredentialsStore, FilePersistenceStore, PersistenceStore},
};
use arachnea_proxy::core::{ArachneaProxyCore, ProxyConfig};
use arachnea_scrapyfy::{scraper_result::ScraperAggregationResult, *};

use crate::services::{
    francetv_resolver::FrancetvResolver,
    m6play_resolver::M6PlayResolver,
    player_resolver::{PlayerResolverEndpoints, PlayerStreamResolver},
    rtbf_auvio_resolver::RtbfAuvioResolver,
    rtlplay_resolver::RtlPlayResolver,
    tf1_resolver::Tf1Resolver,
};
use crate::stream_resolver::{
    ResolvedStream, StreamResolver, GENERIC_STREAM_RESOLVER_ID, STREAM_RESOLVER_CONFIG_PATH,
    STREAM_RESOLVER_GROUP_NAME,
};

/// Default group name used by the stream scraper crate.
pub const STREAM_SERVICE_GROUP_NAME: &str = "arachnea-stream";
/// Persistence namespace containing administrator service activation overrides.
pub const STREAM_SERVICES_STORE_NAME: &str = "arachnea-services";

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

/// Creates missing persistent service states without changing existing choices.
fn synchronize_service_defaults(
    store: Arc<dyn PersistenceStore>,
    sources: Vec<ScraperSourceDescriptor>,
) -> Result<()> {
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .map_err(anyhow::Error::from)?
            .block_on(async move {
                PersistenceSourceEnabled::new(store, STREAM_SERVICES_STORE_NAME)
                    .register_defaults(&sources)
                    .await
            })
    })
    .join()
    .map_err(|_| anyhow::anyhow!("service-state synchronization thread panicked"))?
}

fn default_page() -> usize {
    1
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
    source_params: Vec<ScraperSourceParamsRequestEntry>,
}

#[derive(Serialize, Deserialize)]
struct GetEntryRequest {
    entry: String,
    source: String,
}

#[derive(Serialize, Deserialize)]
struct GetSeasonRequest {
    season: String,
    source: String,
    #[serde(default = "default_page")]
    page: usize,
}

#[derive(Serialize, Deserialize)]
struct ListLivesRequest {}

#[derive(Serialize, Deserialize)]
struct GetLiveRequest {
    channel: String,
    source: String,
}

#[derive(Serialize, Deserialize)]
struct GetStreamRequest {
    resolver: String,
    target: String,
}

#[derive(Default, Serialize, Deserialize)]
struct LoadHomeRequest {}

#[derive(Default, Serialize, Deserialize)]
struct GetServiceRequest {}

#[derive(Serialize, Deserialize)]
struct GetCategoryRequest {
    #[serde(default)]
    source: HashMap<String, String>,
    #[serde(default)]
    sources: Vec<HashMap<String, String>>,
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default, alias = "sourceParams")]
    source_params: Vec<ScraperSourceParamsRequestEntry>,
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
    source_params: Vec<ScraperSourceParamsRequestEntry>,
}

#[derive(Serialize, Deserialize)]
struct GetBannersRequest {
    #[serde(default)]
    source: String,
    #[serde(default)]
    link: String,
}

#[derive(Serialize, Deserialize)]
struct GetPlayersRequest {
    #[serde(default)]
    source: String,
    #[serde(default)]
    link: String,
}

/// High-level facade exposing scraper operations used by controllers and tests.
pub struct StreamScraper {
    pub(crate) scraper_agregator: Box<ScraperAgregator>,
    credentials_store: Arc<dyn CredentialsStore>,
    persistence_store: Arc<dyn PersistenceStore>,
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
        agregator.set_source_enabled(Arc::new(PersistenceSourceEnabled::new(
            persistence_store.clone(),
            STREAM_SERVICES_STORE_NAME,
        )));
        agregator.ensure_proxy_core();
        let proxy_handle = agregator.get_proxy_handle();
        let proxy_http_core = agregator.proxy_core().cloned();

        StreamScraper {
            scraper_agregator: agregator,
            credentials_store,
            persistence_store,
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
        let services_path = json_path.unwrap_or(DEFAULT_SERVICES_CONFIG_PATH);
        let catalog = load_service_catalog(services_path)?;
        synchronize_service_defaults(
            Arc::clone(&instance.persistence_store),
            catalog.iter().map(|entry| entry.source.clone()).collect(),
        )?;

        instance
            .scraper_agregator
            .add_query_collection_from_config_json(STREAM_SERVICE_GROUP_NAME, services_path)?;
        instance
            .scraper_agregator
            .add_query_collection_from_config_json(
                STREAM_RESOLVER_GROUP_NAME,
                STREAM_RESOLVER_CONFIG_PATH,
            )?;
        instance.configure_proxy_insecure_tls_hosts();

        Ok(instance)
    }

    /// Loads the complete service catalog, including sources disabled by default.
    pub fn load_service_catalog(
        &self,
        json_path: Option<&str>,
    ) -> Result<Vec<ScraperServiceCatalogEntry>> {
        load_service_catalog(json_path.unwrap_or(DEFAULT_SERVICES_CONFIG_PATH))
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

    /// Executes the `search` query using URL-encoded terms.
    ///
    /// # Arguments
    ///
    /// * `query` - Raw search string that will be URL-encoded before execution.
    /// * `media_types` - Optional media type filters applied to query selection and result filtering.
    /// * `themes` - Optional theme filters applied to scraped results.
    /// * `page` - 1-based page number requested by the caller.
    /// * `source_params` - Source-specific runtime parameters returned by the previous page.
    /// * `context` - Context of the incoming controller request.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn search(
        &self,
        context: RequestControlerContext,
        query: String,
        media_types: Vec<String>,
        themes: Vec<String>,
        page: usize,
        source_params: ScraperSourceParams,
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

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::ClientCache),
                STREAM_SERVICE_GROUP_NAME,
                "search",
                &params,
                Some(&source_params),
                scrapper_list.as_ref(),
                media,
                None,
                None,
                "search",
            )
            .await;
        let global_etag = result.take_global_etag();
        Ok((result, global_etag))
    }

    /// Convenience helper for the `get_entry` query using the provided absolute entry URL.
    ///
    /// # Arguments
    ///
    /// * `query_source` - Query source name.
    /// * `query_url` - Absolute URL of the entry page to fetch.
    /// * `context` - Context of the incoming controller request.
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
        context: RequestControlerContext,
        query_source: String,
        query_url: String,
    ) -> Result<(
        ScraperAggregationResult<HashMap<String, ScraperDataNode>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("query_url".to_string(), query_url);
        self.enrich_runtime_params(&mut params);

        let scrapper_list = vec![query_source];

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::ClientCache),
                STREAM_SERVICE_GROUP_NAME,
                "get_entry",
                &params,
                None,
                Some(&scrapper_list),
                None,
                None,
                None,
                "get_entry",
            )
            .await;
        let global_etag = result.take_global_etag();

        let mut root = ScraperDataNode {
            children: result.data.into_iter().next().unwrap_or_default(),
            ..Default::default()
        };
        populate_stream_resolver_web_links(&mut root);

        Ok((
            ScraperAggregationResult::new(root.children, result.errors)
                .with_validations(result.validations)
                .with_global_etag(global_etag.clone()),
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
    /// * `context` - Context of the incoming controller request.
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
        context: RequestControlerContext,
        query_source: String,
        query_url: String,
        page: usize,
    ) -> Result<(
        ScraperAggregationResult<HashMap<String, ScraperDataNode>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("query_url".to_string(), query_url);
        params.insert("page".to_string(), page.max(1).to_string());
        self.enrich_runtime_params(&mut params);

        let scrapper_list = vec![query_source];

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::ClientCache),
                STREAM_SERVICE_GROUP_NAME,
                "get_season",
                &params,
                None,
                Some(&scrapper_list),
                None,
                None,
                None,
                "get_season",
            )
            .await;
        let global_etag = result.take_global_etag();

        Ok((
            ScraperAggregationResult::new(
                result.data.into_iter().next().unwrap_or_default(),
                result.errors,
            )
            .with_validations(result.validations)
            .with_global_etag(global_etag.clone()),
            global_etag,
        ))
    }

    /// Loads the aggregated live catalog across every configured source.
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn list_lives(
        &self,
        context: RequestControlerContext,
    ) -> Result<(
        ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        self.enrich_runtime_params(&mut params);
        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::FullCache),
                STREAM_SERVICE_GROUP_NAME,
                "list_lives",
                &params,
                None,
                None,
                None,
                None,
                Some("source"),
                "list_lives",
            )
            .await;
        let global_etag = result.take_global_etag();
        Ok((result, global_etag))
    }

    /// Loads one live payload for the provided source and channel identifier.
    ///
    /// # Arguments
    ///
    /// * `query_source` - Query source name.
    /// * `channel` - Source-specific live channel identifier.
    /// * `context` - Context of the incoming controller request.
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
        context: RequestControlerContext,
        query_source: String,
        channel: String,
    ) -> Result<(
        ScraperAggregationResult<HashMap<String, ScraperDataNode>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("channel".to_string(), channel);
        self.enrich_runtime_params(&mut params);

        let scrapper_list = vec![query_source];

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::default(),
                STREAM_SERVICE_GROUP_NAME,
                "get_live",
                &params,
                None,
                Some(&scrapper_list),
                None,
                None,
                None,
                "get_live",
            )
            .await;
        let global_etag = result.take_global_etag();

        Ok((
            ScraperAggregationResult::new(
                result.data.into_iter().next().unwrap_or_default(),
                result.errors,
            )
            .with_validations(result.validations)
            .with_global_etag(global_etag.clone()),
            global_etag,
        ))
    }

    /// Loads the aggregated home catalog across every configured source.
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn load_home(
        &self,
        context: RequestControlerContext,
    ) -> Result<(
        ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        self.enrich_runtime_params(&mut params);
        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::FullCache),
                STREAM_SERVICE_GROUP_NAME,
                "load_home",
                &params,
                None,
                None,
                None,
                None,
                Some("source"),
                "load_home",
            )
            .await;
        let global_etag = result.take_global_etag();
        Ok((result, global_etag))
    }

    /// Loads display metadata for every configured source.
    ///
    /// # Arguments
    ///
    /// * `context` - Context of the incoming controller request.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn get_service(
        &self,
        context: RequestControlerContext,
    ) -> Result<(
        ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>>,
        Option<String>,
    )> {
        let mut params: HashMap<String, String> = HashMap::new();
        self.enrich_runtime_params(&mut params);

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::NoCache),
                STREAM_SERVICE_GROUP_NAME,
                "service_stream_metadata",
                &params,
                None,
                None,
                None,
                None,
                None,
                "service_stream_metadata",
            )
            .await;
        let global_etag = result.take_global_etag();
        Ok((result, global_etag))
    }

    /// Loads category payloads for the provided YAML-defined source descriptors.
    ///
    /// # Arguments
    ///
    /// * `sources` - Source descriptors returned under `category.sources` by `load_home`.
    /// * `page` - 1-based page number requested from the backend source.
    /// * `source_params` - Optional per-source runtime parameters overriding global values.
    /// * `context` - Context of the incoming controller request.
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
        context: RequestControlerContext,
        sources: Vec<HashMap<String, String>>,
        page: usize,
        source_params: ScraperSourceParams,
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

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::FullCache),
                STREAM_SERVICE_GROUP_NAME,
                "get_category",
                &params,
                Some(&source_params),
                Some(&scrapper_list),
                None,
                None,
                Some("source"),
                "get_category",
            )
            .await;
        let global_etag = result.take_global_etag();
        Ok((result, global_etag))
    }

    /// Loads one paged section payload for the provided source and section link.
    ///
    /// # Arguments
    ///
    /// * `query_source` - Query source name.
    /// * `query_url` - Source-specific section URL or API endpoint.
    /// * `page` - 1-based page number requested from the backend source.
    /// * `source_params` - Optional per-source runtime parameters overriding global values.
    /// * `context` - Context of the incoming controller request.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn get_section(
        &self,
        context: RequestControlerContext,
        query_source: String,
        query_url: String,
        page: usize,
        mut source_params: ScraperSourceParams,
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

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::FullCache),
                STREAM_SERVICE_GROUP_NAME,
                "get_section",
                &params,
                Some(&source_params),
                scrapper_list.as_ref(),
                None,
                None,
                Some("source"),
                "get_section",
            )
            .await;
        let global_etag = result.take_global_etag();

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
                .with_validations(result.validations)
                .with_global_etag(global_etag.clone()),
            global_etag,
        ))
    }

    /// Loads banners for the provided source and banner link.
    ///
    /// # Arguments
    ///
    /// * `query_source` - Query source name.
    /// * `query_url` - Source-specific banner URL or API endpoint.
    /// * `context` - Context of the incoming controller request.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn get_banners(
        &self,
        context: RequestControlerContext,
        query_source: String,
        query_url: String,
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

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::ClientCache),
                STREAM_SERVICE_GROUP_NAME,
                "get_banners",
                &params,
                None,
                scrapper_list.as_ref(),
                None,
                None,
                Some("source"),
                "get_banners",
            )
            .await;
        let global_etag = result.take_global_etag();

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
                .with_validations(result.validations)
                .with_global_etag(global_etag.clone()),
            global_etag,
        ))
    }

    /// Loads players for the provided source and player link.
    ///
    /// # Arguments
    ///
    /// * `query_source` - Query source name.
    /// * `query_url` - Source-specific player URL or API endpoint.
    /// * `context` - Context of the incoming controller request.
    ///
    /// # Returns
    ///
    /// The aggregation result alongside the up-to-date global ETag.
    pub async fn get_players(
        &self,
        context: RequestControlerContext,
        query_source: String,
        query_url: String,
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

        let mut result = self
            .scraper_agregator
            .execute_query_async(
                &context,
                QueryParameters::from_cache_type(CacheType::ClientCache),
                STREAM_SERVICE_GROUP_NAME,
                "get_players",
                &params,
                None,
                scrapper_list.as_ref(),
                None,
                None,
                Some("source"),
                "get_players",
            )
            .await;
        let global_etag = result.take_global_etag();

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
                .with_validations(result.validations)
                .with_global_etag(global_etag.clone()),
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

        controler.register_result_function_with_state(
            "search",
            Arc::clone(&connector),
            |scraper, context, input: SearchRequest| async move {
                scraper
                    .search(
                        context,
                        input.query,
                        input.media_types,
                        input.themes,
                        input.page,
                        source_params_from_entries(input.source_params),
                    )
                    .await
            },
        );

        controler.register_result_function_with_state(
            "load_home",
            Arc::clone(&connector),
            |scraper, context, _input: LoadHomeRequest| async move {
                scraper.load_home(context).await
            },
        );

        controler.register_result_function_with_state(
            "get_service",
            Arc::clone(&connector),
            |scraper, context, _input: GetServiceRequest| async move {
                scraper.get_service(context).await
            },
        );

        controler.register_result_function_with_state(
            "list_lives",
            Arc::clone(&connector),
            |scraper, context, _input: ListLivesRequest| async move {
                scraper.list_lives(context).await
            },
        );

        controler.register_result_function_with_state(
            "get_category",
            Arc::clone(&connector),
            |scraper, context, input: GetCategoryRequest| async move {
                scraper
                    .get_category(
                        context,
                        category_sources_from_request(input.source, input.sources),
                        input.page,
                        source_params_from_entries(input.source_params),
                    )
                    .await
            },
        );

        controler.register_result_function_with_state(
            "get_section",
            Arc::clone(&connector),
            |scraper, context, input: GetSectionRequest| async move {
                scraper
                    .get_section(
                        context,
                        input.source,
                        input.link,
                        input.page,
                        source_params_from_entries(input.source_params),
                    )
                    .await
            },
        );

        controler.register_result_function_with_state(
            "get_banners",
            Arc::clone(&connector),
            |scraper, context, input: GetBannersRequest| async move {
                scraper.get_banners(context, input.source, input.link).await
            },
        );

        controler.register_result_function_with_state(
            "get_players",
            Arc::clone(&connector),
            |scraper, context, input: GetPlayersRequest| async move {
                scraper.get_players(context, input.source, input.link).await
            },
        );

        controler.register_result_function_with_state(
            "get_entry",
            Arc::clone(&connector),
            |scraper, context, input: GetEntryRequest| async move {
                scraper.get_entry(context, input.source, input.entry).await
            },
        );

        controler.register_result_function_with_state(
            "get_season",
            Arc::clone(&connector),
            |scraper, context, input: GetSeasonRequest| async move {
                scraper
                    .get_season(context, input.source, input.season, input.page)
                    .await
            },
        );

        controler.register_result_function_with_state(
            "get_live",
            Arc::clone(&connector),
            |scraper, context, input: GetLiveRequest| async move {
                scraper.get_live(context, input.source, input.channel).await
            },
        );

        controler.register_result_function_with_state(
            "get_stream",
            Arc::clone(&connector),
            |scraper, _context, input: GetStreamRequest| async move {
                scraper
                    .get_stream(input.resolver, input.target)
                    .await
                    .map(|result| (result, None::<String>))
            },
        );

        controler.register_stream_function_with_state(
            DRM_LICENSE_PROXY_COMMAND,
            Arc::clone(&connector),
            |scraper, input| async move { scraper.get_drm_license(input).await },
        );
    }
}

#[cfg(test)]
#[path = "stream_scraper_tests.rs"]
mod tests;
