use anyhow::{bail, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, LazyLock},
};
use urlencoding::encode;

use arachnea_core::{
    controler::{
        ControlerService, ControlerStreamInput, ControlerStreamOutput, RequestControlerContext,
        ResponseBody,
    },
    persistence::{
        CredentialsStore, FileCredentialsStore, MemoryEntityStore, PersistenceStoreConfig,
        PersistenceTableConfig, PersistentEntity, SqliteDatabase, SqliteStoreConfig,
        TypedEntityStore,
    },
};
use arachnea_http::chaser_session::{CachedChaserSession, CLOUDFLARE_SESSION_STORE_NAME};
use arachnea_proxy::core::{
    ArachneaProxyCore, ProxyConfig, ProxyDestinationPreference, ProxyRecord,
    TypedProxyPreferenceRepository, TypedProxyRepository, PROXY_PREFERENCE_STORE_NAME,
    PROXY_STORE_NAME,
};
use arachnea_scrapyfy::{
    scraper_result::{ScraperAggregationResult, ScraperErrorOrigin, ScraperExecutionError},
    *,
};

static HOME_ERROR_CODES: LazyLock<arachnea_core::error_code::ErrorCodeGenerator> =
    LazyLock::new(arachnea_core::error_code::ErrorCodeGenerator::new);

use crate::reloadable_stream_scraper::ReloadableStreamScraper;
use crate::services::{
    antennereunion_resolver::AntenneReunionResolver,
    francetv_resolver::FrancetvResolver,
    m6play_resolver::M6PlayResolver,
    player_resolver::{PlayerResolverEndpoints, PlayerStreamResolver},
    rmcbfmplay_resolver::RmcBfmPlayResolver,
    rtbf_auvio_resolver::RtbfAuvioResolver,
    rtlplay_resolver::RtlPlayResolver,
    tf1_resolver::Tf1Resolver,
    tv5mondeplus_resolver::Tv5mondeplusResolver,
};
use crate::stream_resolver::{
    resolve_scraper_query_stream, ResolvedStream, StreamResolver, GENERIC_STREAM_RESOLVER_ID,
    SCRAPER_QUERY_STREAM_RESOLVER_ID, STREAM_RESOLVER_CONFIG_PATH, STREAM_RESOLVER_GROUP_NAME,
};

pub use crate::STREAM_SERVICE_GROUP_NAME;

pub use crate::STREAM_SERVICES_STORE_NAME;

pub use crate::PERSISTENCE_DATA_ROOT;

/// Typed persistence stores composing the application backends.
///
/// Each handle represents one entity table. The three proxy tables share a
/// physical SQLite database; compatibility constructors use in-memory tables.
#[derive(Clone)]
pub struct ApplicationStores {
    /// Dynamic proxy inventory cache (`proxy-inventory`).
    pub proxy_inventory: Arc<dyn TypedEntityStore<ProxyRecord>>,
    /// Durable bounded rediscovery protection for evicted endpoints.
    pub proxy_failures: Arc<dyn TypedEntityStore<ProxyRecord>>,
    /// Durable accepted histories independent of the request country filter.
    pub proxy_preferences: Arc<dyn TypedEntityStore<ProxyDestinationPreference>>,
    /// Cached Cloudflare sessions (`cloudflare-session`).
    pub cloudflare_session: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    /// Administrator service records: activation override and encrypted
    /// credentials (`arachnea-services`).
    pub source_enabled: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
}

pub use crate::DEFAULT_SERVICES_CONFIG_PATH;

use crate::STREAM_HTTP_PROXY_COMMAND as HTTP_PROXY_COMMAND;

pub(crate) use crate::STREAM_DRM_LICENSE_PROXY_COMMAND as DRM_LICENSE_PROXY_COMMAND;

static ANTENNE_REUNION_RESOLVER: AntenneReunionResolver = AntenneReunionResolver;
static M6PLAY_RESOLVER: M6PlayResolver = M6PlayResolver;
static RMCBFMPLAY_RESOLVER: RmcBfmPlayResolver = RmcBfmPlayResolver;
static RTBF_AUVIO_RESOLVER: RtbfAuvioResolver = RtbfAuvioResolver;
static RTLPLAY_RESOLVER: RtlPlayResolver = RtlPlayResolver;
static TF1_RESOLVER: Tf1Resolver = Tf1Resolver;
static FRANCETV_RESOLVER: FrancetvResolver = FrancetvResolver;
static TV5MONDEPLUS_RESOLVER: Tv5mondeplusResolver = Tv5mondeplusResolver;

fn default_page() -> usize {
    crate::STREAM_DEFAULT_PAGE
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
    #[serde(default, alias = "proxyCountries")]
    pub(crate) proxy_countries: Vec<String>,
    /// Optional request to rewrite absolute URLs inside proxied HLS manifests.
    #[serde(default, alias = "proxyRewriteManifestUrls")]
    pub(crate) proxy_rewrite_manifest_urls: Option<bool>,
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct LoadHomeRequest {
    /// Exact source identifiers; omitted means all, an empty list means none.
    #[serde(
        default,
        deserialize_with = "deserialize_home_sources",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) sources: Option<Vec<String>>,
}

/// Rejects explicit null, invalid types and blank identifiers without treating them as all sources.
fn deserialize_home_sources<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let sources = Vec::<String>::deserialize(deserializer)?;
    if sources.iter().any(|source| source.trim().is_empty()) {
        return Err(serde::de::Error::custom(
            "load_home sources must contain non-blank identifiers",
        ));
    }
    Ok(Some(sources))
}

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
pub(crate) struct GetRecommendationsRequest {
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
    /// Shared loopback listener retained across validated scraper rebuilds.
    pub proxy_runtime: Arc<arachnea_proxy::server::LoopbackProxyRuntime>,
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
            proxy_runtime: Arc::default(),
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
        Self::new_with_stores_and_proxy_runtime(credentials_store, stores, Arc::default())
    }

    fn new_with_stores_and_proxy_runtime(
        credentials_store: Arc<dyn CredentialsStore>,
        stores: ApplicationStores,
        proxy_runtime: Arc<arachnea_proxy::server::LoopbackProxyRuntime>,
    ) -> Self {
        let mut agregator = Box::new(ScraperAgregator::new_with_typed_stores_and_proxy_handle(
            SharedProxyConfigHandle::with_runtime(proxy_runtime),
            Arc::clone(&stores.proxy_inventory),
            Arc::clone(&stores.cloudflare_session),
        ));
        agregator.set_proxy_repositories(
            Arc::new(TypedProxyRepository::with_failure_store(
                stores.proxy_inventory.clone(),
                stores.proxy_failures.clone(),
            )),
            Arc::new(TypedProxyPreferenceRepository::new(
                stores.proxy_preferences.clone(),
            )),
        );
        agregator.set_source_enabled(Arc::new(PersistenceSourceEnabled::with_typed_store(
            Arc::clone(&stores.source_enabled),
        )));
        agregator.ensure_proxy_core();
        if let Some(core) = agregator.proxy_core().cloned() {
            let core = core.with_fallback_direct(true);
            agregator
                .get_proxy_handle()
                .set_proxy(HttpProxyConfig::Arachnea(core.clone()));
            agregator.set_proxy_core(core);
        }
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
        let mut instance = Self::new_with_stores_and_proxy_runtime(
            Arc::clone(&options.credentials_store),
            options.stores.clone(),
            options.proxy_runtime.clone(),
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
}

fn normalize_proxy_countries(proxy_countries: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for country in proxy_countries {
        let country = country.trim().to_ascii_uppercase();
        if country.len() == 2
            && country.bytes().all(|byte| byte.is_ascii_alphabetic())
            && !normalized.contains(&country)
        {
            normalized.push(country);
        }
    }
    normalized
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
        source if source == ANTENNE_REUNION_RESOLVER.source_id() => Some(&ANTENNE_REUNION_RESOLVER),
        source if source == M6PLAY_RESOLVER.source_id() => Some(&M6PLAY_RESOLVER),
        source if source == RMCBFMPLAY_RESOLVER.source_id() => Some(&RMCBFMPLAY_RESOLVER),
        source if source == RTBF_AUVIO_RESOLVER.source_id() => Some(&RTBF_AUVIO_RESOLVER),
        source if source == RTLPLAY_RESOLVER.source_id() => Some(&RTLPLAY_RESOLVER),
        source if source == TF1_RESOLVER.source_id() => Some(&TF1_RESOLVER),
        source if source == FRANCETV_RESOLVER.source_id() => Some(&FRANCETV_RESOLVER),
        source if source == TV5MONDEPLUS_RESOLVER.source_id() => Some(&TV5MONDEPLUS_RESOLVER),
        _ => None,
    }
}

fn player_resolver_for_id(resolver_id: &str) -> Option<&'static dyn PlayerStreamResolver> {
    let resolver_id = resolver_id.trim();
    [
        &ANTENNE_REUNION_RESOLVER as &dyn PlayerStreamResolver,
        &M6PLAY_RESOLVER as &dyn PlayerStreamResolver,
        &RMCBFMPLAY_RESOLVER,
        &RTBF_AUVIO_RESOLVER,
        &RTLPLAY_RESOLVER,
        &TF1_RESOLVER,
        &FRANCETV_RESOLVER,
        &TV5MONDEPLUS_RESOLVER,
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
            proxy_runtime: self.proxy_handle.proxy_runtime(),
        };
        ReloadableStreamScraper::from_instance(self, options).register_service(controler);
    }
}

#[cfg(test)]
#[path = "../tests/stream_scraper.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/support/stream_scraper.rs"]
mod test_support;

mod catalog;
mod playback;
mod registration;

mod persistence;
use persistence::synchronize_service_defaults;
pub use persistence::{memory_application_stores, sqlite_application_stores};
