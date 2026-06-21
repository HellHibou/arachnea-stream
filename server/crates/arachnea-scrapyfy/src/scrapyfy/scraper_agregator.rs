use anyhow::{Context, Result};
use futures::future::try_join_all;
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::io::BufReader;
use std::path::Path;

use super::*;

#[cfg(feature = "arachnea-proxy")]
use arachnea_proxy::core::ArachneaProxyCore;

/// Runtime parameters grouped by source name for one aggregated query execution.
pub type ScraperSourceParams = HashMap<String, HashMap<String, String>>;

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
    true
}

/// Aggregates the same query across several configured sources.
pub struct ScraperAgregator {
    queries_collection: Vec<ScraperQueryCollection>,
    proxy_handle: SharedProxyConfigHandle,
    #[cfg(feature = "arachnea-proxy")]
    proxy_core: Option<ArachneaProxyCore>,
}

impl ScraperAgregator {
    /// Creates an empty aggregator with no loaded query collections.
    pub fn new() -> Self {
        Self::new_with_proxy_handle(SharedProxyConfigHandle::new())
    }

    /// Creates an empty aggregator bound to one shared proxy handle.
    pub fn new_with_proxy_handle(proxy_handle: SharedProxyConfigHandle) -> Self {
        ScraperAgregator {
            queries_collection: Vec::new(),
            proxy_handle,
            #[cfg(feature = "arachnea-proxy")]
            proxy_core: None,
        }
    }

    /// Returns the shared proxy handle used by this aggregator and its queries.
    pub fn proxy_handle(&self) -> SharedProxyConfigHandle {
        self.proxy_handle.clone()
    }

    /// Sets the proxy core instance used by clients created through this aggregator.
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
            let handle = SharedProxyConfigHandle::new();
            handle.set_proxy(HttpProxyConfig::Arachnea(proxy_core.clone()));
            return HttpClient::with_http_config_and_proxy_handle(http_config, handle);
        }

        HttpClient::with_http_config(http_config)
    }

    /// Loads every configured source from a config file using the provided parser.
    ///
    /// # Arguments
    ///
    /// * `config_path` - Path to the aggregator config file.
    /// * `parse` - Deserializer used to parse each source file content into a query collection raw.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file or one of the source files cannot be loaded or parsed.
    #[allow(dead_code)]
    pub fn add_query_collection_from_config<F, E>(
        &mut self,
        config_path: impl AsRef<Path>,
        parse: F,
    ) -> Result<&mut Self>
    where
        F: Fn(BufReader<fs::File>) -> std::result::Result<ScraperQueryCollectionRaw, E>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let config_path = config_path.as_ref();
        let config_file = fs::File::open(config_path)
            .with_context(|| format!("Failed to open config file: {}", config_path.display()))?;
        let sources: Vec<ScraperAggregatorSourceEntry> =
            serde_json::from_reader(BufReader::new(config_file)).with_context(|| {
                format!("Failed to parse config file: {}", config_path.display())
            })?;

        let config_dir = config_path.parent().unwrap_or(Path::new(""));

        for source in sources.into_iter().filter(|s| s.enabled) {
            let source_path = config_dir.join(&source.path);
            let file = fs::File::open(&source_path).with_context(|| {
                format!("Failed to open source file: {}", source_path.display())
            })?;
            let reader = BufReader::new(file);
            let mut raw: ScraperQueryCollectionRaw = parse(reader).with_context(|| {
                format!("Failed to parse source file: {}", source_path.display())
            })?;

            // Merge parameters: start with YAML's, override with source-specific if provided
            if !source.parameters.is_empty() {
                let mut param_map: HashMap<String, ScraperQueryCollectionParameter> = raw
                    .parameters
                    .into_iter()
                    .map(|p| (p.name.clone(), p))
                    .collect();
                for param in source.parameters {
                    param_map.insert(param.name.clone(), param);
                }
                raw.parameters = param_map.into_values().collect();
            }

            let mut collection: ScraperQueryCollection = raw.try_into().with_context(|| {
                format!(
                    "Failed to convert source config to collection: {}",
                    source_path.display()
                )
            })?;
            collection.set_proxy_handle(self.proxy_handle.clone());

            self.queries_collection.push(collection);
        }

        Ok(self)
    }

    /// Loads every configured source from files using the provided parser.
    ///
    /// # Arguments
    ///
    /// * `paths` - List of configuration files to load and aggregate.
    /// * `parse` - Deserializer used to parse each file content into a query collection.
    ///
    /// # Errors
    ///
    /// Returns an error if one of the files cannot be loaded or parsed.
    #[allow(dead_code)]
    pub fn add_query_collection_from_files<F, E>(
        &mut self,
        paths: Vec<String>,
        parse: F,
    ) -> Result<&mut Self>
    where
        F: Fn(BufReader<fs::File>) -> std::result::Result<ScraperQueryCollection, E>,
        E: std::error::Error + Send + Sync + 'static,
    {
        for path in paths {
            let mut collection =
                ScraperQueryCollection::from_file_with(path, |reader| parse(reader))?;
            collection.set_proxy_handle(self.proxy_handle.clone());
            self.queries_collection.push(collection);
        }

        Ok(self)
    }

    /// Loads every configured source from its YAML file.
    ///
    /// # Arguments
    ///
    /// * `yaml_paths` - List of YAML configuration files to load and aggregate.
    ///
    /// # Errors
    ///
    /// Returns an error if one of the YAML files cannot be loaded or parsed.
    #[allow(dead_code)]
    pub fn add_query_collection_from_files_yaml(
        &mut self,
        yaml_paths: Vec<String>,
    ) -> Result<&mut Self> {
        self.add_query_collection_from_files(yaml_paths, serde_yaml::from_reader)?;
        Ok(self)
    }

    /// Loads every configured source from a JSON config file.
    ///
    /// # Arguments
    ///
    /// * `config_path` - Path to the JSON aggregator config file.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file or one of the source files cannot be loaded or parsed.
    pub fn add_query_collection_from_config_json(
        &mut self,
        config_path: impl AsRef<Path>,
    ) -> Result<&mut Self> {
        self.add_query_collection_from_config(config_path, serde_yaml::from_reader)?;
        Ok(self)
    }

    /// Returns the configured parameters for one loaded source.
    ///
    /// # Arguments
    ///
    /// * `name` - Source name to look up.
    pub fn query_collection_parameters(
        &self,
        name: &str,
    ) -> Option<&[ScraperQueryCollectionParameter]> {
        self.queries_collection
            .iter()
            .find(|query_collection| query_collection.name() == name)
            .map(ScraperQueryCollection::parameters)
    }

    #[cfg(any(test, feature = "test-support"))]
    /// Returns the last loaded query collection.
    pub fn get_last_query_collection(&self) -> Option<&ScraperQueryCollection> {
        self.queries_collection.last()
    }

    #[cfg(any(test, feature = "test-support"))]
    /// Returns one loaded query collection by source name.
    ///
    /// # Arguments
    ///
    /// * `name` - Source name to look up.
    pub fn get_query_collection(&self, name: &str) -> Option<&ScraperQueryCollection> {
        for entry in &self.queries_collection {
            if entry.name() == name {
                return Some(&entry);
            }
        }

        return None;
    }

    /// Executes the same query on all sources and merges the resulting entries.
    ///
    /// # Arguments
    ///
    /// * `query_name` - Name of the query to execute on every configured source.
    /// * `params` - Runtime values passed to each source-specific query template.
    /// * `source_params` - Optional per-source parameter overrides merged into `params`.
    /// * `scrapper_list` - Optional list of source names to execute. When `None`, every
    ///   configured source is queried.
    /// * `query_media_type_filter` - Filter on media_type query.
    /// * `fields_filters` - Root fields filter list or None.
    /// * `source_field_name` - Optional metadata key used to store the originating source name.
    ///
    /// The returned entries are already tree-shaped when fields include `>` in their names.
    ///
    /// # Errors
    ///
    /// Returns an error if one of the underlying query collections fails.
    pub async fn execute_query_async(
        &self,
        query_name: &str,
        params: &HashMap<String, String>,
        source_params: Option<&ScraperSourceParams>,
        scrapper_list: Option<&Vec<String>>,
        query_media_type_filter: Option<&Vec<String>>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
        source_field_name: Option<&str>,
    ) -> Result<Vec<HashMap<String, ScraperDataNode>>> {
        let filtered_queries: Vec<&ScraperQueryCollection> = self
            .queries_collection
            .iter()
            .filter(|query_collection| match scrapper_list {
                Some(scrapper_list) => scrapper_list
                    .iter()
                    .any(|scrapper_name| scrapper_name == query_collection.name()),
                _none => true,
            })
            .collect();

        let results = try_join_all(filtered_queries.iter().map(|query_collection| {
            let mut execution_params = params.clone();

            if let Some(source_params) = source_params {
                if let Some(params_for_source) = source_params.get(query_collection.name()) {
                    execution_params.extend(params_for_source.clone());
                }
            }

            async move {
                query_collection
                    .execute_query(
                        query_name,
                        &execution_params,
                        query_media_type_filter,
                        fields_filters,
                    )
                    .await
            }
        }))
        .await?;

        let mut aggregated_results: Vec<HashMap<String, ScraperDataNode>> = Vec::new();
        for (query, entries) in filtered_queries.into_iter().zip(results) {
            for mut entry in entries {
                if let Some(source_field_name) = source_field_name {
                    // Preserve the origin of each row when the caller requests it.
                    entry.insert(
                        source_field_name.to_string(),
                        ScraperDataNode::from_values(vec![query.name().to_string()]),
                    );
                }

                aggregated_results.push(entry);
            }
        }

        Ok(aggregated_results)
    }
}
