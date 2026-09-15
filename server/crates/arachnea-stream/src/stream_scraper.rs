use anyhow::{bail, Result};
use const_format::concatcp;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf, sync::Arc};
use urlencoding::encode;

use arachnea_core::{
    controler::{
        ControlerService, ControlerStreamInput, ControlerStreamOutput, RequestControlerContext,
        ResponseBody,
    },
    persistence::{
        CredentialsStore, FileCredentialsStore, MemoryEntityStore, PersistenceStoreConfig,
        PersistentEntity, SqliteEntityStore, TypedEntityStore,
    },
};
use arachnea_http::chaser_session::{CachedChaserSession, CLOUDFLARE_SESSION_STORE_NAME};
use arachnea_proxy::core::{ArachneaProxyCore, ProxyConfig, ProxyRecord};
use arachnea_scrapyfy::{scraper_result::ScraperAggregationResult, *};

use crate::reloadable_stream_scraper::ReloadableStreamScraper;
use crate::services::{
    francetv_resolver::FrancetvResolver,
    m6play_resolver::M6PlayResolver,
    player_resolver::{PlayerResolverEndpoints, PlayerStreamResolver},
    rtbf_auvio_resolver::RtbfAuvioResolver,
    rtlplay_resolver::RtlPlayResolver,
    tf1_resolver::Tf1Resolver,
};
use crate::stream_resolver::{
    resolve_scraper_query_stream, ResolvedStream, StreamResolver, GENERIC_STREAM_RESOLVER_ID,
    SCRAPER_QUERY_STREAM_RESOLVER_ID, STREAM_RESOLVER_CONFIG_PATH, STREAM_RESOLVER_GROUP_NAME,
};

/// Default group name used by the stream scraper crate.
pub const STREAM_SERVICE_GROUP_NAME: &str = "arachnea-stream";
/// Persistence namespace containing administrator service activation overrides.
pub const STREAM_SERVICES_STORE_NAME: &str = "arachnea-services";
/// Root directory, relative to the application data directory, of the SQLite
/// persistence stores.
pub const PERSISTENCE_DATA_ROOT: &str = "data/persistence";
/// Marker written after the namespaced service-record schema is initialized.
const SERVICE_STORE_SCHEMA_V2_MARKER: &str = ".source-service-schema-v2";

/// Typed persistence stores composing the application backends.
///
/// Each store owns exactly one entity schema and one SQLite database; the
/// in-memory variant is used by compatibility constructors and tests.
#[derive(Clone)]
pub struct ApplicationStores {
    /// Dynamic proxy inventory cache (`proxy-inventory`).
    pub proxy_inventory: Arc<dyn TypedEntityStore<ProxyRecord>>,
    /// Cached Cloudflare sessions (`cloudflare-session`).
    pub cloudflare_session: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    /// Administrator service records: activation override and encrypted
    /// credentials (`arachnea-services`).
    pub source_enabled: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
}

/// Opens the application SQLite stores under `application_data_path`.
///
/// Each store lives in its own `<root>/<store-name>/records.sqlite3` database;
/// missing columns and indexes are reconciled at open time.
///
/// # Errors
///
/// Returns an error when a database cannot be opened or its physical schema is
/// incompatible with the declared entity schema. The incompatible pre-v2
/// `arachnea-services` schema is deliberately reset once: the store directory
/// (including SQLite WAL/SHM files) is removed before the v2 composite primary
/// key is created. Existing service activation overrides and credentials are
/// therefore lost. Other legacy caches whose recorded schema no longer matches
/// the declared entity must be deleted manually and are intentionally never
/// migrated:
///
/// - the legacy proxy inventory cache uses an incompatible primary key; delete
///   `<application-data>/data/persistence/proxy-inventory/` before opening
///   this version;
/// - the legacy Cloudflare session cache keeps a `session TEXT NOT NULL`
///   column that the flattened `CachedChaserSession` schema no longer writes,
///   so every insert into an existing database fails and legacy rows fail to
///   decode; delete
///   `<application-data>/data/persistence/cloudflare-session/` (including the
///   SQLite WAL/SHM auxiliary files) before starting this version.
pub fn sqlite_application_stores(
    application_data_path: impl Into<PathBuf>,
) -> Result<ApplicationStores> {
    let root = application_data_path.into().join(PERSISTENCE_DATA_ROOT);
    reset_legacy_service_store(&root)?;
    let stores = ApplicationStores {
        proxy_inventory: Arc::new(SqliteEntityStore::new(
            PersistenceStoreConfig::new("proxy-inventory", ProxyRecord::schema())?,
            &root,
        )?),
        cloudflare_session: Arc::new(SqliteEntityStore::new(
            PersistenceStoreConfig::new(
                CLOUDFLARE_SESSION_STORE_NAME,
                CachedChaserSession::schema(),
            )?,
            &root,
        )?),
        source_enabled: Arc::new(SqliteEntityStore::new(
            PersistenceStoreConfig::new(STREAM_SERVICES_STORE_NAME, SourceServiceRecord::schema())?,
            &root,
        )?),
    };
    fs::create_dir_all(root.join(STREAM_SERVICES_STORE_NAME))?;
    fs::write(
        root.join(STREAM_SERVICES_STORE_NAME)
            .join(SERVICE_STORE_SCHEMA_V2_MARKER),
        b"v2",
    )?;
    Ok(stores)
}

/// Removes the pre-v2 service-record database before opening the incompatible
/// composite-key schema. The marker ensures the reset happens only once.
fn reset_legacy_service_store(root: &std::path::Path) -> Result<()> {
    let directory = root.join(STREAM_SERVICES_STORE_NAME);
    if directory.exists() && !directory.join(SERVICE_STORE_SCHEMA_V2_MARKER).exists() {
        fs::remove_dir_all(&directory)?;
    }
    Ok(())
}

/// Builds the in-memory application stores used by compatibility constructors.
///
/// # Errors
///
/// Returns an error when a store configuration is invalid.
pub fn memory_application_stores() -> Result<ApplicationStores> {
    Ok(ApplicationStores {
        proxy_inventory: Arc::new(MemoryEntityStore::new(PersistenceStoreConfig::new(
            "proxy-inventory",
            ProxyRecord::schema(),
        )?)?),
        cloudflare_session: Arc::new(MemoryEntityStore::new(PersistenceStoreConfig::new(
            CLOUDFLARE_SESSION_STORE_NAME,
            CachedChaserSession::schema(),
        )?)?),
        source_enabled: Arc::new(MemoryEntityStore::new(PersistenceStoreConfig::new(
            STREAM_SERVICES_STORE_NAME,
            SourceServiceRecord::schema(),
        )?)?),
    })
}

/// Default path used by the services
pub const DEFAULT_SERVICES_CONFIG_PATH: &str = concatcp!(
    DEFAULT_SERVICES_DIRECTORY,
    "/",
    STREAM_SERVICE_GROUP_NAME,
    "/services.json"
);

const HTTP_PROXY_COMMAND: &str = "proxy";
pub(crate) const DRM_LICENSE_PROXY_COMMAND: &str = "get_drm_license";

static M6PLAY_RESOLVER: M6PlayResolver = M6PlayResolver;
static RTBF_AUVIO_RESOLVER: RtbfAuvioResolver = RtbfAuvioResolver;
static RTLPLAY_RESOLVER: RtlPlayResolver = RtlPlayResolver;
static TF1_RESOLVER: Tf1Resolver = Tf1Resolver;
static FRANCETV_RESOLVER: FrancetvResolver = FrancetvResolver;

/// Creates missing persistent service states without changing existing choices.
async fn synchronize_service_defaults_async(
    store: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
    sources: Vec<ScraperSourceDescriptor>,
) -> Result<()> {
    PersistenceSourceEnabled::with_typed_store(store)
        .register_defaults(&sources)
        .await
}

/// Blocking variant of [`synchronize_service_defaults_async`] for synchronous callers.
fn synchronize_service_defaults(
    store: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
    sources: Vec<ScraperSourceDescriptor>,
) -> Result<()> {
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .map_err(anyhow::Error::from)?
            .block_on(synchronize_service_defaults_async(store, sources))
    })
    .join()
    .map_err(|_| anyhow::anyhow!("service-state synchronization thread panicked"))?
}

fn default_page() -> usize {
    1
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SearchRequest {
    pub(crate) query: String,
    #[serde(default = "default_page")]
    pub(crate) page: usize,
    #[serde(default, alias = "mediaTypes")]
    pub(crate) media_types: Vec<String>,
    #[serde(default)]
    pub(crate) themes: Vec<String>,
    #[serde(default, alias = "sourceParams")]
    pub(crate) source_params: Vec<ScraperSourceParamsRequestEntry>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct GetEntryRequest {
    pub(crate) entry: String,
    pub(crate) source: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct GetSeasonRequest {
    pub(crate) season: String,
    pub(crate) source: String,
    #[serde(default = "default_page")]
    pub(crate) page: usize,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ListLivesRequest {}

#[derive(Serialize, Deserialize)]
pub(crate) struct GetLiveRequest {
    pub(crate) channel: String,
    pub(crate) source: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct GetStreamRequest {
    pub(crate) resolver: String,
    pub(crate) target: String,
    #[serde(default)]
    pub(crate) source: Option<String>,
    #[serde(default, alias = "proxyCountry")]
    pub(crate) proxy_country: Option<String>,
    /// Optional request to rewrite absolute URLs inside proxied HLS manifests.
    #[serde(default, alias = "proxyRewriteManifestUrls")]
    pub(crate) proxy_rewrite_manifest_urls: Option<bool>,
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct LoadHomeRequest {}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct GetServiceRequest {}

#[derive(Serialize, Deserialize)]
pub(crate) struct GetCategoryRequest {
    #[serde(default)]
    pub(crate) source: HashMap<String, String>,
    #[serde(default)]
    pub(crate) sources: Vec<HashMap<String, String>>,
    #[serde(default = "default_page")]
    pub(crate) page: usize,
    #[serde(default, alias = "sourceParams")]
    pub(crate) source_params: Vec<ScraperSourceParamsRequestEntry>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct GetSectionRequest {
    #[serde(default)]
    pub(crate) source: String,
    #[serde(default)]
    pub(crate) link: String,
    #[serde(default = "default_page")]
    pub(crate) page: usize,
    #[serde(default, alias = "sourceParams")]
    pub(crate) source_params: Vec<ScraperSourceParamsRequestEntry>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct GetBannersRequest {
    #[serde(default)]
    pub(crate) source: String,
    #[serde(default)]
    pub(crate) link: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct GetPlayersRequest {
    #[serde(default)]
    pub(crate) source: String,
    #[serde(default)]
    pub(crate) link: String,
}

/// High-level facade exposing scraper operations used by controllers and tests.
pub struct StreamScraper {
    pub(crate) scraper_agregator: Box<ScraperAgregator>,
    pub(crate) credentials_store: Arc<dyn CredentialsStore>,
    pub(crate) stores: ApplicationStores,
    proxy_handle: SharedProxyConfigHandle,
    pub(crate) proxy_http_core: Option<ArachneaProxyCore>,
    pub(crate) player_resolver_endpoints: PlayerResolverEndpoints,
}

/// Options controlling how a [`StreamScraper`] instance is built or rebuilt.
///
/// Cloning the options is cheap: the stores are shared through `Arc`.
#[derive(Clone)]
pub struct StreamScraperBuildOptions {
    /// Services manifest path, relative to application resources when not absolute.
    pub services_config_path: String,
    /// Shared credentials persistence used by service resolvers.
    pub credentials_store: Arc<dyn CredentialsStore>,
    /// Typed persistence stores used by activation overrides, HTTP clients, and
    /// the proxy inventory.
    pub stores: ApplicationStores,
    /// Optional explicit local country used for geo proxy decisions.
    pub current_country: Option<String>,
}

impl StreamScraperBuildOptions {
    /// Creates build options with the default services manifest path.
    ///
    /// # Arguments
    /// * `credentials_store` - Shared credentials persistence used by service resolvers.
    /// * `stores` - Typed persistence stores shared by the application.
    pub fn new(credentials_store: Arc<dyn CredentialsStore>, stores: ApplicationStores) -> Self {
        Self {
            services_config_path: DEFAULT_SERVICES_CONFIG_PATH.to_string(),
            credentials_store,
            stores,
            current_country: None,
        }
    }

    /// Overrides the services manifest path.
    pub fn with_services_config_path(mut self, path: impl Into<String>) -> Self {
        self.services_config_path = path.into();
        self
    }
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
        Self::new_with_typed_stores(
            credentials_store,
            memory_application_stores().expect("in-memory application stores are valid"),
        )
    }

    /// Creates a scraper facade backed by the provided credentials store and
    /// typed persistence stores.
    ///
    /// # Arguments
    ///
    /// * `credentials_store` - Shared credentials persistence used by service resolvers.
    /// * `stores` - Typed persistence stores used by activation overrides, HTTP
    ///   clients, and the proxy inventory.
    ///
    /// # Returns
    ///
    /// A configured scraper facade.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration file cannot be loaded or parsed.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_typed_stores(
        credentials_store: Arc<dyn CredentialsStore>,
        stores: ApplicationStores,
    ) -> Self {
        let mut agregator = Box::new(ScraperAgregator::new_with_typed_stores(
            Arc::clone(&stores.proxy_inventory),
            Arc::clone(&stores.cloudflare_session),
        ));
        agregator.set_source_enabled(Arc::new(PersistenceSourceEnabled::with_typed_store(
            Arc::clone(&stores.source_enabled),
        )));
        agregator.ensure_proxy_core();
        let proxy_handle = agregator.get_proxy_handle();
        let proxy_http_core = agregator.proxy_core().cloned();

        StreamScraper {
            scraper_agregator: agregator,
            credentials_store,
            stores,
            proxy_handle,
            proxy_http_core,
            player_resolver_endpoints: PlayerResolverEndpoints::default(),
        }
    }

    /// Creates a scraper facade from explicit build options.
    ///
    /// The built instance shares the provided persistence store, registers the
    /// persistent activation overrides handler under `arachnea-services`,
    /// synchronizes missing service states, loads the stream and stream-resolver
    /// source groups, and applies the optional cache configuration.
    ///
    /// # Arguments
    /// * `options` - Build options describing stores, manifest path, and sizing.
    ///
    /// # Returns
    /// A configured scraper facade with every declared source loaded.
    ///
    /// # Errors
    /// Returns an error when the catalog or one of the source files cannot be
    /// loaded or parsed, or when service-state synchronization fails.
    pub fn from_options(options: &StreamScraperBuildOptions) -> Result<Self> {
        Self::from_options_with_activation_sync(options, true)
    }

    /// Creates a scraper facade from build options whose service group was
    /// already validated by the administration reload coordinator.
    ///
    /// This constructor still loads the configured query collections for the
    /// replacement instance, but it does not read or write source activation
    /// defaults. The coordinator owns that generic validation and persistence
    /// work before asking the Stream runtime to rebuild.
    ///
    /// # Arguments
    ///
    /// * `options` - Build options describing stores, manifest path, and sizing.
    ///
    /// # Errors
    ///
    /// Returns an error when a configured source file cannot be loaded or
    /// parsed.
    pub(crate) fn from_validated_options(options: &StreamScraperBuildOptions) -> Result<Self> {
        Self::from_options_with_activation_sync(options, false)
    }

    /// Creates a scraper facade with optional activation-default synchronization.
    fn from_options_with_activation_sync(
        options: &StreamScraperBuildOptions,
        synchronize_activation_defaults: bool,
    ) -> Result<Self> {
        let mut instance = Self::new_with_typed_stores(
            Arc::clone(&options.credentials_store),
            options.stores.clone(),
        );
        let services_path = options.services_config_path.as_str();
        if synchronize_activation_defaults {
            let catalog = load_service_catalog(services_path, STREAM_SERVICE_GROUP_NAME)?;
            synchronize_service_defaults(
                Arc::clone(&instance.stores.source_enabled),
                catalog.iter().map(|entry| entry.source.clone()).collect(),
            )?;
        }

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

    /// Creates a instance of the scraper facade using a file-based credentials store and the default services configuration.
    ///
    /// The typed persistence stores are SQLite databases under the default
    /// application data directory.
    ///
    /// # Returns
    /// A configured scraper facade with default configuration.
    ///
    /// # Errors
    /// Returns an error if the default configuration file cannot be loaded or
    /// parsed, or if the persistence stores cannot be opened.
    pub fn from_json(json_path: Option<&str>) -> Result<Self> {
        let options = StreamScraperBuildOptions::new(
            FileCredentialsStore::default().as_arc(),
            sqlite_application_stores(&arachnea_core::application::get_application_data_path(""))?,
        )
        .with_services_config_path(json_path.unwrap_or(DEFAULT_SERVICES_CONFIG_PATH));
        Self::from_options(&options)
    }

    /// Loads the complete service catalog, including sources disabled by default.
    pub fn load_service_catalog(
        &self,
        json_path: Option<&str>,
    ) -> Result<Vec<ScraperServiceCatalogEntry>> {
        load_service_catalog(
            json_path.unwrap_or(DEFAULT_SERVICES_CONFIG_PATH),
            STREAM_SERVICE_GROUP_NAME,
        )
    }

    /// Returns the mutable proxy handle shared by this scraper instance.
    pub fn proxy_handle(&self) -> SharedProxyConfigHandle {
        self.proxy_handle.clone()
    }

    /// Returns the query aggregator backing this scraper instance.
    pub fn scraper_agregator(&self) -> &ScraperAgregator {
        &self.scraper_agregator
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

    pub(crate) async fn get_stream(
        &self,
        resolver_id: String,
        target: String,
        source: Option<String>,
        proxy_country: Option<String>,
        proxy_rewrite_manifest_urls: Option<bool>,
    ) -> Result<ScraperAggregationResult<ResolvedStream>> {
        let resolved = if resolver_id.trim() == SCRAPER_QUERY_STREAM_RESOLVER_ID {
            ResolvedStream::Stream(
                resolve_scraper_query_stream(
                    &self.scraper_agregator,
                    &self.player_resolver_endpoints,
                    source.as_deref().unwrap_or_default(),
                    &target,
                    proxy_country.as_deref(),
                    proxy_rewrite_manifest_urls.unwrap_or(false),
                )
                .await?,
            )
        } else if resolver_id.trim() == GENERIC_STREAM_RESOLVER_ID {
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

    pub(crate) async fn get_drm_license(
        &self,
        input: ControlerStreamInput,
    ) -> Result<ControlerStreamOutput> {
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

    /// Prepares browser-facing endpoints and registers the shared proxy command.
    ///
    /// Must be called once before route registration. Reloaded instances reuse
    /// the endpoints captured at registration time through
    /// [`StreamScraper::apply_registration_endpoints`].
    ///
    /// # Arguments
    /// * `controler` - Controller used to resolve public stream command paths.
    pub(crate) fn prepare_registration(&mut self, controler: &mut dyn ControlerService) {
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

        self.proxy_http_core = proxy_core;
    }

    /// Applies registration-time endpoints to a rebuilt instance so reloaded
    /// scrapers keep serving the same public proxy and DRM license paths.
    ///
    /// # Arguments
    /// * `endpoints` - Endpoints captured when routes were first registered.
    pub(crate) fn apply_registration_endpoints(
        &mut self,
        endpoints: &crate::reloadable_stream_scraper::RegistrationEndpoints,
    ) {
        self.player_resolver_endpoints.drm_license_public_path =
            endpoints.drm_license_public_path.clone();
        self.player_resolver_endpoints.http_proxy_public_path =
            endpoints.http_proxy_public_path.clone();

        if let Some(proxy_core) = &endpoints.proxy_core {
            self.scraper_agregator.set_proxy_core(proxy_core.clone());
            proxy_core.set_insecure_tls_hosts(
                self.scraper_agregator
                    .group_proxy_insecure_tls_hosts(STREAM_RESOLVER_GROUP_NAME),
            );
            self.proxy_http_core = Some(proxy_core.clone());
        }
    }
}

/// Groups the category filters sent by category requests.
pub(crate) fn category_sources_from_request(
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

    fn register_service(self, controler: &mut dyn ControlerService) {
        let options = StreamScraperBuildOptions {
            services_config_path: DEFAULT_SERVICES_CONFIG_PATH.to_string(),
            credentials_store: Arc::clone(&self.credentials_store),
            stores: self.stores.clone(),
            current_country: None,
        };
        ReloadableStreamScraper::from_instance(self, options).register_service(controler);
    }
}

#[cfg(test)]
#[path = "stream_scraper_tests.rs"]
mod tests;
