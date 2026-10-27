//! Source collection loading and parser adaptation.

use super::*;

impl ScraperAgregator {
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
        let json_path = config_path.to_string_lossy().into_owned();
        tracing::debug!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
            group_name,
            config_path = %config_path.display(),
            "loading scraper query collection config"
        );
        // Relative manifest paths resolve against the application resource
        // root (executable directory, bundle resources, or system resource
        // dir), never against the process working directory.
        let config_path: PathBuf = if config_path.is_relative() {
            arachnea_core::application::get_application_resource_path(
                &config_path.to_string_lossy(),
            )
            .into()
        } else {
            config_path.to_path_buf()
        };
        let sources = resolve_manifest_sources(&config_path)?;

        let config_dir = config_path.parent().unwrap_or(Path::new(""));
        let collections = self
            .queries_collection
            .entry(group_name.to_string())
            .or_insert_with(|| ScraperQueryService {
                name: group_name.to_string(),
                services: Vec::new(),
                json_path,
            });
        let source_count = sources.len();
        let enabled_source_count = sources.iter().filter(|source| source.enabled).count();
        tracing::debug!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
            group_name,
            config_path = %config_path.display(),
            source_count,
            enabled_source_count,
            existing_collection_count = collections.services.len(),
            "parsed scraper query collection config"
        );

        for (source_index, source) in sources.into_iter().enumerate() {
            let source_path = config_dir.join(&source.path);
            tracing::debug!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
                group_name,
                source_index,
                source_path = %source_path.display(),
                source_parameter_overrides = source.parameters.len(),
                "loading scraper query collection source"
            );
            // A missing YAML source is skipped with a warning so the remaining
            // entries keep loading; other open errors stay fatal.
            let file = match fs::File::open(&source_path) {
                Ok(found) => found,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    tracing::warn!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
                        group_name,
                        source_index,
                        source_path = %source_path.display(),
                        error = %error,
                        "skipping missing scraper query collection source file"
                    );
                    continue;
                }
                Err(error) => {
                    return Err(anyhow::anyhow!(
                        "Failed to open source file: {}: {error}",
                        source_path.display()
                    ));
                }
            };
            let reader = BufReader::new(file);
            let source_bytes = fs::read(&source_path).map_err(|error| {
                anyhow::anyhow!(
                    "Failed to read source file: {}: {error}",
                    source_path.display()
                )
            })?;
            let source_yaml_hash = arachnea_core::crypt::hash62_64(&source_bytes);
            let mut raw: ScraperQueryCollectionRaw = parse(reader).map_err(|error| {
                anyhow::anyhow!(
                    "Failed to parse source file: {}: {error}",
                    source_path.display()
                )
            })?;
            let source_id = raw.id.clone();
            let descriptor = ScraperSourceDescriptor {
                service_store_id: group_name.to_string(),
                id: source_id.clone(),
                path: source_path.clone(),
                default_enabled: source.enabled,
                parameters: source.parameters.clone(),
            };
            if let Some(policy) = &self.source_enabled {
                let enabled = if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    tokio::task::block_in_place(|| handle.block_on(policy.is_enabled(&descriptor)))?
                } else {
                    tokio::runtime::Runtime::new()?.block_on(policy.is_enabled(&descriptor))?
                };
                if !enabled {
                    continue;
                }
            } else if !source.enabled {
                continue;
            }
            let query_names: Vec<String> = raw.queries.iter().map(|query| query.name()).collect();
            tracing::debug!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
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
                tracing::debug!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
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
            collection.set_yaml_hash(source_yaml_hash);
            collection.set_runtime_handles_with_session_store(
                self.proxy_handle.clone(),
                self.local_country.clone(),
                self.session_store.clone(),
            );

            collections.services.push(collection);
            tracing::debug!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
                group_name,
                source_index,
                collection_count = collections.services.len(),
                "registered scraper query collection source"
            );
        }

        tracing::debug!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
            group_name,
            total_collection_count = collections.services.len(),
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
            .or_insert_with(|| ScraperQueryService {
                name: group_name.to_string(),
                services: Vec::new(),
                json_path: String::new(),
            });

        for path in paths {
            let source_bytes = fs::read(&path).map_err(|error| {
                anyhow::anyhow!("Failed to read source file: {}: {error}", path)
            })?;
            let source_yaml_hash = arachnea_core::crypt::hash62_64(&source_bytes);
            let mut collection =
                ScraperQueryCollection::from_file_with(path, |reader| parse(reader))?;
            collection.set_yaml_hash(source_yaml_hash);
            collection.set_runtime_handles_with_session_store(
                self.proxy_handle.clone(),
                self.local_country.clone(),
                self.session_store.clone(),
            );
            collections.services.push(collection);
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
}
