use anyhow::{Context, Result};
use arachnea_core::persistence::TypedEntityStore;
use arachnea_http::chaser_session::{memory_session_store, CachedChaserSession};
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
        self.set_runtime_handles_with_session_store(
            proxy_handle,
            local_country,
            memory_session_store().expect("in-memory Cloudflare session store is valid"),
        );
    }

    /// Rebinds all embedded HTTP clients to the provided shared runtime handles
    /// and persistence store.
    pub fn set_runtime_handles_with_session_store(
        &mut self,
        proxy_handle: SharedProxyConfigHandle,
        local_country: SharedLocalCountry,
        session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    ) {
        match self {
            ScraperQueryDefinition::Html(query) => bind_html_query_runtime_handles(
                query,
                &proxy_handle,
                &local_country,
                &session_store,
            ),
            ScraperQueryDefinition::Json(query) => bind_json_query_runtime_handles(
                query,
                &proxy_handle,
                &local_country,
                &session_store,
            ),
            ScraperQueryDefinition::Static(query) => query.set_runtime_handles_with_session_store(
                proxy_handle,
                local_country,
                session_store,
            ),
            ScraperQueryDefinition::Text(query) => bind_text_query_runtime_handles(
                query,
                &proxy_handle,
                &local_country,
                &session_store,
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

    /// Returns whether this source declares a query with the supplied name.
    ///
    /// # Arguments
    ///
    /// * `query_name` - Name of the query to look up.
    pub fn has_query(&self, query_name: &str) -> bool {
        self.queries.contains_key(query_name)
    }

    /// Returns the sorted query names supported by this loaded source.
    pub fn query_names(&self) -> Vec<String> {
        let mut names = self.queries.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }

    /// Records the base62 hash of the raw source YAML document.
    pub(crate) fn set_yaml_hash(&mut self, yaml_hash: String) -> &mut Self {
        self.yaml_hash = yaml_hash;
        self
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
        self.set_runtime_handles_with_session_store(
            proxy_handle,
            local_country,
            memory_session_store().expect("in-memory Cloudflare session store is valid"),
        );
    }

    /// Rebinds all query HTTP clients to the provided shared runtime handles
    /// and persistence store.
    pub fn set_runtime_handles_with_session_store(
        &mut self,
        proxy_handle: SharedProxyConfigHandle,
        local_country: SharedLocalCountry,
        session_store: Arc<dyn TypedEntityStore<CachedChaserSession>>,
    ) {
        for query in self.queries.values_mut() {
            query.set_runtime_handles_with_session_store(
                proxy_handle.clone(),
                local_country.clone(),
                session_store.clone(),
            );
        }
    }
}

fn bind_text_query_runtime_handles(
    query: &mut TextScraperQuery,
    proxy_handle: &SharedProxyConfigHandle,
    local_country: &SharedLocalCountry,
    session_store: &Arc<dyn TypedEntityStore<CachedChaserSession>>,
) {
    query.http_client =
        HttpClient::with_http_config_proxy_handle_and_local_country_and_session_store(
            query.http_config.clone(),
            proxy_handle.clone(),
            local_country.clone(),
            session_store.clone(),
        );
}

fn bind_html_query_runtime_handles(
    query: &mut HtmlScraperQuery,
    proxy_handle: &SharedProxyConfigHandle,
    local_country: &SharedLocalCountry,
    session_store: &Arc<dyn TypedEntityStore<CachedChaserSession>>,
) {
    query.http_client =
        HttpClient::with_http_config_proxy_handle_and_local_country_and_session_store(
            query.http_config.clone(),
            proxy_handle.clone(),
            local_country.clone(),
            session_store.clone(),
        );

    for sub_query in &mut query.sub_queries {
        if let Some(html_sub_query) = sub_query.as_any_mut().downcast_mut::<HtmlScraperSubQuery>() {
            bind_html_sub_query_runtime_handles(
                html_sub_query,
                proxy_handle,
                local_country,
                session_store,
            );
        } else if let Some(json_sub_query) =
            sub_query.as_any_mut().downcast_mut::<JsonScraperSubQuery>()
        {
            bind_json_sub_query_runtime_handles(
                json_sub_query,
                proxy_handle,
                local_country,
                session_store,
            );
        }
    }
}

fn bind_html_sub_query_runtime_handles(
    query: &mut HtmlScraperSubQuery,
    proxy_handle: &SharedProxyConfigHandle,
    local_country: &SharedLocalCountry,
    session_store: &Arc<dyn TypedEntityStore<CachedChaserSession>>,
) {
    query.http_client =
        HttpClient::with_http_config_proxy_handle_and_local_country_and_session_store(
            query.http_config.clone(),
            proxy_handle.clone(),
            local_country.clone(),
            session_store.clone(),
        );
}

fn bind_json_query_runtime_handles(
    query: &mut JsonScraperQuery,
    proxy_handle: &SharedProxyConfigHandle,
    local_country: &SharedLocalCountry,
    session_store: &Arc<dyn TypedEntityStore<CachedChaserSession>>,
) {
    query.http_client =
        HttpClient::with_http_config_proxy_handle_and_local_country_and_session_store(
            query.http_config.clone(),
            proxy_handle.clone(),
            local_country.clone(),
            session_store.clone(),
        );

    for sub_query in &mut query.sub_queries {
        if let Some(json_sub_query) = sub_query.as_any_mut().downcast_mut::<JsonScraperSubQuery>() {
            bind_json_sub_query_runtime_handles(
                json_sub_query,
                proxy_handle,
                local_country,
                session_store,
            );
        } else if let Some(html_sub_query) =
            sub_query.as_any_mut().downcast_mut::<HtmlScraperSubQuery>()
        {
            bind_html_sub_query_runtime_handles(
                html_sub_query,
                proxy_handle,
                local_country,
                session_store,
            );
        }
    }
}

fn bind_json_sub_query_runtime_handles(
    query: &mut JsonScraperSubQuery,
    proxy_handle: &SharedProxyConfigHandle,
    local_country: &SharedLocalCountry,
    session_store: &Arc<dyn TypedEntityStore<CachedChaserSession>>,
) {
    query.http_client =
        HttpClient::with_http_config_proxy_handle_and_local_country_and_session_store(
            query.http_config.clone(),
            proxy_handle.clone(),
            local_country.clone(),
            session_store.clone(),
        );

    for sub_query in &mut query.sub_queries {
        if let Some(json_sub_query) = sub_query.as_any_mut().downcast_mut::<JsonScraperSubQuery>() {
            bind_json_sub_query_runtime_handles(
                json_sub_query,
                proxy_handle,
                local_country,
                session_store,
            );
        } else if let Some(html_sub_query) =
            sub_query.as_any_mut().downcast_mut::<HtmlScraperSubQuery>()
        {
            bind_html_sub_query_runtime_handles(
                html_sub_query,
                proxy_handle,
                local_country,
                session_store,
            );
        }
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
#[path = "../../tests/scrapyfy/scraper_query_collection.rs"]
mod tests;

#[cfg(any(test, feature = "test-support"))]
#[path = "../../tests/support/scrapyfy/scraper_query_collection.rs"]
mod test_support;

mod execution;

mod collection_execution;

mod parameters;
