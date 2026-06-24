use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use urlencoding::encode;

use arachnea_core::{
    controler::{
        ControlerService, ControlerServiceExt, ControlerStreamInput, ControlerStreamOutput,
    },
    persistence::{CredentialsStore, FileCredentialsStore},
};
use arachnea_proxy::core::{ArachneaProxyCore, ProxyConfig};
use arachnea_scrapyfy::*;

use crate::services::{
    francetv_resolver::FrancetvResolver,
    m6play_resolver::M6PlayResolver,
    player_resolver::{PlayerResolverEndpoints, PlayerStreamResolver, ResolvedPlayerStream},
    rtbf_auvio_resolver::RtbfAuvioResolver,
    rtlplay_resolver::RtlPlayResolver,
    tf1_resolver::Tf1Resolver,
};

const HTTP_PROXY_COMMAND: &str = "proxy";
const STREAM_PROXY_COMMAND: &str = "get_stream";

static M6PLAY_RESOLVER: M6PlayResolver = M6PlayResolver;
static RTBF_AUVIO_RESOLVER: RtbfAuvioResolver = RtbfAuvioResolver;
static RTLPLAY_RESOLVER: RtlPlayResolver = RtlPlayResolver;
static TF1_RESOLVER: Tf1Resolver = Tf1Resolver;
static FRANCETV_RESOLVER: FrancetvResolver = FrancetvResolver;

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
    source_params: Vec<SourceParamsRequestEntry>,
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
struct ResolvePlayerStreamRequest {
    source: String,
    #[serde(alias = "resolverKind")]
    resolver_kind: String,
    #[serde(alias = "resolverTarget")]
    resolver_target: String,
    #[serde(default, alias = "resolverStreamKind")]
    resolver_stream_kind: Option<String>,
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
    source_params: Vec<SourceParamsRequestEntry>,
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
}

#[derive(Clone, Serialize, Deserialize)]
struct SourceParamsRequestEntry {
    source: String,
    #[serde(flatten)]
    params: HashMap<String, Value>,
}

/// High-level facade exposing scraper operations used by controllers and tests.
pub struct StreamScraper {
    scraper_agregator: ScraperAgregator,
    credentials_store: Arc<dyn CredentialsStore>,
    proxy_handle: SharedProxyConfigHandle,
    proxy_http_core: Option<ArachneaProxyCore>,
    player_resolver_endpoints: PlayerResolverEndpoints,
}

impl StreamScraper {
    /// Loads every configured source from its YAML file.
    ///
    /// # Arguments
    /// credential
    /// # Errors
    ///
    /// Returns an error if one of the YAML files cannot be loaded or parsed.
    pub fn new(credentials: &str) -> Self {
        Self::with_credentials_store(Arc::new(FileCredentialsStore::new(credentials)))
    }

    /// Creates a scraper facade backed by the provided credentials store.
    ///
    /// # Arguments
    /// * `credentials_store` - Shared credentials persistence used by service resolvers.
    ///
    /// # Returns
    /// A configured scraper facade.
    pub fn with_credentials_store(credentials_store: Arc<dyn CredentialsStore>) -> Self {
        let proxy_handle = SharedProxyConfigHandle::new();
        if let Err(error) = proxy_handle.enable_system_proxy() {
            tracing::warn!(
                error = %error,
                "failed to enable default scraper HTTP proxy; continuing without proxy override"
            );
        }

        StreamScraper {
            scraper_agregator: ScraperAgregator::new_with_proxy_handle(proxy_handle.clone()),
            credentials_store,
            proxy_handle,
            proxy_http_core: None,
            player_resolver_endpoints: PlayerResolverEndpoints::default(),
        }
    }

    /// Returns the mutable proxy handle shared by this scraper instance.
    pub fn proxy_handle(&self) -> SharedProxyConfigHandle {
        self.proxy_handle.clone()
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
    ///
    /// # Errors
    ///
    /// Returns an error if query execution cannot be started or if one
    /// of the configured sources fails to execute the search query.
    pub async fn search(
        &self,
        query: String,
        media_types: Vec<String>,
        themes: Vec<String>,
        page: usize,
        source_params: ScraperSourceParams,
    ) -> Result<Vec<HashMap<String, ScraperDataNode>>> {
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

        self.scraper_agregator
            .execute_query_async(
                "search",
                &params,
                Some(&source_params),
                scrapper_list.as_ref(),
                media,
                None,
                None,
            )
            .await
    }

    /// Convenience helper for the `get_entry` query using the provided absolute entry URL.
    ///
    /// # Arguments
    ///
    /// * `query_source` - Query source name.
    /// * `query_url` - Absolute URL of the entry page to fetch.
    ///
    /// # Errors
    ///
    /// Returns an error if query execution cannot be started or if one
    /// of the configured sources fails to execute the `get_entry` query.
    pub async fn get_entry(
        &self,
        query_source: String,
        query_url: String,
    ) -> Result<Vec<HashMap<String, ScraperDataNode>>> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("query_url".to_string(), query_url);
        self.enrich_runtime_params(&mut params);

        let scrapper_list = vec![query_source];

        self.scraper_agregator
            .execute_query_async(
                "get_entry",
                &params,
                None,
                Some(&scrapper_list),
                None,
                None,
                None,
            )
            .await
    }

    /// Convenience helper for the `get_season` query using the provided absolute season URL.
    ///
    /// # Arguments
    ///
    /// * `query_source` - Query source name.
    /// * `query_url` - Absolute URL of the season payload to fetch.
    /// * `page` - 1-based page number requested from the backend source.
    ///
    /// # Errors
    ///
    /// Returns an error if query execution cannot be started or if one
    /// of the configured sources fails to execute the `get_season` query.
    pub async fn get_season(
        &self,
        query_source: String,
        query_url: String,
        page: usize,
    ) -> Result<HashMap<String, ScraperDataNode>> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("query_url".to_string(), query_url);
        params.insert("page".to_string(), page.max(1).to_string());
        self.enrich_runtime_params(&mut params);

        let scrapper_list = vec![query_source];

        let mut results = self
            .scraper_agregator
            .execute_query_async(
                "get_season",
                &params,
                None,
                Some(&scrapper_list),
                None,
                None,
                None,
            )
            .await?;

        Ok(results.pop().unwrap_or_default())
    }

    /// Loads the aggregated live catalog across every configured source.
    ///
    /// # Errors
    ///
    /// Returns an error if one of the configured sources fails to execute the
    /// `list_lives` query.
    pub async fn list_lives(&self) -> Result<Vec<HashMap<String, ScraperDataNode>>> {
        let mut params: HashMap<String, String> = HashMap::new();
        self.enrich_runtime_params(&mut params);
        let lives = self
            .scraper_agregator
            .execute_query_async(
                "list_lives",
                &params,
                None,
                None,
                None,
                None,
                Some("source"),
            )
            .await?;

        Ok(lives)
    }

    /// Loads one live payload for the provided source and channel identifier.
    ///
    /// # Arguments
    ///
    /// * `query_source` - Query source name.
    /// * `channel` - Source-specific live channel identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if query execution cannot be started or if one
    /// of the configured sources fails to execute the `get_live` query.
    pub async fn get_live(
        &self,
        query_source: String,
        channel: String,
    ) -> Result<HashMap<String, ScraperDataNode>> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("channel".to_string(), channel);
        self.enrich_runtime_params(&mut params);

        let scrapper_list = vec![query_source];

        let mut results = self
            .scraper_agregator
            .execute_query_async(
                "get_live",
                &params,
                None,
                Some(&scrapper_list),
                None,
                None,
                None,
            )
            .await?;

        Ok(results.pop().unwrap_or_default())
    }

    /// Loads the aggregated home catalog across every configured source.
    ///
    /// # Errors
    ///
    /// Returns an error if one of the configured sources fails to execute the
    /// `load_home` query.
    pub async fn load_home(&self) -> Result<Vec<HashMap<String, ScraperDataNode>>> {
        let mut params: HashMap<String, String> = HashMap::new();
        self.enrich_runtime_params(&mut params);
        self.scraper_agregator
            .execute_query_async("load_home", &params, None, None, None, None, Some("source"))
            .await
    }

    /// Loads display metadata for every configured source.
    ///
    /// # Errors
    ///
    /// Returns an error if one of the configured sources fails to execute the
    /// `service_stream_metadata` query.
    pub async fn get_service(&self) -> Result<Vec<HashMap<String, ScraperDataNode>>> {
        let mut params: HashMap<String, String> = HashMap::new();
        self.enrich_runtime_params(&mut params);

        self.scraper_agregator
            .execute_query_async(
                "service_stream_metadata",
                &params,
                None,
                None,
                None,
                None,
                None,
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
    ///
    /// # Errors
    ///
    /// Returns an error if a descriptor is missing its source name or if one
    /// selected source fails to execute the `get_category` query.
    pub async fn get_category(
        &self,
        sources: Vec<HashMap<String, String>>,
        page: usize,
        source_params: ScraperSourceParams,
    ) -> Result<Vec<HashMap<String, ScraperDataNode>>> {
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

        self.scraper_agregator
            .execute_query_async(
                "get_category",
                &params,
                Some(&source_params),
                Some(&scrapper_list),
                None,
                None,
                Some("source"),
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
    ///
    /// # Errors
    ///
    /// Returns an error if the selected source fails to execute the `get_section` query.
    pub async fn get_section(
        &self,
        query_source: String,
        query_url: String,
        page: usize,
        mut source_params: ScraperSourceParams,
    ) -> Result<HashMap<String, ScraperDataNode>> {
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

        let rows = self
            .scraper_agregator
            .execute_query_async(
                "get_section",
                &params,
                Some(&source_params),
                scrapper_list.as_ref(),
                None,
                None,
                Some("source"),
            )
            .await?;

        let mut root = ScraperDataNode::default();
        for row in rows {
            root.merge(ScraperDataNode {
                children: row,
                ..Default::default()
            });
        }

        Ok(root.children)
    }

    async fn resolve_player_stream(
        &self,
        query_source: String,
        resolver_kind: String,
        resolver_target: String,
        resolver_stream_kind: Option<String>,
    ) -> Result<ResolvedPlayerStream> {
        let source = query_source.trim();
        let resolver = player_resolver_for_source(source)
            .ok_or_else(|| anyhow::anyhow!("Unsupported player source `{}`.", source))?;
        let service_parameters = self
            .scraper_agregator
            .query_collection_parameters(source)
            .unwrap_or(&[]);

        resolver
            .resolve_player_stream(
                &self.scraper_agregator,
                self.credentials_store.as_ref(),
                &resolver_kind,
                &resolver_target,
                resolver_stream_kind,
                service_parameters,
                &self.player_resolver_endpoints,
            )
            .await
    }

    async fn get_stream(&self, input: ControlerStreamInput) -> Result<ControlerStreamOutput> {
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
            .get_stream(&self.scraper_agregator, token, &input.body)
            .await?;

        Ok(ControlerStreamOutput {
            status: 200,
            body: response.body,
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

impl ScraperManager for StreamScraper {
    fn get_scraper_agregator_mut(&mut self) -> &mut ScraperAgregator {
        &mut self.scraper_agregator
    }

    fn create_http_client(&self, http_config: ScraperHttpConfig) -> HttpClient {
        self.scraper_agregator.create_http_client(http_config)
    }

    fn register_service(mut self, controler: &mut dyn ControlerService) {
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
            |scraper, input: SearchRequest| async move {
                scraper
                    .search(
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
            |scraper, _input: LoadHomeRequest| async move { scraper.load_home().await },
        );

        controler.register_result_function_with_state(
            "get_service",
            Arc::clone(&connector),
            |scraper, _input: GetServiceRequest| async move { scraper.get_service().await },
        );

        controler.register_result_function_with_state(
            "list_lives",
            Arc::clone(&connector),
            |scraper, _input: ListLivesRequest| async move { scraper.list_lives().await },
        );

        controler.register_result_function_with_state(
            "get_category",
            Arc::clone(&connector),
            |scraper, input: GetCategoryRequest| async move {
                scraper
                    .get_category(
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
            |scraper, input: GetSectionRequest| async move {
                scraper
                    .get_section(
                        input.source,
                        input.link,
                        input.page,
                        source_params_from_entries(input.source_params),
                    )
                    .await
            },
        );

        controler.register_result_function_with_state(
            "get_entry",
            Arc::clone(&connector),
            |scraper, input: GetEntryRequest| async move {
                scraper.get_entry(input.source, input.entry).await
            },
        );

        controler.register_result_function_with_state(
            "get_season",
            Arc::clone(&connector),
            |scraper, input: GetSeasonRequest| async move {
                scraper
                    .get_season(input.source, input.season, input.page)
                    .await
            },
        );

        controler.register_result_function_with_state(
            "get_live",
            Arc::clone(&connector),
            |scraper, input: GetLiveRequest| async move {
                scraper.get_live(input.source, input.channel).await
            },
        );

        controler.register_result_function_with_state(
            "resolve_player_stream",
            Arc::clone(&connector),
            |scraper, input: ResolvePlayerStreamRequest| async move {
                scraper
                    .resolve_player_stream(
                        input.source,
                        input.resolver_kind,
                        input.resolver_target,
                        input.resolver_stream_kind,
                    )
                    .await
            },
        );

        controler.register_stream_function_with_state(
            STREAM_PROXY_COMMAND,
            Arc::clone(&connector),
            |scraper, input| async move { scraper.get_stream(input).await },
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
