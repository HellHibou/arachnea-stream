use anyhow::{Context, Result};
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;
use std::fs;
use std::io::{BufReader, Read};
use std::path::Path;

use super::*;

/// One collection-level default parameter available to every query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScraperQueryCollectionParameter {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub description: Option<String>,
}

impl ScraperQueryCollectionParameter {
    fn validate_and_normalize(&self) -> Result<(String, String)> {
        let parameter_name = self.name.trim();
        if parameter_name.is_empty() {
            anyhow::bail!("Collection parameter name cannot be empty");
        }

        // Description is now optional
        Ok((parameter_name.to_string(), self.value.clone()))
    }
}

/// Raw configuration structure describing the queries available for one source.
#[derive(Serialize, Deserialize)]
pub struct ScraperQueryCollectionRaw {
    #[serde(alias = "name")]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub description: HashMap<String, String>,
    #[serde(default, alias = "parametres", skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ScraperQueryCollectionParameter>,
    #[serde(default, skip_serializing_if = "ScraperHttpConfig::is_empty")]
    pub http: ScraperHttpConfig,
    pub queries: Vec<ScraperQueryDefinitionRaw>,
}

/// Raw configuration wrapper supporting both HTML and JSON query definitions.
#[derive(Serialize, Deserialize)]
#[serde(tag = "scraper_type", rename_all = "snake_case")]
pub enum ScraperQueryDefinitionRaw {
    Html {
        #[serde(flatten)]
        query: HtmlScraperQueryRaw,
    },
    Json {
        #[serde(flatten)]
        query: JsonScraperQueryRaw,
    },
    Static {
        #[serde(flatten)]
        query: StaticScraperQueryRaw,
    },
}

impl ScraperQueryDefinitionRaw {
    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        match self {
            ScraperQueryDefinitionRaw::Html { query } => query.name(),
            ScraperQueryDefinitionRaw::Json { query } => query.name(),
            ScraperQueryDefinitionRaw::Static { query } => query.name(),
        }
    }

    fn resolve_collection_params(&mut self, params: &HashMap<String, String>) -> Result<()> {
        match self {
            ScraperQueryDefinitionRaw::Html { query } => query.resolve_collection_params(params),
            ScraperQueryDefinitionRaw::Json { query } => query.resolve_collection_params(params),
            ScraperQueryDefinitionRaw::Static { query } => query.resolve_collection_params(params),
        }
    }

    fn apply_collection_http(&mut self, http: &ScraperHttpConfig) {
        match self {
            ScraperQueryDefinitionRaw::Html { query } => query.apply_collection_http(http),
            ScraperQueryDefinitionRaw::Json { query } => query.apply_collection_http(http),
            ScraperQueryDefinitionRaw::Static { .. } => {}
        }
    }
}

/// Runtime query wrapper supporting both HTML and JSON scraping backends.
#[derive(Deserialize)]
#[serde(try_from = "ScraperQueryDefinitionRaw")]
pub enum ScraperQueryDefinition {
    Html(HtmlScraperQuery),
    Json(JsonScraperQuery),
    Static(StaticScraperQuery),
}

impl ScraperQueryDefinition {
    /// Returns the query name used as the lookup key in a collection.
    pub fn name(&self) -> String {
        match self {
            ScraperQueryDefinition::Html(query) => query.name(),
            ScraperQueryDefinition::Json(query) => query.name(),
            ScraperQueryDefinition::Static(query) => query.name(),
        }
    }

    /// Returns whether the query matches at least one requested media type.
    pub fn is_media_type(&self, media_types: &[String]) -> bool {
        match self {
            ScraperQueryDefinition::Html(query) => query.is_media_type(media_types),
            ScraperQueryDefinition::Json(query) => query.is_media_type(media_types),
            ScraperQueryDefinition::Static(query) => query.is_media_type(media_types),
        }
    }

    /// Returns the flattened list of leaf field names produced by this query.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_field_names(&self) -> Vec<String> {
        match self {
            ScraperQueryDefinition::Html(query) => query.get_field_names(),
            ScraperQueryDefinition::Json(query) => query.get_field_names(),
            ScraperQueryDefinition::Static(query) => query.get_field_names(),
        }
    }

    /// Executes the query and returns the extracted rows.
    pub async fn execute_query(
        &self,
        params: &HashMap<String, String>,
        fields_filters: Option<&HashMap<String, Vec<String>>>,
    ) -> Result<Vec<HashMap<String, ScraperDataNode>>> {
        let (rows, result_item_field) = match self {
            ScraperQueryDefinition::Html(query) => (
                query.execute_query(params, fields_filters).await?,
                query.result_item_field(),
            ),
            ScraperQueryDefinition::Json(query) => (
                query.execute_query(params, fields_filters).await?,
                query.result_item_field(),
            ),
            ScraperQueryDefinition::Static(query) => {
                (query.execute_query(params, fields_filters).await?, None)
            }
        };

        Ok(match result_item_field {
            Some(result_item_field) => flatten_result_item_field(rows, result_item_field),
            None => rows,
        })
    }
}

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

    fn try_from(config: ScraperQueryDefinitionRaw) -> Result<Self> {
        match config {
            ScraperQueryDefinitionRaw::Html { query } => Ok(Self::Html(query.try_into()?)),
            ScraperQueryDefinitionRaw::Json { query } => Ok(Self::Json(query.try_into()?)),
            ScraperQueryDefinitionRaw::Static { query } => Ok(Self::Static(query.try_into()?)),
        }
    }
}

impl From<&ScraperQueryDefinition> for ScraperQueryDefinitionRaw {
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
        }
    }
}

impl Serialize for ScraperQueryDefinition {
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
    parameter_defaults: HashMap<String, String>,
    queries: HashMap<String, ScraperQueryDefinition>,
}

impl ScraperQueryCollection {
    /// Builds a query collection indexed by query name.
    ///
    /// # Arguments
    ///
    /// * `name` - Collection name, typically matching the source identifier.
    /// * `parameters` - Collection-level default parameters merged into every execution.
    /// * `parameter_defaults` - Validated parameter defaults indexed by parameter name.
    /// * `queries` - Query definitions indexed by their individual names.
    pub fn new(
        name: &str,
        title: &str,
        logo: &str,
        description: HashMap<String, String>,
        parameters: Vec<ScraperQueryCollectionParameter>,
        parameter_defaults: HashMap<String, String>,
        queries: Vec<ScraperQueryDefinition>,
    ) -> Self {
        let mut collection = Self {
            name: name.to_string(),
            title: title.to_string(),
            logo: logo.to_string(),
            description,
            parameters,
            parameter_defaults,
            queries: HashMap::new(),
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

    #[cfg(any(test, feature = "test-support"))]
    /// Returns one query definition by name for test assertions.
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
        let mut merged_params = self.build_execution_params(params);
        merged_params
            .entry("source".to_string())
            .or_insert_with(|| self.name.clone());

        match self.queries.get(query_name) {
            Some(value) => {
                if query_media_type_filter.is_none()
                    || value.is_media_type(query_media_type_filter.unwrap())
                {
                    value.execute_query(&merged_params, fields_filters).await
                } else {
                    Ok(Vec::new())
                }
            }
            _none => Ok(Vec::new()),
        }
    }

    fn build_parameter_defaults(
        parameters: &[ScraperQueryCollectionParameter],
    ) -> Result<HashMap<String, String>> {
        let mut defaults = HashMap::new();

        for parameter in parameters {
            let (parameter_name, parameter_value) = parameter.validate_and_normalize()?;

            if defaults
                .insert(parameter_name.clone(), parameter_value)
                .is_some()
            {
                anyhow::bail!("Duplicate collection parameter: {}", parameter_name);
            }
        }

        Ok(defaults)
    }

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

        merged_params
    }

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

    fn try_from(config: ScraperQueryCollectionRaw) -> Result<Self> {
        let ScraperQueryCollectionRaw {
            id,
            title,
            logo,
            description,
            parameters,
            http,
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
            parameter_defaults,
            queries,
        ))
    }
}

impl From<&ScraperQueryCollection> for ScraperQueryCollectionRaw {
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
            parameters: collection.parameters.clone(),
            http: ScraperHttpConfig::default(),
            queries,
        }
    }
}

impl Serialize for ScraperQueryCollection {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ScraperQueryCollectionRaw::from(self).serialize(serializer)
    }
}

#[cfg(test)]
mod tests {

    use arachnea_core::persistence::resources;

    /// Verifies that the YAML configuration can be deserialized
    /// into a query collection and serialized back as JSON.
    #[test]
    fn config_yaml_to_json() -> super::Result<()> {
        let services_path = format!("{}/services", resources::get_application_root());

        for entry in std::fs::read_dir(services_path)? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("yaml") {
                continue;
            }

            let collection =
                super::ScraperQueryCollection::from_file_with(path, serde_yaml::from_reader)?;
            println!("Parsed service config: {}", collection.name());
        }

        // TODO: Add check result
        Ok(())
    }
}
