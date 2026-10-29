use anyhow::Result;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use arachnea_core::controler::{RequestControlerContext, HEADER_IF_NONE_MATCH};
use arachnea_core::error_code::ErrorCodeGenerator;
use arachnea_core::persistence::TypedEntityStore;
use arachnea_http::chaser_session::{memory_session_store, CachedChaserSession};

#[cfg(feature = "arachnea-proxy")]
use arachnea_proxy::core::proxy_repository::memory_proxy_store;
use scraper_result::{
    ScraperAggregationResult, ScraperErrorOrigin, ScraperExecutionError, ScraperSourceStatus,
    ScraperSourceValidation,
};

/// Shared diagnostic-code generator for attributed scraper execution failures.
static ERROR_CODE_GEN: LazyLock<ErrorCodeGenerator> = LazyLock::new(ErrorCodeGenerator::new);

use super::*;

#[cfg(feature = "arachnea-proxy")]
use arachnea_proxy::core::{ArachneaProxyCore, ProxyRecord};

/// Runtime parameters grouped by source name for one aggregated query execution.
pub type ScraperSourceParams = HashMap<String, HashMap<String, String>>;

/// Borrowed source selection and result metadata for one aggregated query.
pub struct ScraperAggregationOptions<'a> {
    /// Per-source parameter overrides merged into the shared runtime parameters.
    pub source_params: Option<&'a ScraperSourceParams>,
    /// Source names to execute, or every configured source in the group when absent.
    pub scrapper_list: Option<&'a Vec<String>>,
    /// Optional media types used to select queries.
    pub query_media_type_filter: Option<&'a Vec<String>>,
    /// Optional root field filters grouped by field name.
    pub fields_filters: Option<&'a HashMap<String, Vec<String>>>,
    /// Optional metadata key used to store the originating source name.
    pub source_field_name: Option<&'a str>,
    /// Logical operation name used in error correlation codes.
    pub operation: &'a str,
}

/// Entry for one source in the aggregator config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScraperAggregatorSourceEntry {
    /// Path to the YAML file, relative to the config file directory.
    pub path: String,
    /// Whether this source is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Optional parameters specific to this source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ScraperQueryCollectionParameter>,
}

/// Returns `true` — the default enabled state for a source entry.
fn default_enabled() -> bool {
    crate::SCRAPER_SOURCE_DEFAULT_ENABLED
}

/// Declares a recursive JSON manifest import relative to the declaring manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScraperAggregatorImport {
    /// JSON manifest to import, relative to the current manifest directory.
    pub import: String,
}

/// One entry in a scraper-aggregator services manifest, either a local YAML
/// source or a recursive import of another JSON manifest.
///
/// The `untagged` representation keeps existing flat manifests valid while
/// letting an entry reference another manifest through `{"import": ...}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScraperAggregatorConfigEntry {
    /// A local YAML source, resolved relative to the declaring manifest.
    Source(ScraperAggregatorSourceEntry),
    /// A recursive import of another JSON manifest.
    Import(ScraperAggregatorImport),
}

/// One named service group loaded by the scraper aggregator.
pub struct ScraperQueryService {
    /// Technical group identifier used to select this service collection.
    pub name: String,
    /// Query collections loaded from the group's configured sources.
    pub services: Vec<ScraperQueryCollection>,
    /// JSON manifest path used to load the group.
    pub json_path: String,
}

/// Aggregates the same query across several configured sources.
pub struct ScraperAgregator {
    queries_collection: HashMap<String, ScraperQueryService>,
    proxy_handle: SharedProxyConfigHandle,
    local_country: SharedLocalCountry,
    /// Typed store backing the dynamic proxy inventory cache.
    #[cfg(feature = "arachnea-proxy")]
    pub(crate) proxy_store: Arc<dyn TypedEntityStore<ProxyRecord>>,
    #[cfg(feature = "arachnea-proxy")]
    /// Repository coordinating available inventory records and retained endpoint failures.
    pub(crate) proxy_repository: Arc<dyn arachnea_proxy::ProxyRepository>,
    #[cfg(feature = "arachnea-proxy")]
    /// Repository persisting accepted endpoint history independently from inventory records.
    pub(crate) proxy_preferences: Arc<dyn arachnea_proxy::ProxyPreferenceRepository>,
    /// Typed store backing the Cloudflare session cache of created HTTP clients.
    session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    source_enabled: Option<Arc<dyn ScraperSourceEnabled>>,
    server_cache: Arc<ScraperServerCache>,
    #[cfg(feature = "arachnea-proxy")]
    proxy_core: Option<ArachneaProxyCore>,
}

/// Runtime options applied to a [`ScraperAgregator`] after construction.
///
/// They carry the effective Scrapyfy configuration resolved from application
/// options (CLI or programmatic) and are applied before the first query
/// execution, so a rebuilt aggregator ends up with the same effective
/// configuration as the initial one.
#[derive(Debug, Clone, Default)]
pub struct ScraperRuntimeOptions {
    /// Optional server cache sizing applied before the first query execution.
    ///
    /// `None` keeps the aggregator default cache configuration untouched.
    pub cache_config: Option<ScraperCacheConfig>,
}

impl ScraperRuntimeOptions {
    /// Applies these options to the aggregator.
    ///
    /// Must be called before any query execution; options left at their default
    /// value leave the aggregator configuration untouched.
    ///
    /// # Arguments
    ///
    /// * `scraper` - Aggregator to configure.
    pub fn apply_to(&self, scraper: &mut ScraperAgregator) {
        if let Some(config) = &self.cache_config {
            scraper.set_cache_config(config.clone());
        }
    }
}

impl Default for ScraperAgregator {
    /// Creates the same empty aggregator as [`Self::new`], including its
    /// feature-gated proxy setup and independent in-memory stores.
    fn default() -> Self {
        Self::new()
    }
}

impl ScraperAgregator {
    /// Creates an empty aggregator with default proxy setup.
    ///
    /// Initializes a shared proxy handle, attempts to enable the system proxy,
    /// and tries to create a dynamic proxy core backed by the scrapyfy proxy
    /// provider for country-based routing.
    pub fn new() -> Self {
        #[cfg(feature = "arachnea-proxy")]
        {
            Self::new_with_typed_stores(
                memory_proxy_store().expect("in-memory proxy store is valid"),
                memory_session_store().expect("in-memory Cloudflare session store is valid"),
            )
        }
        #[cfg(not(feature = "arachnea-proxy"))]
        Self::new_with_typed_stores(
            memory_session_store().expect("in-memory Cloudflare session store is valid"),
        )
    }

    /// Creates an empty aggregator with default proxy setup and typed stores.
    ///
    /// # Arguments
    ///
    /// * `proxy_store` - Typed store used as the dynamic proxy inventory cache.
    /// * `session_store` - Typed store used as the Cloudflare session cache of
    ///   created HTTP clients.
    pub fn new_with_typed_stores(
        #[cfg(feature = "arachnea-proxy")] proxy_store: Arc<dyn TypedEntityStore<ProxyRecord>>,
        session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    ) -> Self {
        Self::new_with_typed_stores_and_proxy_handle(
            SharedProxyConfigHandle::new(),
            #[cfg(feature = "arachnea-proxy")]
            proxy_store,
            session_store,
        )
    }

    /// Creates an aggregator using an application-owned listener and independent routing state.
    ///
    /// # Arguments
    /// - `proxy_handle`: Transport handle shared with this aggregator's clients.
    /// - `proxy_store`: Dynamic proxy persistence, when proxy support is enabled.
    /// - `session_store`: Shared browser-session persistence.
    pub fn new_with_typed_stores_and_proxy_handle(
        proxy_handle: SharedProxyConfigHandle,
        #[cfg(feature = "arachnea-proxy")] proxy_store: Arc<dyn TypedEntityStore<ProxyRecord>>,
        session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    ) -> Self {
        #[cfg(feature = "arachnea-proxy")]
        {
            if let Err(error) = proxy_handle.enable_system_proxy() {
                tracing::warn!(
                    error = %error,
                    "failed to enable default scraper HTTP proxy; continuing without proxy override"
                );
            }
        }

        ScraperAgregator {
            queries_collection: HashMap::new(),
            proxy_handle,
            local_country: SharedLocalCountry::new(),
            #[cfg(feature = "arachnea-proxy")]
            proxy_repository: Arc::new(arachnea_proxy::TypedProxyRepository::new(
                proxy_store.clone(),
            )),
            #[cfg(feature = "arachnea-proxy")]
            proxy_preferences: Arc::new(arachnea_proxy::TypedProxyPreferenceRepository::new(
                arachnea_proxy::core::proxy_preference_repository::memory_proxy_preference_store()
                    .expect("valid preference schema"),
            )),
            #[cfg(feature = "arachnea-proxy")]
            proxy_store,
            session_store,
            source_enabled: None,
            server_cache: Arc::new(ScraperServerCache::new(ScraperCacheConfig::default())),
            #[cfg(feature = "arachnea-proxy")]
            proxy_core: None,
        }
    }

    /// Initializes the proxy core with independently owned source snapshots.
    #[cfg(feature = "arachnea-proxy")]
    pub fn ensure_proxy_core(&mut self) {
        // The shared persistence store serves both the HTTP clients (cookies,
        // Cloudflare sessions) and the proxy inventory cache; namespaces keep
        // the data families isolated.
        match default_scrapyfy_proxy_core(self, self.proxy_store.clone()) {
            Ok(core) => {
                tracing::info!(
                    "Dynamic proxy core created with scrapyfy provider for country routing"
                );
                self.proxy_handle
                    .set_proxy(HttpProxyConfig::Arachnea(Box::new(core.clone())));
                self.proxy_core = Some(core);
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "failed to create dynamic proxy core; proxy_http will be unavailable"
                );
            }
        }
    }

    /// Builds an owned query runtime for providers that can outlive this aggregator.
    ///
    /// The snapshot shares country and persistence state and activation policy,
    /// but uses a separate system-proxy handle to avoid a core/provider ownership
    /// cycle. Source manifests are parsed independently.
    ///
    /// # Arguments
    /// * `groups` - Loaded groups to copy into the provider runtime.
    #[cfg(feature = "arachnea-proxy")]
    pub(crate) fn provider_snapshot(&self, groups: &[&str]) -> Arc<Self> {
        let proxy_handle = SharedProxyConfigHandle::with_runtime(self.proxy_handle.proxy_runtime());
        if let Err(error) = proxy_handle.enable_system_proxy() {
            tracing::warn!(error = %error, "failed to configure provider system proxy");
        }
        let mut snapshot = Self::new_with_proxy_handle_and_typed_stores(
            proxy_handle,
            self.proxy_store.clone(),
            self.session_store.clone(),
        );
        snapshot.local_country = self.local_country.clone();
        snapshot.proxy_repository = self.proxy_repository.clone();
        snapshot.proxy_preferences = self.proxy_preferences.clone();
        snapshot.source_enabled = self.source_enabled.clone();
        for group in groups {
            if let Some(service) = self.queries_collection.get(*group) {
                if let Err(error) =
                    snapshot.add_query_collection_from_config_json(group, &service.json_path)
                {
                    tracing::warn!(group, error = %error, "failed to build provider source snapshot");
                }
            }
        }
        Arc::new(snapshot)
    }

    /// Creates an empty aggregator bound to one shared proxy handle.
    ///
    /// This constructor does not attempt any default proxy setup. Use it when
    /// the caller wants full control over the proxy configuration.
    ///
    /// # Arguments
    ///
    /// * `proxy_handle` - Shared, versioned routing configuration observed by derived HTTP clients.
    pub fn new_with_proxy_handle(proxy_handle: SharedProxyConfigHandle) -> Self {
        #[cfg(feature = "arachnea-proxy")]
        {
            Self::new_with_proxy_handle_and_typed_stores(
                proxy_handle,
                memory_proxy_store().expect("in-memory proxy store is valid"),
                memory_session_store().expect("in-memory Cloudflare session store is valid"),
            )
        }
        #[cfg(not(feature = "arachnea-proxy"))]
        Self::new_with_proxy_handle_and_typed_stores(
            proxy_handle,
            memory_session_store().expect("in-memory Cloudflare session store is valid"),
        )
    }

    /// Creates an empty aggregator bound to one shared proxy handle and typed
    /// stores.
    ///
    /// This constructor does not attempt any default proxy setup. Use it when
    /// the caller wants full control over the proxy configuration.
    ///
    /// # Arguments
    ///
    /// * `proxy_handle` - Shared proxy handle used by this aggregator.
    /// * `proxy_store` - Typed store used as the dynamic proxy inventory cache.
    /// * `session_store` - Typed store used as the Cloudflare session cache of
    ///   created HTTP clients.
    pub fn new_with_proxy_handle_and_typed_stores(
        proxy_handle: SharedProxyConfigHandle,
        #[cfg(feature = "arachnea-proxy")] proxy_store: Arc<dyn TypedEntityStore<ProxyRecord>>,
        session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    ) -> Self {
        ScraperAgregator {
            queries_collection: HashMap::new(),
            proxy_handle,
            local_country: SharedLocalCountry::new(),
            #[cfg(feature = "arachnea-proxy")]
            proxy_repository: Arc::new(arachnea_proxy::TypedProxyRepository::new(
                proxy_store.clone(),
            )),
            #[cfg(feature = "arachnea-proxy")]
            proxy_preferences: Arc::new(arachnea_proxy::TypedProxyPreferenceRepository::new(
                arachnea_proxy::core::proxy_preference_repository::memory_proxy_preference_store()
                    .expect("valid preference schema"),
            )),
            #[cfg(feature = "arachnea-proxy")]
            proxy_store,
            session_store,
            source_enabled: None,
            server_cache: Arc::new(ScraperServerCache::new(ScraperCacheConfig::default())),
            #[cfg(feature = "arachnea-proxy")]
            proxy_core: None,
        }
    }

    /// Injects shared proxy repositories before initializing or replacing the core.
    ///
    /// # Arguments
    /// * `inventory` - Inventory and rediscovery-protection repository.
    /// * `preferences` - Durable exact-destination accepted histories.
    #[cfg(feature = "arachnea-proxy")]
    pub fn set_proxy_repositories(
        &mut self,
        inventory: Arc<dyn arachnea_proxy::ProxyRepository>,
        preferences: Arc<dyn arachnea_proxy::ProxyPreferenceRepository>,
    ) {
        self.proxy_repository = inventory;
        self.proxy_preferences = preferences;
    }

    /// Sets the optional policy that overrides manifest source activation.
    ///
    /// # Arguments
    ///
    /// * `source_enabled` - Activation policy consulted when loading or executing configured sources.
    pub fn set_source_enabled(&mut self, source_enabled: Arc<dyn ScraperSourceEnabled>) {
        self.source_enabled = Some(source_enabled);
    }

    /// Returns the shared local-country state used by this aggregator.
    pub fn local_country(&self) -> SharedLocalCountry {
        self.local_country.clone()
    }

    /// Stores the explicitly configured local country for this aggregator.
    ///
    /// # Arguments
    ///
    /// * `country` - Country-code text, trimmed and converted to uppercase ASCII.
    pub fn set_explicit_local_country(&self, country: impl AsRef<str>) {
        self.local_country.set_explicit_country(country);
    }

    /// Returns the shared proxy handle used by this aggregator and its queries.
    pub fn proxy_handle(&self) -> SharedProxyConfigHandle {
        self.proxy_handle.clone()
    }

    /// Returns the shared proxy handle used by this aggregator and its queries.
    pub fn get_proxy_handle(&self) -> SharedProxyConfigHandle {
        self.proxy_handle.clone()
    }

    /// Replaces the server-side cache configuration.
    ///
    /// Must be called before any query execution; the underlying hybrid cache
    /// engine is created lazily on the first cached execution.
    ///
    /// # Arguments
    ///
    /// * `config` - Cache sizing and directory configuration used to construct the server-side cache.
    pub fn set_cache_config(&mut self, config: ScraperCacheConfig) {
        self.server_cache = Arc::new(ScraperServerCache::new(config));
    }

    /// Returns the shared server-side cache used by this aggregator.
    pub fn server_cache(&self) -> Arc<ScraperServerCache> {
        self.server_cache.clone()
    }

    /// Builds the deterministic server-cache key of one source execution.
    ///
    /// Returns `None` when the source is unknown within the group.
    ///
    /// # Arguments
    ///
    /// * `group_name` - Service group identifier used to select the source collection.
    /// * `source_name` - Source collection identifier within the service group.
    /// * `query_name` - Query identifier used for lookup and contextual error messages.
    /// * `params` - Runtime parameters available to query templates and action pipelines.
    /// * `source_params` - Source-specific runtime parameters retained across paginated requests.
    /// * `query_media_type_filter` - Media types used for query selection and result filtering.
    /// * `fields_filters` - Field filters applied to produced rows and included in their cache identity.
    #[allow(clippy::too_many_arguments)]
    fn server_cache_key_for_source(
        &self,
        group_name: &str,
        source_name: &str,
        query_name: &str,
        params: &HashMap<String, String>,
        source_params: Option<&ScraperSourceParams>,
        query_media_type_filter: Option<&Vec<String>>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
    ) -> Option<String> {
        let collection = self
            .queries_collection
            .get(group_name)?
            .services
            .iter()
            .find(|collection| collection.name() == source_name)?;
        // Replicates exactly the per-source parameter merge performed by
        // `execute_query_pass` so both sides derive the same key.
        let mut merged_params = params.clone();
        if let Some(source_params) = source_params {
            if let Some(params_for_source) = source_params.get(source_name) {
                merged_params.extend(params_for_source.clone());
            }
        }
        Some(server_cache_key(
            group_name,
            source_name,
            query_name,
            &merged_params,
            query_media_type_filter,
            fields_filters,
            collection.yaml_hash(),
        ))
    }

    /// Sets the proxy core instance used by clients created through this aggregator.
    ///
    /// # Arguments
    ///
    /// * `proxy_core` - Proxy core installed as the shared transport for subsequent scraper requests.
    #[cfg(feature = "arachnea-proxy")]
    pub fn set_proxy_core(&mut self, proxy_core: ArachneaProxyCore) {
        self.proxy_core = Some(proxy_core);
    }

    /// Returns a reference to the proxy core, if one has been set.
    #[cfg(feature = "arachnea-proxy")]
    pub fn proxy_core(&self) -> Option<&ArachneaProxyCore> {
        self.proxy_core.as_ref()
    }

    /// Creates a scraper `HttpClient` configured to use the proxy core when available.
    ///
    /// When `proxy_core` is `Some`, the returned `HttpClient` uses it as the HTTP
    /// proxy transport via `SharedProxyConfigHandle`. Otherwise a standalone client
    /// is created with the given config.
    ///
    /// # Arguments
    ///
    /// * `http_config` - Scraper HTTP configuration (mode, user agent, etc.).
    pub fn create_http_client(&self, http_config: ScraperHttpConfig) -> HttpClient {
        #[cfg(feature = "arachnea-proxy")]
        if let Some(proxy_core) = &self.proxy_core {
            let handle = self.proxy_handle.with_independent_config();
            let proxy_core = if !http_config.proxy_countries.is_empty()
                || http_config.proxy_affinity.is_some()
            {
                proxy_core.with_dynamic_proxy_observation_scope()
            } else {
                proxy_core.clone()
            };
            handle.set_proxy(HttpProxyConfig::Arachnea(Box::new(proxy_core)));
            return HttpClient::with_http_config_proxy_handle_and_local_country_and_session_store(
                http_config,
                handle,
                self.local_country.clone(),
                self.session_store.clone(),
            );
        }

        HttpClient::with_http_config_proxy_handle_and_local_country_and_session_store(
            http_config,
            self.proxy_handle.clone(),
            self.local_country.clone(),
            self.session_store.clone(),
        )
    }

    /// Returns every loaded service group and its originating JSON manifest.
    pub fn query_services(&self) -> impl Iterator<Item = &ScraperQueryService> {
        self.queries_collection.values()
    }

    /// Returns the source names within a group, in insertion (services.json) order.
    ///
    /// # Arguments
    ///
    /// * `group_name` - Group name to look up.
    pub fn source_names_in_group(&self, group_name: &str) -> Vec<String> {
        self.queries_collection
            .get(group_name)
            .map(|service| {
                service
                    .services
                    .iter()
                    .map(|collection| collection.name().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns the configured parameters for one loaded source within a group.
    ///
    /// # Arguments
    ///
    /// * `group_name` - Group name to look up.
    /// * `name` - Source name to look up within the group.
    pub fn query_collection_parameters(
        &self,
        group_name: &str,
        name: &str,
    ) -> Option<&[ScraperQueryCollectionParameter]> {
        match self.queries_collection.get(group_name) {
            Some(service) => service
                .services
                .iter()
                .find(|query_collection| query_collection.name() == name)
                .map(ScraperQueryCollection::parameters),
            None => {
                tracing::warn!("query group `{group_name}` not found");
                None
            }
        }
    }

    /// Returns whether one loaded source declares a named query.
    ///
    /// # Arguments
    ///
    /// * `group_name` - Group containing the source.
    /// * `source_name` - Source identifier to inspect.
    /// * `query_name` - Query name that must be declared by the source.
    pub fn source_has_query(&self, group_name: &str, source_name: &str, query_name: &str) -> bool {
        self.queries_collection
            .get(group_name)
            .and_then(|group| {
                group
                    .services
                    .iter()
                    .find(|collection| collection.name() == source_name)
            })
            .is_some_and(|collection| collection.has_query(query_name))
    }

    /// Returns the exact hosts declared by one source for an explicit proxy TLS bypass.
    ///
    /// # Arguments
    ///
    /// * `group_name` - Service group identifier used to select the source collection.
    /// * `name` - Source collection name within the selected group.
    pub fn proxy_insecure_tls_hosts(&self, group_name: &str, name: &str) -> Option<&[String]> {
        self.queries_collection
            .get(group_name)?
            .services
            .iter()
            .find(|query_collection| query_collection.name() == name)
            .map(ScraperQueryCollection::proxy_insecure_tls_hosts)
    }

    /// Returns the deduplicated exact TLS-bypass host allowlist for a query group.
    ///
    /// # Arguments
    ///
    /// * `group_name` - Service group identifier used to select the source collection.
    pub fn group_proxy_insecure_tls_hosts(&self, group_name: &str) -> Vec<String> {
        let mut hosts = self
            .queries_collection
            .get(group_name)
            .into_iter()
            .flat_map(|service| service.services.iter())
            .flat_map(|collection| collection.proxy_insecure_tls_hosts().iter().cloned())
            .map(|host| host.trim().trim_end_matches('.').to_ascii_lowercase())
            .filter(|host| !host.is_empty())
            .collect::<Vec<_>>();
        hosts.sort();
        hosts.dedup();
        hosts
    }
}

#[cfg(test)]
#[path = "../../tests/scrapyfy/scraper_agregator.rs"]
pub(crate) mod tests;

#[cfg(any(test, feature = "test-support"))]
#[path = "../../tests/support/scrapyfy/scraper_agregator.rs"]
mod test_support;

#[cfg(all(feature = "arachnea-proxy", feature = "performance-diagnostics"))]
mod performance_diagnostics;
#[cfg(all(feature = "arachnea-proxy", feature = "performance-diagnostics"))]
use performance_diagnostics::ProxySourceObservation;

mod execution;
mod loading;

mod manifest;
pub use manifest::resolve_manifest_sources;
