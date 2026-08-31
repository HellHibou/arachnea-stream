use anyhow::{Context, Result};
use arachnea_core::persistence::PersistenceStore;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;
use std::fs;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::scrapyfy::query_helpers::replace_template_placeholders;
use crate::scrapyfy::scraper::query_trait::ScraperQuery;
use crate::scrapyfy::scraper_html::query::HtmlScraperSubQuery;
use crate::scrapyfy::scraper_json::query::JsonScraperSubQuery;
use crate::scrapyfy::scraper_text::query::{TextScraperQuery, TextScraperQueryRaw};

/// One collection-level default parameter available to every query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScraperQueryCollectionParameter {
    /// Parameter name used as a `{placeholder}` in query templates.
    pub name: String,
    /// Default value for this parameter.
    pub value: String,
    /// Optional human-readable description of the parameter's purpose.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional actions applied to the parameter value before use.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<crate::scrapyfy::actions::ScraperAction>,
}

impl ScraperQueryCollectionParameter {
    /// Validates and normalizes this parameter, returning the trimmed name and raw value.
    ///
    /// # Errors
    ///
    /// Returns an error if the parameter name is empty after trimming.
    fn validate_and_normalize(&self) -> Result<(String, String)> {
        let parameter_name = self.name.trim();
        if parameter_name.is_empty() {
            anyhow::bail!("Collection parameter name cannot be empty");
        }

        // Description is now optional
        Ok((parameter_name.to_string(), self.value.clone()))
    }
}

/// Optional account requirements declared by a service YAML file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScraperServiceCredentials {
    /// Whether the service requires credentials before it can be used.
    #[serde(default)]
    pub required: bool,
    /// Optional public URL where a user can create a service account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signup_url: Option<String>,
}

/// Raw configuration structure describing the queries available for one source.
#[derive(Serialize, Deserialize)]
pub struct ScraperQueryCollectionRaw {
    /// Unique source identifier.
    #[serde(alias = "name")]
    pub id: String,
    /// Optional human-readable title for the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional logo URL or path for the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    /// Optional multi-language description map.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub description: HashMap<String, String>,
    /// Optional credentials requirements exposed to administration clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<ScraperServiceCredentials>,
    /// Collection-level default parameters merged into every query execution.
    #[serde(default, alias = "parametres", skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ScraperQueryCollectionParameter>,
    /// HTTP configuration shared across all queries in this collection.
    #[serde(default, skip_serializing_if = "ScraperHttpConfig::is_empty")]
    pub http: ScraperHttpConfig,
    /// Exact HTTPS hosts eligible for an explicit proxy TLS bypass.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proxy_insecure_tls_hosts: Vec<String>,
    /// Query definitions available for this source.
    pub queries: Vec<ScraperQueryDefinitionRaw>,
}

/// Raw configuration wrapper supporting both HTML and JSON query definitions.
#[derive(Serialize, Deserialize)]
#[serde(tag = "scraper_type", rename_all = "snake_case")]
pub enum ScraperQueryDefinitionRaw {
    /// HTML scraper query definition.
    Html {
        #[serde(flatten)]
        /// HTML scraper query configuration.
        query: HtmlScraperQueryRaw,
    },
    /// JSON scraper query definition.
    Json {
        #[serde(flatten)]
        /// JSON scraper query configuration.
        query: JsonScraperQueryRaw,
    },
    /// Static scraper query definition.
    Static {
        #[serde(flatten)]
        /// Static scraper query configuration.
        query: StaticScraperQueryRaw,
    },
    /// Text scraper query definition.
    Text {
        #[serde(flatten)]
        /// Text scraper query configuration.
        query: TextScraperQueryRaw,
    },
}

impl ScraperQueryDefinitionRaw {
    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        match self {
            ScraperQueryDefinitionRaw::Html { query } => query.name(),
            ScraperQueryDefinitionRaw::Json { query } => query.name(),
            ScraperQueryDefinitionRaw::Static { query } => query.name(),
            ScraperQueryDefinitionRaw::Text { query } => query.name(),
        }
    }

    /// Resolves collection-level placeholders in the inner query configuration.
    ///
    /// # Arguments
    ///
    /// * `params` - Collection-level template parameters used for placeholder resolution.
    fn resolve_collection_params(&mut self, params: &HashMap<String, String>) -> Result<()> {
        match self {
            ScraperQueryDefinitionRaw::Html { query } => query.resolve_collection_params(params),
            ScraperQueryDefinitionRaw::Json { query } => query.resolve_collection_params(params),
            ScraperQueryDefinitionRaw::Static { query } => query.resolve_collection_params(params),
            ScraperQueryDefinitionRaw::Text { query } => query.resolve_collection_params(params),
        }
    }

    /// Propagates the collection-level HTTP configuration into the inner query.
    ///
    /// # Arguments
    ///
    /// * `http` - HTTP configuration inherited from the parent collection.
    fn apply_collection_http(&mut self, http: &ScraperHttpConfig) {
        match self {
            ScraperQueryDefinitionRaw::Html { query } => query.apply_collection_http(http),
            ScraperQueryDefinitionRaw::Json { query } => query.apply_collection_http(http),
            ScraperQueryDefinitionRaw::Static { .. } => {}
            ScraperQueryDefinitionRaw::Text { query } => query.apply_collection_http(http),
        }
    }
}

/// Runtime query wrapper supporting both HTML, JSON, and static scraping backends.
#[derive(Deserialize)]
#[serde(try_from = "ScraperQueryDefinitionRaw")]
pub enum ScraperQueryDefinition {
    /// HTML scraper query.
    Html(HtmlScraperQuery),
    /// JSON scraper query.
    Json(JsonScraperQuery),
    /// Static scraper query.
    Static(StaticScraperQuery),
    /// Text scraper query.
    Text(TextScraperQuery),
}

impl ScraperQueryDefinition {
    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        match self {
            ScraperQueryDefinition::Html(query) => query.name(),
            ScraperQueryDefinition::Json(query) => query.name(),
            ScraperQueryDefinition::Static(query) => query.name(),
            ScraperQueryDefinition::Text(query) => query.name(),
        }
    }

    /// Returns whether the query matches at least one requested media type.
    pub fn is_media_type(&self, media_types: &[String]) -> bool {
        match self {
            ScraperQueryDefinition::Html(query) => query.is_media_type(media_types),
            ScraperQueryDefinition::Json(query) => query.is_media_type(media_types),
            ScraperQueryDefinition::Static(query) => query.is_media_type(media_types),
            ScraperQueryDefinition::Text(query) => query.is_media_type(media_types),
        }
    }

    /// Returns the flattened list of leaf field names produced by this query.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_field_names(&self) -> Vec<String> {
        match self {
            ScraperQueryDefinition::Html(query) => query.get_field_names(),
            ScraperQueryDefinition::Json(query) => query.get_field_names(),
            ScraperQueryDefinition::Static(query) => query.get_field_names(),
            ScraperQueryDefinition::Text(query) => query.get_field_names(),
        }
    }

    /// Executes the query and returns the extracted rows.
    ///
    /// All query types (HTML, JSON, Static) are dispatched through the
    /// unified polymorphic executor [`scraper::query_executor::execute_query_items`].
    /// When `result_item_field` is set, the returned rows are flattened by
    /// promoting the group's items to top-level entries.
    pub async fn execute_query(
        &self,
        params: &HashMap<String, String>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
    ) -> Result<Vec<HashMap<String, ScraperDataNode>>> {
        // Apply query_param_mappings before creating the context
        let (query_ref, result_item_field, execution_params) = match self {
            ScraperQueryDefinition::Html(query) => {
                let mappings = &query.query_param_mappings;
                let base_url = query.base_url();
                let execution_params = crate::scrapyfy::query_helpers::build_query_execution_params(
                    base_url, params, mappings,
                );
                (
                    &*query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    query.result_item_field(),
                    execution_params,
                )
            }
            ScraperQueryDefinition::Json(query) => {
                let mappings = &query.query_param_mappings;
                let base_url = query.base_url();
                let execution_params = crate::scrapyfy::query_helpers::build_query_execution_params(
                    base_url, params, mappings,
                );
                (
                    &*query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    query.result_item_field(),
                    execution_params,
                )
            }
            ScraperQueryDefinition::Static(query) => {
                // Static queries don't have query_param_mappings, just use params as-is
                // but still resolve nested templates
                let execution_params =
                    crate::scrapyfy::query_helpers::resolve_nested_template_params(params);
                (
                    &*query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    None,
                    execution_params,
                )
            }
            ScraperQueryDefinition::Text(query) => {
                let mappings = &query.query_param_mappings;
                let base_url = query.base_url();
                let execution_params = crate::scrapyfy::query_helpers::build_query_execution_params(
                    base_url, params, mappings,
                );
                (
                    &*query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    query.result_item_field(),
                    execution_params,
                )
            }
        };

        let dynamic_template_variables =
            std::sync::Mutex::new(crate::scrapyfy::query_helpers::DynamicTemplateVariables::new());
        let context = crate::scrapyfy::scraper::query_executor::QueryContext {
            params: &execution_params,
            dynamic_template_variables: &dynamic_template_variables,
            request_url: "",
            response_body: None,
            http_client: query_ref.http_client(),
            fields_filters,
            parent_response: None,
            validation: None,
        };

        let rows =
            crate::scrapyfy::scraper::query_executor::execute_query_items(query_ref, &context)
                .await?;

        Ok(match result_item_field {
            Some(result_item_field) => flatten_result_item_field(rows, result_item_field),
            None => rows,
        })
    }

    /// Executes the query with optional conditional validation of its root request.
    ///
    /// When `client_fragment` is provided, conditional headers (`If-None-Match`
    /// / `If-Modified-Since`) are applied to the source's first request and a
    /// `304` or identical content hash skips parsing entirely.
    ///
    /// # Arguments
    ///
    /// * `params` - Runtime values used to format the query URL template.
    /// * `fields_filters` - Root fields filter list or None.
    /// * `client_fragment` - Client-provided validator fragment for this source.
    /// * `yaml_hash` - Base62 hash of the source YAML document, embedded in
    ///   every outgoing validator fragment.
    ///
    /// # Returns
    ///
    /// The extracted rows alongside the root-fetch outcome, when the query
    /// performed an HTTP fetch.
    ///
    /// # Errors
    ///
    /// Returns an error if query execution fails.
    pub async fn execute_query_with_validation(
        &self,
        params: &HashMap<String, String>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
        client_fragment: Option<&str>,
        yaml_hash: &str,
    ) -> Result<(
        Vec<HashMap<String, ScraperDataNode>>,
        Option<crate::scrapyfy::RootFetchOutcome>,
    )> {
        // Apply query_param_mappings before creating the context
        let (query_ref, result_item_field, execution_params) = match self {
            ScraperQueryDefinition::Html(query) => {
                let mappings = &query.query_param_mappings;
                let base_url = query.base_url();
                let execution_params = crate::scrapyfy::query_helpers::build_query_execution_params(
                    base_url, params, mappings,
                );
                (
                    &*query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    query.result_item_field(),
                    execution_params,
                )
            }
            ScraperQueryDefinition::Json(query) => {
                let mappings = &query.query_param_mappings;
                let base_url = query.base_url();
                let execution_params = crate::scrapyfy::query_helpers::build_query_execution_params(
                    base_url, params, mappings,
                );
                (
                    &*query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    query.result_item_field(),
                    execution_params,
                )
            }
            ScraperQueryDefinition::Static(query) => {
                let execution_params =
                    crate::scrapyfy::query_helpers::resolve_nested_template_params(params);
                (
                    &*query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    None,
                    execution_params,
                )
            }
            ScraperQueryDefinition::Text(query) => {
                let mappings = &query.query_param_mappings;
                let base_url = query.base_url();
                let execution_params = crate::scrapyfy::query_helpers::build_query_execution_params(
                    base_url, params, mappings,
                );
                (
                    &*query as &dyn crate::scrapyfy::scraper::query_trait::ScraperQuery,
                    query.result_item_field(),
                    execution_params,
                )
            }
        };

        let dynamic_template_variables =
            std::sync::Mutex::new(crate::scrapyfy::query_helpers::DynamicTemplateVariables::new());
        let validation_slot = crate::scrapyfy::ValidationSlot::new(
            client_fragment.map(str::to_string),
            yaml_hash.to_string(),
        );
        let context = crate::scrapyfy::scraper::query_executor::QueryContext {
            params: &execution_params,
            dynamic_template_variables: &dynamic_template_variables,
            request_url: "",
            response_body: None,
            http_client: query_ref.http_client(),
            fields_filters,
            parent_response: None,
            validation: Some(&validation_slot),
        };

        let rows =
            crate::scrapyfy::scraper::query_executor::execute_query_items(query_ref, &context)
                .await?;
        let outcome = validation_slot.take();

        let rows = match result_item_field {
            Some(result_item_field) => flatten_result_item_field(rows, result_item_field),
            None => rows,
        };
        Ok((rows, outcome))
    }

    /// Rebinds all embedded HTTP clients to the provided shared proxy handle.
    pub fn set_proxy_handle(&mut self, proxy_handle: SharedProxyConfigHandle) {
        self.set_runtime_handles(proxy_handle, SharedLocalCountry::new());
    }

    /// Rebinds all embedded HTTP clients to the provided shared runtime handles.
    pub fn set_runtime_handles(
        &mut self,
        proxy_handle: SharedProxyConfigHandle,
        local_country: SharedLocalCountry,
    ) {
        self.set_runtime_handles_with_persistence_store(
            proxy_handle,
            local_country,
            Arc::new(arachnea_core::persistence::MemoryPersistenceStore::new()),
        );
    }

    /// Rebinds all embedded HTTP clients to the provided shared runtime handles
    /// and persistence store.
    pub fn set_runtime_handles_with_persistence_store(
        &mut self,
        proxy_handle: SharedProxyConfigHandle,
        local_country: SharedLocalCountry,
        persistence_store: Arc<dyn PersistenceStore>,
    ) {
        match self {
            ScraperQueryDefinition::Html(query) => bind_html_query_runtime_handles(
                query,
                &proxy_handle,
                &local_country,
                &persistence_store,
            ),
            ScraperQueryDefinition::Json(query) => bind_json_query_runtime_handles(
                query,
                &proxy_handle,
                &local_country,
                &persistence_store,
            ),
            ScraperQueryDefinition::Static(query) => query
                .set_runtime_handles_with_persistence_store(
                    proxy_handle,
                    local_country,
                    persistence_store,
                ),
            ScraperQueryDefinition::Text(query) => bind_text_query_runtime_handles(
                query,
                &proxy_handle,
                &local_country,
                &persistence_store,
            ),
        }
    }
}

/// Flattens a group field's items into top-level entries when `result_item_field` is set.
///
/// # Arguments
///
/// * `rows` - Original query result rows.
/// * `target` - Name of the group field whose items should become top-level entries.
fn flatten_result_item_field(
    rows: Vec<HashMap<String, ScraperDataNode>>,
    target: &str,
) -> Vec<HashMap<String, ScraperDataNode>> {
    let mut flattened = Vec::new();

    for mut row in rows {
        let Some(group) = row.remove(target) else {
            flattened.push(row);
            continue;
        };

        if group.items.is_empty() {
            flattened.push(row);
            continue;
        }

        flattened.extend(group.items.into_iter().map(|item| item.children));
    }

    flattened
}

impl TryFrom<ScraperQueryDefinitionRaw> for ScraperQueryDefinition {
    type Error = anyhow::Error;

    /// Converts a raw query definition into its validated runtime variant.
    ///
    /// # Errors
    ///
    /// Returns an error if the inner query definition is invalid.
    fn try_from(config: ScraperQueryDefinitionRaw) -> Result<Self> {
        match config {
            ScraperQueryDefinitionRaw::Html { query } => Ok(Self::Html(query.try_into()?)),
            ScraperQueryDefinitionRaw::Json { query } => Ok(Self::Json(query.try_into()?)),
            ScraperQueryDefinitionRaw::Static { query } => Ok(Self::Static(query.try_into()?)),
            ScraperQueryDefinitionRaw::Text { query } => Ok(Self::Text(query.try_into()?)),
        }
    }
}

impl From<&ScraperQueryDefinition> for ScraperQueryDefinitionRaw {
    /// Converts a runtime query definition back into its raw YAML-compatible representation.
    fn from(query: &ScraperQueryDefinition) -> Self {
        match query {
            ScraperQueryDefinition::Html(query) => Self::Html {
                query: HtmlScraperQueryRaw::from(query),
            },
            ScraperQueryDefinition::Json(query) => Self::Json {
                query: JsonScraperQueryRaw::from(query),
            },
            ScraperQueryDefinition::Static(query) => Self::Static {
                query: StaticScraperQueryRaw::from(query),
            },
            ScraperQueryDefinition::Text(query) => Self::Text {
                query: TextScraperQueryRaw::from(query),
            },
        }
    }
}

impl Serialize for ScraperQueryDefinition {
    /// Serializes the query definition through its raw YAML representation.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ScraperQueryDefinitionRaw::from(self).serialize(serializer)
    }
}

/// Set of named queries loaded from one YAML file.
#[derive(Deserialize)]
#[serde(try_from = "ScraperQueryCollectionRaw")]
pub struct ScraperQueryCollection {
    name: String,
    title: String,
    logo: String,
    description: HashMap<String, String>,
    parameters: Vec<ScraperQueryCollectionParameter>,
    proxy_insecure_tls_hosts: Vec<String>,
    parameter_defaults: HashMap<String, String>,
    queries: HashMap<String, ScraperQueryDefinition>,
    yaml_hash: String,
}

impl ScraperQueryCollection {
    /// Builds a query collection indexed by query name.
    ///
    /// # Arguments
    ///
    /// * `name` - Collection name, typically matching the source identifier.
    /// * `title` - Human-readable title for the source.
    /// * `logo` - Logo URL or path for the source.
    /// * `description` - Multi-language description map.
    /// * `parameters` - Collection-level default parameters merged into every execution.
    /// * `parameter_defaults` - Resolved and action-processed parameter defaults indexed by name.
    /// * `queries` - Query definitions indexed by their individual names.
    pub fn new(
        name: &str,
        title: &str,
        logo: &str,
        description: HashMap<String, String>,
        parameters: Vec<ScraperQueryCollectionParameter>,
        proxy_insecure_tls_hosts: Vec<String>,
        parameter_defaults: HashMap<String, String>,
        queries: Vec<ScraperQueryDefinition>,
    ) -> Self {
        let mut collection = Self {
            name: name.to_string(),
            title: title.to_string(),
            logo: logo.to_string(),
            description,
            parameters,
            proxy_insecure_tls_hosts,
            parameter_defaults,
            queries: HashMap::new(),
            yaml_hash: String::new(),
        };

        for query in queries {
            collection.add_query(query);
        }

        collection
    }

    /// Returns the collection name, usually matching the source identifier.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the collection-level parameters available to this source.
    pub fn parameters(&self) -> &[ScraperQueryCollectionParameter] {
        &self.parameters
    }

    /// Returns exact hosts eligible for an explicit proxy TLS bypass.
    pub fn proxy_insecure_tls_hosts(&self) -> &[String] {
        &self.proxy_insecure_tls_hosts
    }

    /// Returns the base62 hash of the source YAML document backing this collection.
    ///
    /// The hash is computed once from the raw file bytes at load time and is
    /// embedded in every validator fragment so a scraper edit invalidates the
    /// cached response even when the remote content is unchanged.
    pub fn yaml_hash(&self) -> &str {
        &self.yaml_hash
    }

    /// Records the base62 hash of the raw source YAML document.
    pub(crate) fn set_yaml_hash(&mut self, yaml_hash: String) -> &mut Self {
        self.yaml_hash = yaml_hash;
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    /// Returns one query definition by name for test assertions.
    ///
    /// # Arguments
    ///
    /// * `query_name` - Name of the query to look up.
    pub fn get_query(&self, query_name: &str) -> Option<&ScraperQueryDefinition> {
        return self.queries.get(query_name);
    }

    /// Loads one query collection from a reader using the provided parser.
    ///
    /// # Arguments
    ///
    /// * `reader` - Reader providing the serialized configuration data.
    /// * `parse` - Deserializer used to parse the reader into a query collection.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration content is invalid.
    pub fn from_reader_with<R, F, E>(reader: R, parse: F) -> Result<Self>
    where
        R: Read,
        F: FnOnce(R) -> std::result::Result<Self, E>,
        E: std::error::Error + Send + Sync + 'static,
    {
        parse(reader).context("Failed to parse config file")
    }

    /// Loads one query collection from a file using the provided parser.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the configuration file describing the source queries.
    /// * `parse` - Deserializer used to parse the file reader into a query collection.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or if the configuration content is invalid.
    pub fn from_file_with<F, E>(path: impl AsRef<Path>, parse: F) -> Result<Self>
    where
        F: FnOnce(BufReader<fs::File>) -> std::result::Result<Self, E>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let path = path.as_ref();
        let file = fs::File::open(path)
            .with_context(|| format!("Failed to open config file: {}", path.display()))?;
        let reader = BufReader::new(file);

        Self::from_reader_with(reader, parse)
    }

    /// Inserts or replaces a query with the same name.
    ///
    /// # Arguments
    ///
    /// * `query` - Query definition to insert into the collection.
    pub fn add_query(&mut self, query: ScraperQueryDefinition) -> &mut Self {
        self.queries.insert(query.name(), query);
        self
    }

    /// Rebinds all query HTTP clients to the provided shared proxy handle.
    pub fn set_proxy_handle(&mut self, proxy_handle: SharedProxyConfigHandle) {
        self.set_runtime_handles(proxy_handle, SharedLocalCountry::new());
    }

    /// Rebinds all query HTTP clients to the provided shared runtime handles.
    pub fn set_runtime_handles(
        &mut self,
        proxy_handle: SharedProxyConfigHandle,
        local_country: SharedLocalCountry,
    ) {
        self.set_runtime_handles_with_persistence_store(
            proxy_handle,
            local_country,
            Arc::new(arachnea_core::persistence::MemoryPersistenceStore::new()),
        );
    }

    /// Rebinds all query HTTP clients to the provided shared runtime handles
    /// and persistence store.
    pub fn set_runtime_handles_with_persistence_store(
        &mut self,
        proxy_handle: SharedProxyConfigHandle,
        local_country: SharedLocalCountry,
        persistence_store: Arc<dyn PersistenceStore>,
    ) {
        for query in self.queries.values_mut() {
            query.set_runtime_handles_with_persistence_store(
                proxy_handle.clone(),
                local_country.clone(),
                persistence_store.clone(),
            );
        }
    }

    /// Executes one named query within this collection.
    ///
    /// # Arguments
    ///
    /// * `query_name` - Name of the query to execute.
    /// * `params` - Runtime values used to format the query URL template. Collection-level
    ///   default parameters are merged first, and runtime values override them.
    /// * `query_media_type_filter` - Filter on media_type query.
    /// * `fields_filters` - Root fields filter list or None.
    ///
    /// The returned entries are tree-shaped when fields include `>` in their names.
    ///
    /// # Errors
    ///
    /// Returns an error if `query_name` is unknown or if query execution fails.
    pub async fn execute_query(
        &self,
        query_name: &str,
        params: &HashMap<String, String>,
        query_media_type_filter: Option<&Vec<String>>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
    ) -> Result<Vec<HashMap<String, ScraperDataNode>>> {
        self.execute_query_with_validation(
            query_name,
            params,
            query_media_type_filter,
            fields_filters,
            None,
            self.yaml_hash(),
        )
        .await
        .map(|(rows, _)| rows)
    }

    /// Executes one named query within this collection with optional
    /// conditional validation of the source's root request.
    ///
    /// # Arguments
    ///
    /// * `query_name` - Name of the query to execute.
    /// * `params` - Runtime values used to format the query URL template.
    /// * `query_media_type_filter` - Filter on media_type query.
    /// * `fields_filters` - Root fields filter list or None.
    /// * `client_fragment` - Client-provided validator fragment for this source.
    ///
    /// # Returns
    ///
    /// The extracted rows alongside the root-fetch outcome, when the query
    /// performed an HTTP fetch.
    ///
    /// # Errors
    ///
    /// Returns an error if `query_name` is unknown or if query execution fails.
    pub async fn execute_query_with_validation(
        &self,
        query_name: &str,
        params: &HashMap<String, String>,
        query_media_type_filter: Option<&Vec<String>>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
        client_fragment: Option<&str>,
        yaml_hash: &str,
    ) -> Result<(
        Vec<HashMap<String, ScraperDataNode>>,
        Option<crate::scrapyfy::RootFetchOutcome>,
    )> {
        let mut merged_params = self.build_execution_params(params);
        merged_params
            .entry("source".to_string())
            .or_insert_with(|| self.name.clone());

        match self.queries.get(query_name) {
            Some(value) => {
                if query_media_type_filter.is_none()
                    || value.is_media_type(query_media_type_filter.unwrap())
                {
                    value
                        .execute_query_with_validation(
                            &merged_params,
                            fields_filters,
                            client_fragment,
                            yaml_hash,
                        )
                        .await
                } else {
                    Ok((Vec::new(), None))
                }
            }
            _none => Ok((Vec::new(), None)),
        }
    }

    /// Process all collection parameters sequentially in YAML declaration order.
    ///
    /// For each parameter:
    /// 1. Resolve `{placeholder}` references using already-resolved parameters.
    /// 2. Apply its actions (e.g. map) to transform the resolved value.
    ///
    /// This ensures that a parameter defined later in the YAML file can reference
    /// an earlier parameter's final (action-processed) value.
    ///
    /// # Arguments
    ///
    /// * `parameters` - Collection-level parameters to validate and resolve.
    ///
    /// # Errors
    ///
    /// Returns an error if a parameter name is empty or if duplicate names exist.
    fn build_parameter_defaults(
        parameters: &[ScraperQueryCollectionParameter],
    ) -> Result<HashMap<String, String>> {
        let mut resolved = HashMap::new();

        for parameter in parameters {
            let (parameter_name, mut current_value) = parameter.validate_and_normalize()?;

            // 1. Resolve {placeholders} using already-processed parameters
            let (resolved_value, _) = replace_template_placeholders(&current_value, &resolved);
            current_value = resolved_value;

            // 2. Apply actions in order on the resolved value
            let mut values = vec![current_value];
            for action in &parameter.actions {
                values = action.apply(&None, values, &resolved, "", None, None);
            }

            // Take the first value after all actions
            let final_value = values.into_iter().next().unwrap_or_default();

            if resolved
                .insert(parameter_name.clone(), final_value)
                .is_some()
            {
                anyhow::bail!("Duplicate collection parameter: {}", parameter_name);
            }
        }

        Ok(resolved)
    }

    /// Builds service metadata defaults from the collection identity and description.
    ///
    /// # Arguments
    ///
    /// * `id` - Source identifier.
    /// * `title` - Human-readable source title.
    /// * `logo` - Logo URL or path.
    /// * `description` - Multi-language description map serialized as JSON.
    fn build_service_metadata_defaults(
        id: &str,
        title: &str,
        logo: &str,
        description: &HashMap<String, String>,
    ) -> Result<HashMap<String, String>> {
        let mut defaults = HashMap::new();
        defaults.insert("service_id".to_string(), id.to_string());
        defaults.insert("service_title".to_string(), title.to_string());
        defaults.insert("service_logo".to_string(), logo.to_string());
        defaults.insert(
            "service_description".to_string(),
            serde_json::to_string(description)?,
        );
        Ok(defaults)
    }

    /// Merges collection parameter defaults with runtime parameters, then
    /// enriches the result with pagination and query separator helpers.
    ///
    /// Parameter defaults are already fully resolved and have had their actions
    /// applied during collection loading. Runtime parameters may still contain
    /// `{placeholder}` references that need resolution.
    ///
    /// # Arguments
    ///
    /// * `runtime_params` - Caller-provided parameters that override defaults.
    fn build_execution_params(
        &self,
        runtime_params: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        let mut merged_params = self.parameter_defaults.clone();

        for (key, value) in runtime_params {
            merged_params.insert(key.clone(), value.clone());
        }

        Self::enrich_pagination_params(&mut merged_params);
        Self::enrich_query_separator_params(&mut merged_params);

        crate::scrapyfy::query_helpers::resolve_nested_template_params(&merged_params)
    }

    /// Derives `page_index` and `offset` from `page` and `page_size` parameters.
    ///
    /// # Arguments
    ///
    /// * `params` - Mutable parameter map enriched in place.
    fn enrich_pagination_params(params: &mut HashMap<String, String>) {
        let page = params
            .get("page")
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0);
        let page_size = params
            .get("page_size")
            .or_else(|| params.get("pageSize"))
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0);

        if let Some(page) = page {
            params.insert("page".to_string(), page.to_string());
            params
                .entry("page_index".to_string())
                .or_insert_with(|| (page - 1).to_string());

            if let Some(page_size) = page_size {
                params
                    .entry("page_size".to_string())
                    .or_insert_with(|| page_size.to_string());
                params
                    .entry("offset".to_string())
                    .or_insert_with(|| ((page - 1) * page_size).to_string());
            }
        }
    }

    /// Inserts a `query_separator` parameter (`?`, `&`, or empty) derived from
    /// the `query_url` or `link` parameter.
    ///
    /// # Arguments
    ///
    /// * `params` - Mutable parameter map enriched in place.
    fn enrich_query_separator_params(params: &mut HashMap<String, String>) {
        let Some(query_url) = params
            .get("query_url")
            .or_else(|| params.get("link"))
            .map(String::as_str)
        else {
            return;
        };

        let separator = query_parameter_separator(query_url);
        params
            .entry("query_separator".to_string())
            .or_insert_with(|| separator.to_string());
    }
}

fn bind_text_query_runtime_handles(
    query: &mut TextScraperQuery,
    proxy_handle: &SharedProxyConfigHandle,
    local_country: &SharedLocalCountry,
    persistence_store: &Arc<dyn PersistenceStore>,
) {
    query.http_client =
        HttpClient::with_http_config_proxy_handle_and_local_country_and_persistence_store(
            query.http_config.clone(),
            proxy_handle.clone(),
            local_country.clone(),
            persistence_store.clone(),
        );
}

fn bind_html_query_runtime_handles(
    query: &mut HtmlScraperQuery,
    proxy_handle: &SharedProxyConfigHandle,
    local_country: &SharedLocalCountry,
    persistence_store: &Arc<dyn PersistenceStore>,
) {
    query.http_client =
        HttpClient::with_http_config_proxy_handle_and_local_country_and_persistence_store(
            query.http_config.clone(),
            proxy_handle.clone(),
            local_country.clone(),
            persistence_store.clone(),
        );

    for sub_query in &mut query.sub_queries {
        if let Some(html_sub_query) = sub_query.as_any_mut().downcast_mut::<HtmlScraperSubQuery>() {
            bind_html_sub_query_runtime_handles(
                html_sub_query,
                proxy_handle,
                local_country,
                persistence_store,
            );
        } else if let Some(json_sub_query) =
            sub_query.as_any_mut().downcast_mut::<JsonScraperSubQuery>()
        {
            bind_json_sub_query_runtime_handles(
                json_sub_query,
                proxy_handle,
                local_country,
                persistence_store,
            );
        }
    }
}

fn bind_html_sub_query_runtime_handles(
    query: &mut HtmlScraperSubQuery,
    proxy_handle: &SharedProxyConfigHandle,
    local_country: &SharedLocalCountry,
    persistence_store: &Arc<dyn PersistenceStore>,
) {
    query.http_client =
        HttpClient::with_http_config_proxy_handle_and_local_country_and_persistence_store(
            query.http_config.clone(),
            proxy_handle.clone(),
            local_country.clone(),
            persistence_store.clone(),
        );
}

fn bind_json_query_runtime_handles(
    query: &mut JsonScraperQuery,
    proxy_handle: &SharedProxyConfigHandle,
    local_country: &SharedLocalCountry,
    persistence_store: &Arc<dyn PersistenceStore>,
) {
    query.http_client =
        HttpClient::with_http_config_proxy_handle_and_local_country_and_persistence_store(
            query.http_config.clone(),
            proxy_handle.clone(),
            local_country.clone(),
            persistence_store.clone(),
        );

    for sub_query in &mut query.sub_queries {
        if let Some(json_sub_query) = sub_query.as_any_mut().downcast_mut::<JsonScraperSubQuery>() {
            bind_json_sub_query_runtime_handles(
                json_sub_query,
                proxy_handle,
                local_country,
                persistence_store,
            );
        } else if let Some(html_sub_query) =
            sub_query.as_any_mut().downcast_mut::<HtmlScraperSubQuery>()
        {
            bind_html_sub_query_runtime_handles(
                html_sub_query,
                proxy_handle,
                local_country,
                persistence_store,
            );
        }
    }
}

fn bind_json_sub_query_runtime_handles(
    query: &mut JsonScraperSubQuery,
    proxy_handle: &SharedProxyConfigHandle,
    local_country: &SharedLocalCountry,
    persistence_store: &Arc<dyn PersistenceStore>,
) {
    query.http_client =
        HttpClient::with_http_config_proxy_handle_and_local_country_and_persistence_store(
            query.http_config.clone(),
            proxy_handle.clone(),
            local_country.clone(),
            persistence_store.clone(),
        );

    for sub_query in &mut query.sub_queries {
        if let Some(json_sub_query) = sub_query.as_any_mut().downcast_mut::<JsonScraperSubQuery>() {
            bind_json_sub_query_runtime_handles(
                json_sub_query,
                proxy_handle,
                local_country,
                persistence_store,
            );
        } else if let Some(html_sub_query) =
            sub_query.as_any_mut().downcast_mut::<HtmlScraperSubQuery>()
        {
            bind_html_sub_query_runtime_handles(
                html_sub_query,
                proxy_handle,
                local_country,
                persistence_store,
            );
        }
    }
}

/// Returns the URL query-parameter separator needed to append further parameters.
///
/// # Arguments
///
/// * `url` - URL string to inspect.
fn query_parameter_separator(url: &str) -> &'static str {
    let url_without_fragment = url.split('#').next().unwrap_or(url);

    if url_without_fragment.ends_with('?') || url_without_fragment.ends_with('&') {
        ""
    } else if url_without_fragment.contains('?') {
        "&"
    } else {
        "?"
    }
}

impl TryFrom<ScraperQueryCollectionRaw> for ScraperQueryCollection {
    type Error = anyhow::Error;

    /// Converts a raw YAML collection definition into a validated runtime collection.
    ///
    /// Resolves collection-level parameters and HTTP config into each query before
    /// converting them to their runtime variants.
    ///
    /// # Errors
    ///
    /// Returns an error if the collection id is empty, parameters are invalid,
    /// or any query definition fails to convert.
    fn try_from(config: ScraperQueryCollectionRaw) -> Result<Self> {
        let ScraperQueryCollectionRaw {
            id,
            title,
            logo,
            description,
            credentials: _,
            parameters,
            http,
            proxy_insecure_tls_hosts,
            mut queries,
        } = config;
        let id = id.trim().to_string();
        if id.is_empty() {
            anyhow::bail!("Collection id cannot be empty");
        }

        let title = title
            .map(|title| title.trim().to_string())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| id.clone());
        let logo = logo.unwrap_or_default();
        let mut parameter_defaults = Self::build_parameter_defaults(&parameters)?;
        parameter_defaults.extend(Self::build_service_metadata_defaults(
            &id,
            &title,
            &logo,
            &description,
        )?);

        for query in &mut queries {
            query.apply_collection_http(&http);
            query.resolve_collection_params(&parameter_defaults)?;
        }

        let queries = queries
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>>>()?;

        Ok(ScraperQueryCollection::new(
            &id,
            &title,
            &logo,
            description,
            parameters,
            proxy_insecure_tls_hosts,
            parameter_defaults,
            queries,
        ))
    }
}

impl From<&ScraperQueryCollection> for ScraperQueryCollectionRaw {
    /// Converts a runtime collection back into its raw YAML-compatible representation.
    fn from(collection: &ScraperQueryCollection) -> Self {
        let mut queries = collection
            .queries
            .values()
            .map(ScraperQueryDefinitionRaw::from)
            .collect::<Vec<_>>();
        queries.sort_by_key(|left| left.name());

        Self {
            id: collection.name.clone(),
            title: Some(collection.title.clone()),
            logo: if collection.logo.is_empty() {
                None
            } else {
                Some(collection.logo.clone())
            },
            description: collection.description.clone(),
            credentials: None,
            parameters: collection.parameters.clone(),
            http: ScraperHttpConfig::default(),
            proxy_insecure_tls_hosts: collection.proxy_insecure_tls_hosts.clone(),
            queries,
        }
    }
}

impl Serialize for ScraperQueryCollection {
    /// Serializes the collection through its raw YAML representation.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ScraperQueryCollectionRaw::from(self).serialize(serializer)
    }
}

#[cfg(test)]
mod tests {

    use anyhow::Context;
    /// Verifies that the YAML configuration can be deserialized
    /// into a query collection and serialized back as JSON.
    #[test]
    fn config_yaml_to_json() -> super::Result<()> {
        let services_path = format!(
            "{}/services",
            arachnea_core::application::get_application_root()
        );

        for entry in std::fs::read_dir(services_path)? {
            let path = entry?.path();
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.'))
            {
                continue;
            }
            if path.extension().and_then(|value| value.to_str()) != Some("yaml") {
                continue;
            }

            let collection = super::ScraperQueryCollection::from_file_with(
                path.as_path(),
                serde_yaml::from_reader,
            )
            .with_context(|| format!("while parsing {}", path.display()))?;
            println!("Parsed service config: {}", collection.name());
        }

        // TODO: Add check result
        Ok(())
    }

    #[tokio::test]
    async fn static_entry_actions_are_applied() -> super::Result<()> {
        let yaml = r#"
id: static-action-test
title: Static Action Test
parameters:
  - name: base_url
    value: https://example.test
queries:
  - name: service_stream_metadata
    scraper_type: static
    entries:
      - name: logo
        type: string
        value: "{base_url}/logo.png"
        actions:
          - type: resolve_url
            proxy: true
"#;
        let collection: super::ScraperQueryCollection = serde_yaml::from_str(yaml)?;
        let params = std::collections::HashMap::from([(
            crate::scrapyfy::HTTP_PROXY_PUBLIC_PATH_PARAM.to_string(),
            "/api/proxy".to_string(),
        )]);

        let rows = collection
            .execute_query("service_stream_metadata", &params, None, None)
            .await?;
        let logo = rows
            .first()
            .and_then(|row| row.get("logo"))
            .and_then(|node| node.values.first())
            .cloned();

        assert_eq!(
            logo.as_deref(),
            Some("/api/proxy/https://example.test/logo.png")
        );

        Ok(())
    }
}
