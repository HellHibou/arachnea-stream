use anyhow::Result;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::sync::LazyLock;

use arachnea_core::error_code::ErrorCodeGenerator;
use scraper_result::{
    ScraperAggregationResult, ScraperErrorOrigin, ScraperExecutionError,
};

static ERROR_CODE_GEN: LazyLock<ErrorCodeGenerator> = LazyLock::new(ErrorCodeGenerator::new);

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
    queries_collection: HashMap<String, Vec<ScraperQueryCollection>>,
    proxy_handle: SharedProxyConfigHandle,
    local_country: SharedLocalCountry,
    #[cfg(feature = "arachnea-proxy")]
    proxy_core: Option<ArachneaProxyCore>,
}

impl ScraperAgregator {
    /// Creates an empty aggregator with default proxy setup.
    ///
    /// Initializes a shared proxy handle, attempts to enable the system proxy,
    /// and tries to create a dynamic proxy core backed by the scrapyfy proxy
    /// provider for country-based routing.
    pub fn new() -> Self {
        let proxy_handle = SharedProxyConfigHandle::new();

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
            proxy_core: None,
        }
    }

    /// Ensures the proxy core is initialized with a correct self-pointer.
    ///
    /// Must be called once after the aggregator is in its final memory location
    /// (after any move), before proxy routing is used.
    #[cfg(feature = "arachnea-proxy")]
    pub fn ensure_proxy_core(&mut self) {
        let ptr: *mut ScraperAgregator = self;
        match default_scrapyfy_proxy_core(unsafe { &mut *ptr }) {
            Ok(core) => {
                tracing::info!(
                    "Dynamic proxy core created with scrapyfy provider for country routing"
                );
                self.proxy_handle
                    .set_proxy(HttpProxyConfig::Arachnea(core.clone()));
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

    /// Creates an empty aggregator bound to one shared proxy handle.
    ///
    /// This constructor does not attempt any default proxy setup. Use it when
    /// the caller wants full control over the proxy configuration.
    pub fn new_with_proxy_handle(proxy_handle: SharedProxyConfigHandle) -> Self {
        ScraperAgregator {
            queries_collection: HashMap::new(),
            proxy_handle,
            local_country: SharedLocalCountry::new(),
            #[cfg(feature = "arachnea-proxy")]
            proxy_core: None,
        }
    }

    /// Returns the shared local-country state used by this aggregator.
    pub fn local_country(&self) -> SharedLocalCountry {
        self.local_country.clone()
    }

    /// Stores the explicitly configured local country for this aggregator.
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
            return HttpClient::with_http_config_proxy_handle_and_local_country(
                http_config,
                handle,
                self.local_country.clone(),
            );
        }

        HttpClient::with_http_config_proxy_handle_and_local_country(
            http_config,
            self.proxy_handle.clone(),
            self.local_country.clone(),
        )
    }

    /// Loads every configured source from a config file using the provided parser.
    ///
    /// # Arguments
    ///
    /// * `group_name` - Group name under which to register the loaded sources.
    /// * `config_path` - Path to the aggregator config file.
    /// * `parse` - Deserializer used to parse each source file content into a query collection raw.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file or one of the source files cannot be loaded or parsed.
    #[allow(dead_code)]
    pub fn add_query_collection_from_config<F, E>(
        &mut self,
        group_name: &str,
        config_path: impl AsRef<Path>,
        parse: F,
    ) -> Result<&mut Self>
    where
        F: Fn(BufReader<fs::File>) -> std::result::Result<ScraperQueryCollectionRaw, E>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let config_path = config_path.as_ref();
        tracing::debug!(
            group_name,
            config_path = %config_path.display(),
            "loading scraper query collection config"
        );
        let config_file = fs::File::open(config_path).map_err(|error| {
            anyhow::anyhow!(
                "Failed to open config file: {}: {error}",
                config_path.display()
            )
        })?;
        let sources: Vec<ScraperAggregatorSourceEntry> =
            serde_json::from_reader(BufReader::new(config_file)).map_err(|error| {
                anyhow::anyhow!(
                    "Failed to parse config file: {}: {error}",
                    config_path.display()
                )
            })?;

        let config_dir = config_path.parent().unwrap_or(Path::new(""));
        let collections = self
            .queries_collection
            .entry(group_name.to_string())
            .or_default();
        let source_count = sources.len();
        let enabled_source_count = sources.iter().filter(|source| source.enabled).count();
        tracing::debug!(
            group_name,
            config_path = %config_path.display(),
            source_count,
            enabled_source_count,
            existing_collection_count = collections.len(),
            "parsed scraper query collection config"
        );

        for (source_index, source) in sources.into_iter().enumerate() {
            if !source.enabled {
                tracing::debug!(
                    group_name,
                    source_index,
                    source_path = %source.path,
                    "skipping disabled scraper query collection source"
                );
                continue;
            }

            let source_path = config_dir.join(&source.path);
            tracing::debug!(
                group_name,
                source_index,
                source_path = %source_path.display(),
                source_parameter_overrides = source.parameters.len(),
                "loading scraper query collection source"
            );
            let file = fs::File::open(&source_path).map_err(|error| {
                anyhow::anyhow!(
                    "Failed to open source file: {}: {error}",
                    source_path.display()
                )
            })?;
            let reader = BufReader::new(file);
            let mut raw: ScraperQueryCollectionRaw = parse(reader).map_err(|error| {
                anyhow::anyhow!(
                    "Failed to parse source file: {}: {error}",
                    source_path.display()
                )
            })?;
            let source_id = raw.id.clone();
            let query_names: Vec<String> = raw.queries.iter().map(|query| query.name()).collect();
            tracing::debug!(
                group_name,
                source_index,
                source_path = %source_path.display(),
                source_id,
                query_count = query_names.len(),
                query_names = ?query_names,
                collection_parameter_count = raw.parameters.len(),
                "parsed scraper query collection source"
            );

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
                tracing::debug!(
                    group_name,
                    source_index,
                    source_id = %raw.id,
                    merged_parameter_count = raw.parameters.len(),
                    "merged scraper query collection source parameter overrides"
                );
            }

            let mut collection: ScraperQueryCollection = raw.try_into().map_err(|error| {
                anyhow::anyhow!(
                    "Failed to convert source config to collection: {}: {error:#}",
                    source_path.display()
                )
            })?;
            collection.set_runtime_handles(self.proxy_handle.clone(), self.local_country.clone());

            collections.push(collection);
            tracing::debug!(
                group_name,
                source_index,
                collection_count = collections.len(),
                "registered scraper query collection source"
            );
        }

        tracing::debug!(
            group_name,
            total_collection_count = collections.len(),
            "finished loading scraper query collection config"
        );

        Ok(self)
    }

    /// Loads every configured source from files using the provided parser.
    ///
    /// # Arguments
    ///
    /// * `group_name` - Group name under which to register the loaded sources.
    /// * `paths` - List of configuration files to load and aggregate.
    /// * `parse` - Deserializer used to parse each file content into a query collection.
    ///
    /// # Errors
    ///
    /// Returns an error if one of the files cannot be loaded or parsed.
    #[allow(dead_code)]
    pub fn add_query_collection_from_files<F, E>(
        &mut self,
        group_name: &str,
        paths: Vec<String>,
        parse: F,
    ) -> Result<&mut Self>
    where
        F: Fn(BufReader<fs::File>) -> std::result::Result<ScraperQueryCollection, E>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let collections = self
            .queries_collection
            .entry(group_name.to_string())
            .or_default();

        for path in paths {
            let mut collection =
                ScraperQueryCollection::from_file_with(path, |reader| parse(reader))?;
            collection.set_runtime_handles(self.proxy_handle.clone(), self.local_country.clone());
            collections.push(collection);
        }

        Ok(self)
    }

    /// Loads every configured source from its YAML file.
    ///
    /// # Arguments
    ///
    /// * `group_name` - Group name under which to register the loaded sources.
    /// * `yaml_paths` - List of YAML configuration files to load and aggregate.
    ///
    /// # Errors
    ///
    /// Returns an error if one of the YAML files cannot be loaded or parsed.
    #[allow(dead_code)]
    pub fn add_query_collection_from_files_yaml(
        &mut self,
        group_name: &str,
        yaml_paths: Vec<String>,
    ) -> Result<&mut Self> {
        self.add_query_collection_from_files(group_name, yaml_paths, serde_yaml::from_reader)?;
        Ok(self)
    }

    /// Loads every configured source from a JSON config file.
    ///
    /// # Arguments
    ///
    /// * `group_name` - Group name under which to register the loaded sources.
    /// * `config_path` - Path to the JSON aggregator config file.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file or one of the source files cannot be loaded or parsed.
    pub fn add_query_collection_from_config_json(
        &mut self,
        group_name: &str,
        config_path: impl AsRef<Path>,
    ) -> Result<&mut Self> {
        self.add_query_collection_from_config(group_name, config_path, serde_yaml::from_reader)?;
        Ok(self)
    }

    /// Returns the source names within a group, in insertion (services.json) order.
    ///
    /// # Arguments
    ///
    /// * `group_name` - Group name to look up.
    pub fn source_names_in_group(&self, group_name: &str) -> Vec<String> {
        self.queries_collection
            .get(group_name)
            .map(|collections| collections.iter().map(|c| c.name().to_string()).collect())
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
            Some(collections) => collections
                .iter()
                .find(|query_collection| query_collection.name() == name)
                .map(ScraperQueryCollection::parameters),
            None => {
                tracing::warn!("query group `{group_name}` not found");
                None
            }
        }
    }

    /// Returns the exact hosts declared by one source for an explicit proxy TLS bypass.
    pub fn proxy_insecure_tls_hosts(&self, group_name: &str, name: &str) -> Option<&[String]> {
        self.queries_collection
            .get(group_name)?
            .iter()
            .find(|query_collection| query_collection.name() == name)
            .map(ScraperQueryCollection::proxy_insecure_tls_hosts)
    }

    /// Returns the deduplicated exact TLS-bypass host allowlist for a query group.
    pub fn group_proxy_insecure_tls_hosts(&self, group_name: &str) -> Vec<String> {
        let mut hosts = self
            .queries_collection
            .get(group_name)
            .into_iter()
            .flatten()
            .flat_map(|collection| collection.proxy_insecure_tls_hosts().iter().cloned())
            .map(|host| host.trim().trim_end_matches('.').to_ascii_lowercase())
            .filter(|host| !host.is_empty())
            .collect::<Vec<_>>();
        hosts.sort();
        hosts.dedup();
        hosts
    }

    #[cfg(any(test, feature = "test-support"))]
    /// Returns the last loaded query collection within a group.
    pub fn get_last_query_collection(&self, group_name: &str) -> Option<&ScraperQueryCollection> {
        self.queries_collection.get(group_name)?.last()
    }

    #[cfg(any(test, feature = "test-support"))]
    /// Returns one loaded query collection by source name within a group.
    ///
    /// # Arguments
    ///
    /// * `group_name` - Group name to look up.
    /// * `name` - Source name to look up within the group.
    pub fn get_query_collection(
        &self,
        group_name: &str,
        name: &str,
    ) -> Option<&ScraperQueryCollection> {
        for entry in self.queries_collection.get(group_name)? {
            if entry.name() == name {
                return Some(entry);
            }
        }

        None
    }

    /// Executes the same query on all sources within a group and merges the resulting entries.
    ///
    /// # Arguments
    ///
    /// * `group_name` - Group name whose sources should be queried.
    /// * `query_name` - Name of the query to execute on every configured source.
    /// * `params` - Runtime values passed to each source-specific query template.
    /// * `source_params` - Optional per-source parameter overrides merged into `params`.
    /// * `scrapper_list` - Optional list of source names to execute. When `None`, every
    ///   configured source in the group is queried.
    /// * `query_media_type_filter` - Filter on media_type query.
    /// * `fields_filters` - Root fields filter list or None.
    /// * `source_field_name` - Optional metadata key used to store the originating source name.
    /// * `operation` - Logical operation name used in error correlation codes.
    ///
    /// The returned entries are already tree-shaped when fields include `>` in their names.
    ///
    /// Per-source and pre-execution errors are collected in
    /// [`ScraperAggregationResult::errors`] and logged with a correlation code.
    pub async fn execute_query_async(
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
    ) -> ScraperAggregationResult<Vec<HashMap<String, ScraperDataNode>>> {
        let Some(queries_collection) = self.queries_collection.get(group_name) else {
            let code = ERROR_CODE_GEN.next_code();
            let message = format!("query group `{group_name}` not found");
            tracing::error!(
                error_code = %code,
                operation = operation,
                source = ?Option::<String>::None,
                error = %message,
                "query execution could not start",
            );
            return ScraperAggregationResult::new(
                Vec::new(),
                vec![ScraperExecutionError {
                    code,
                    operation: operation.to_string(),
                    source: None,
                    origin: ScraperErrorOrigin::Backend,
                    message,
                }],
            );
        };

        let filtered_queries: Vec<&ScraperQueryCollection> = queries_collection
            .iter()
            .filter(|query_collection| match scrapper_list {
                Some(scrapper_list) => scrapper_list
                    .iter()
                    .any(|scrapper_name| scrapper_name == query_collection.name()),
                _none => true,
            })
            .collect();

        if filtered_queries.is_empty() {
            let code = ERROR_CODE_GEN.next_code();
            let message = format!("no configured sources selected in query group `{group_name}`");
            tracing::error!(
                error_code = %code,
                operation = operation,
                source = ?Option::<String>::None,
                error = %message,
                "query execution could not start",
            );
            return ScraperAggregationResult::new(
                Vec::new(),
                vec![ScraperExecutionError {
                    code,
                    operation: operation.to_string(),
                    source: None,
                    origin: ScraperErrorOrigin::Backend,
                    message,
                }],
            );
        }

        let results = join_all(filtered_queries.iter().map(|query_collection| {
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
        .await;

        let mut aggregated_results: Vec<HashMap<String, ScraperDataNode>> = Vec::new();
        let mut errors: Vec<ScraperExecutionError> = Vec::new();

        for (query, result) in filtered_queries.into_iter().zip(results) {
            match result {
                Ok(entries) => {
                    for mut entry in entries {
                        if let Some(source_field_name) = source_field_name {
                            // Preserve the origin of each row when the caller requests it.
                            entry.insert(
                                source_field_name.to_string(),
                                ScraperDataNode::from_values_typed(
                                    vec![query.name().to_string()],
                                    ScraperOutputType::String,
                                ),
                            );
                        }

                        aggregated_results.push(entry);
                    }
                }
                Err(error) => {
                    let code = ERROR_CODE_GEN.next_code();
                    let source_name = query.name().to_string();
                    tracing::error!(
                        error_code = %code,
                        operation = operation,
                        source = %source_name,
                        error = %error,
                        "source query failed",
                    );
                    errors.push(ScraperExecutionError {
                        code,
                        operation: operation.to_string(),
                        source: Some(source_name),
                        origin: ScraperErrorOrigin::Backend,
                        message: format!("{error:#}"),
                    });
                }
            }
        }

        ScraperAggregationResult::new(aggregated_results, errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arachnea_core::error_code::ErrorCodeGenerator;
    use scraper_result::{
        ScraperAggregationResult, ScraperErrorOrigin, ScraperExecutionError,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_DIR_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX_EPOCH")
            .as_nanos();
        let sequence = TEMP_DIR_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "arachnea-scrapyfy-aggregator-test-{}-{nanos}-{sequence}",
            std::process::id()
        ))
    }

    fn setup_aggregator_sources(sources: &[(&str, &str)]) -> (ScraperAgregator, PathBuf) {
        let dir = unique_temp_dir();
        std::fs::create_dir_all(&dir).expect("test service directory should be created");

        let mut service_entries = Vec::new();
        for (name, yaml) in sources {
            let file_name = format!("{name}.yaml");
            std::fs::write(dir.join(&file_name), yaml).expect("test YAML should be written");
            service_entries.push(serde_json::json!({
                "path": file_name,
                "enabled": true,
            }));
        }

        let config_path = dir.join("services.json");
        std::fs::write(
            &config_path,
            serde_json::to_string(&service_entries).expect("test services config should serialize"),
        )
        .expect("test services config should be written");

        let mut aggregator =
            ScraperAgregator::new_with_proxy_handle(SharedProxyConfigHandle::new());
        aggregator
            .add_query_collection_from_config_json("test", &config_path)
            .expect("test sources should load");

        (aggregator, dir)
    }

    fn static_source(id: &str, value: &str) -> String {
        format!(
            r#"
id: {id}
queries:
  - name: probe
    scraper_type: static
    entries:
      - name: value
        type: string
        value: {value}
"#
        )
    }

    fn failing_html_source(id: &str, url: &str) -> String {
        format!(
            r#"
id: {id}
queries:
  - name: probe
    scraper_type: html
    base_url: {url}
    query_url: {url}
    row_selector: html
    entries:
      - name: value
        type: string
        selector: title
        actions:
          - type: get_text
"#
        )
    }

    fn value_from_row(row: &HashMap<String, ScraperDataNode>) -> Option<&str> {
        row.get("value")
            .and_then(|node| node.values.first())
            .map(String::as_str)
    }

    #[test]
    fn error_code_generator_monotonic() {
        let gen = ErrorCodeGenerator::new();
        let a = gen.next_code();
        let b = gen.next_code();
        let c = gen.next_code();

        let a_str = a.to_string();
        let b_str = b.to_string();
        let c_str = c.to_string();

        assert!(
            a_str.starts_with("ARACHNEA_E"),
            "code should start with prefix"
        );
        assert!(b_str >= a_str, "codes should be monotonic");
        assert!(c_str >= b_str, "codes should be monotonic");
        assert_ne!(a_str, b_str, "consecutive codes should differ");
    }

    #[test]
    fn error_code_format() {
        let gen = ErrorCodeGenerator::new();
        let code = gen.next_code();
        let s = code.to_string();

        // Format: ARACHNEA_E{millis}{seq:02}
        assert!(s.starts_with("ARACHNEA_E"), "missing prefix");
        let numeric = s.trim_start_matches("ARACHNEA_E");
        assert!(
            numeric.len() >= 15,
            "expected millis + 2-digit seq, got '{}'",
            numeric
        );
        assert!(
            numeric.chars().all(|c| c.is_ascii_digit()),
            "numeric part should be all digits, got '{}'",
            numeric,
        );

        // Sequence part (last 2 chars)
        let seq = &numeric[numeric.len() - 2..];
        assert!(
            seq.parse::<u8>().is_ok(),
            "sequence should be 2-digit number, got '{}'",
            seq,
        );
    }

    #[test]
    fn error_code_serializable() {
        let gen = ErrorCodeGenerator::new();
        let code = gen.next_code();
        let json = serde_json::to_string(&code).expect("serialization should succeed");
        assert!(
            json.contains("ARACHNEA_E"),
            "serialized code should contain prefix"
        );
    }

    #[test]
    fn aggregation_result_ok_has_no_errors() {
        let data: Vec<HashMap<String, String>> = vec![HashMap::new()];
        let result = ScraperAggregationResult::ok(data.clone());
        assert_eq!(result.data, data);
        assert!(result.errors.is_empty());
        assert!(result.is_ok());
    }

    #[test]
    fn aggregation_result_with_errors() {
        let data: Vec<i32> = vec![1, 2, 3];
        let errors = vec![ScraperExecutionError {
            code: ErrorCodeGenerator::new().next_code(),
            operation: "test_op".to_string(),
            source: Some("source-a".to_string()),
            origin: ScraperErrorOrigin::Backend,
            message: "something went wrong".to_string(),
        }];
        let result = ScraperAggregationResult::new(data.clone(), errors);
        assert_eq!(result.data, vec![1, 2, 3]);
        assert!(!result.errors.is_empty());
        assert!(!result.is_ok());
    }

    #[test]
    fn execution_error_has_code() {
        let gen = ErrorCodeGenerator::new();
        let error = ScraperExecutionError {
            code: gen.next_code(),
            operation: "load_home".to_string(),
            source: Some("my-source".to_string()),
            origin: ScraperErrorOrigin::Backend,
            message: "HTTP 500".to_string(),
        };
        assert!(
            error.code.to_string().starts_with("ARACHNEA_E"),
            "error should carry a correlation code",
        );
        assert_eq!(error.operation, "load_home");
        assert_eq!(error.source.as_deref(), Some("my-source"));
    }

    #[test]
    fn execution_error_nullable_source() {
        let gen = ErrorCodeGenerator::new();
        let error = ScraperExecutionError {
            code: gen.next_code(),
            operation: "validate".to_string(),
            source: None,
            origin: ScraperErrorOrigin::Backend,
            message: "pre-execution validation failed".to_string(),
        };
        assert!(
            error.source.is_none(),
            "pre-execution errors should have no source"
        );
    }

    #[tokio::test]
    async fn execute_query_async_keeps_successes_in_config_order() {
        let (aggregator, dir) = setup_aggregator_sources(&[
            ("first", &static_source("first", "first")),
            ("second", &static_source("second", "second")),
        ]);

        let result = aggregator
            .execute_query_async(
                "test",
                "probe",
                &HashMap::new(),
                None,
                None,
                None,
                None,
                None,
                "probe",
            )
            .await;

        assert!(result.is_ok(), "full success should not produce errors");
        assert_eq!(result.data.len(), 2);
        assert_eq!(value_from_row(&result.data[0]), Some("first"));
        assert_eq!(value_from_row(&result.data[1]), Some("second"));
        std::fs::remove_dir_all(dir).expect("test service directory should be removed");
    }

    #[tokio::test]
    async fn execute_query_async_preserves_partial_success_and_error_code() {
        let failing_url = "not-a-valid-url";
        let (aggregator, dir) = setup_aggregator_sources(&[
            ("success", &static_source("success", "kept")),
            ("failure", &failing_html_source("failure", &failing_url)),
        ]);

        let result = aggregator
            .execute_query_async(
                "test",
                "probe",
                &HashMap::new(),
                None,
                None,
                None,
                None,
                None,
                "probe",
            )
            .await;

        assert_eq!(
            result.data.len(),
            1,
            "successful source data should be kept"
        );
        assert_eq!(value_from_row(&result.data[0]), Some("kept"));
        assert_eq!(result.errors.len(), 1, "failed source should be reported");
        let error = &result.errors[0];
        assert_eq!(error.operation, "probe");
        assert_eq!(error.source.as_deref(), Some("failure"));
        assert!(matches!(&error.origin, ScraperErrorOrigin::Backend));
        assert!(error.code.to_string().starts_with("ARACHNEA_E"));
        assert!(!error.message.is_empty());
        std::fs::remove_dir_all(dir).expect("test service directory should be removed");
    }

    #[tokio::test]
    async fn execute_query_async_reports_all_failed_sources_in_config_order() {
        let failing_url = "not-a-valid-url";
        let (aggregator, dir) = setup_aggregator_sources(&[
            ("first", &failing_html_source("first", &failing_url)),
            ("second", &failing_html_source("second", &failing_url)),
        ]);

        let result = aggregator
            .execute_query_async(
                "test",
                "probe",
                &HashMap::new(),
                None,
                None,
                None,
                None,
                None,
                "probe",
            )
            .await;

        assert!(
            result.data.is_empty(),
            "all failed sources should produce no data"
        );
        assert_eq!(result.errors.len(), 2);
        assert_eq!(result.errors[0].source.as_deref(), Some("first"));
        assert_eq!(result.errors[1].source.as_deref(), Some("second"));
        assert!(result
            .errors
            .iter()
            .all(|error| error.code.to_string().starts_with("ARACHNEA_E")));
        std::fs::remove_dir_all(dir).expect("test service directory should be removed");
    }

    #[tokio::test]
    async fn execute_query_async_reports_pre_execution_error_without_source() {
        let aggregator = ScraperAgregator::new_with_proxy_handle(SharedProxyConfigHandle::new());
        let result = aggregator
            .execute_query_async(
                "missing",
                "probe",
                &HashMap::new(),
                None,
                None,
                None,
                None,
                None,
                "probe",
            )
            .await;

        assert!(result.data.is_empty());
        assert_eq!(result.errors.len(), 1);
        let error = &result.errors[0];
        assert_eq!(error.operation, "probe");
        assert!(error.source.is_none());
        assert!(error.code.to_string().starts_with("ARACHNEA_E"));
    }
}
