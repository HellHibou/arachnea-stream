#[path = "assertions.rs"]
pub mod assertions;
#[path = "yaml.rs"]
mod yaml;
pub use yaml::read_strict_yaml;

use super::*;
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json;
use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::io::ErrorKind;
use std::pin::Pin;

struct StatsTest {
    count: usize,
}

impl StatsTest {
    fn new() -> Self {
        StatsTest { count: 0 }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestQueryCollection {
    #[serde(alias = "query")]
    queries: Vec<TestQueryConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestQueryConfig {
    name: String,
    #[serde(default)]
    fields_result: Vec<String>,
    #[serde(default)]
    fields_optional: Vec<String>,
}

struct TestExpectedFields {
    required: Vec<String>,
    optional: Vec<String>,
}

/// Test configuration parameters for scraper query tests.
#[derive(Default)]
pub struct TestParams {
    /// When enabled, uses mock HTML files instead of making real HTTP requests.
    pub use_mock_file: bool,

    /// Treats wholly absent expected fields as warnings, but never permits empty
    /// extraction or the absence of every useful expected field.
    pub ignore_entry_not_mapped: bool,

    /// When enabled, logs the full response data in JSON format.
    pub log_response: bool,
}

fn load_expected_fields_from_test_data(
    query_source: &str,
    query: &str,
) -> Result<Option<TestExpectedFields>> {
    let yaml_file = format!(
        "{}/{}/{}.yaml",
        arachnea_core::application::get_application_root(),
        TEST_DATA_FOLDER,
        query_source
    );

    let file = match std::fs::File::open(&yaml_file) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Unable to open test data file `{}`", yaml_file));
        }
    };

    let config: TestQueryCollection = read_strict_yaml(file)
        .with_context(|| format!("Unable to parse test data file `{}`", yaml_file))?;

    let mut names = HashSet::new();
    for entry in &config.queries {
        anyhow::ensure!(
            !entry.name.trim().is_empty() && names.insert(&entry.name),
            "Blank or duplicate query name in `{yaml_file}`"
        );
        let mut fields = HashSet::new();
        for field in entry.fields_result.iter().chain(&entry.fields_optional) {
            anyhow::ensure!(
                field.split('>').all(|segment| !segment.trim().is_empty()) && fields.insert(field),
                "Blank, duplicate or conflicting field `{field}` in query `{}` of `{yaml_file}`",
                entry.name
            );
        }
        anyhow::ensure!(
            !entry.fields_result.is_empty(),
            "Query `{}` in `{yaml_file}` requires fields_result",
            entry.name
        );
    }

    let query_config = config
        .queries
        .into_iter()
        .find(|entry| entry.name == query)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Query `{}` not found in test data file `{}`",
                query,
                yaml_file
            )
        })?;

    Ok(Some(TestExpectedFields {
        required: query_config.fields_result,
        optional: query_config.fields_optional,
    }))
}

/// Executes one configured query and validates nonempty field-level extraction.
///
/// Explicit legacy expectations are loaded strictly when present. The production
/// fallback remains until source scenarios migrate to independent v1 contracts.
/// Optional and newly discovered fields are diagnostic, not uniformity requirements.
pub fn test_query<M, F>(
    manager: &mut M,
    groupe_name: &str,
    query: &str,
    test_params: TestParams,
    yaml_file: &str,
    callback: F,
) -> Result<()>
where
    M: ScraperManager,
    F: for<'a> Fn(
        &'a mut M,
    ) -> Pin<
        Box<dyn Future<Output = Result<Vec<HashMap<String, ScraperDataNode>>>> + 'a>,
    >,
{
    fn run_query_future<T>(future: impl Future<Output = Result<T>>) -> Result<T> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(future)
    }

    let yaml_path = format!(
        "{}/{}/{}",
        arachnea_core::application::get_application_root(),
        DEFAULT_SERVICES_DIRECTORY,
        yaml_file
    );

    // === SETUP BLOCK (mutable borrow ends here) ===
    let (fallback_expected_fields, query_source) = {
        let scraper_agregator = manager.get_scraper_agregator_mut();

        scraper_agregator.add_query_collection_from_files_yaml(groupe_name, vec![yaml_path])?;

        let query_collection = scraper_agregator
            .get_last_query_collection(groupe_name)
            .ok_or_else(|| anyhow::anyhow!("No query collection loaded"))?;

        let query_source = query_collection.name().to_string();

        let query_exec = query_collection
            .get_query(query)
            .ok_or_else(|| anyhow::anyhow!("Query {} not found in {}", query, query_source))?;

        (query_exec.get_field_names(), query_source)
    };

    let expected_fields_from_test_data = load_expected_fields_from_test_data(&query_source, query)?;
    let strict_expected_fields = expected_fields_from_test_data.is_some();
    let expected_fields = expected_fields_from_test_data.unwrap_or(TestExpectedFields {
        required: fallback_expected_fields,
        optional: Vec::new(),
    });

    // === MOCK SETUP ===
    if test_params.use_mock_file {
        init_mock(&query_source, query);
    } else {
        http_client::remove_router();
    }

    scopeguard::defer! {
        http_client::remove_router();
    }

    // === CALLBACK ===
    let results = run_query_future(callback(manager))
        .with_context(|| format!("Source `{query_source}`, query `{query}` failed"))?;
    anyhow::ensure!(!results.is_empty(),
        "extraction_failed: source `{query_source}`, query `{query}` returned 0 rows; expected at least 1");

    if test_params.log_response {
        if test_params.use_mock_file {
            println!(
                "{} => {}/{}-{}.html: {}",
                query,
                MOCK_DATA_FOLDER,
                query_source,
                query,
                serde_json::to_string_pretty(&results)?
            );
        } else {
            println!("{}: {}", query, serde_json::to_string_pretty(&results)?);
        }
    }

    // === STATS ANALYSIS ===
    let mut stats: HashMap<String, StatsTest> = HashMap::new();
    let total = results.len();

    println!("Total row: {}\nFields count:", total);

    for row in results {
        let mut row_fields = HashSet::new();
        collect_fields(&mut row_fields, &"".to_string(), &row);
        for field in row_fields {
            let stat_entry = stats.entry(field).or_insert_with(StatsTest::new);
            stat_entry.count += 1;
        }
    }

    let expected_fields_set: HashSet<&str> = expected_fields
        .required
        .iter()
        .chain(expected_fields.optional.iter())
        .map(String::as_str)
        .collect();
    anyhow::ensure!(expected_fields.required.iter().any(|field| {
        field != "source" && field != "id" && stats.contains_key(field.as_str())
    }), "extraction_failed: source `{query_source}`, query `{query}` has no useful expected fields in {total} rows");
    let optional_fields: HashSet<&str> = expected_fields
        .optional
        .iter()
        .map(String::as_str)
        .collect();
    let mut not_mapped: Vec<String> = expected_fields
        .required
        .iter()
        .filter(|name| !stats.contains_key(name.as_str()))
        .cloned()
        .collect();
    not_mapped.sort();

    let mut bad_data_format = 0;
    let mut new_fields = 0;
    let mut skipped = 0;

    let mut stats_entries: Vec<_> = stats.iter().collect();
    stats_entries.sort_by_key(|(left_key, _)| *left_key);

    for (stat_key, stat_value) in stats_entries {
        let equal_str = if optional_fields.contains(stat_key.as_str()) {
            "Optional".to_string()
        } else if strict_expected_fields && !expected_fields_set.contains(stat_key.as_str()) {
            new_fields += 1;
            "New field".to_string()
        } else if stat_value.count == total {
            "OK".to_string()
        } else {
            bad_data_format += 1;
            format!("Warning: {} != {}", stat_value.count, total)
        };
        println!(" - {}: {} - {}", stat_key, stat_value.count, equal_str);
    }

    for entry in not_mapped {
        println!(" - {}: 0 - Not mapped", entry);

        if test_params.ignore_entry_not_mapped {
            skipped += 1;
        } else {
            bad_data_format += 1;
        }
    }

    if bad_data_format == 0 {
        println!(
            "Test count field: OK - {:2} skipped, {:2} new fields",
            skipped, new_fields
        );
    }

    if bad_data_format > 0 {
        anyhow::bail!(
            "contract_failed: source `{query_source}`, query `{query}`, {total} rows: {:2} field errors, {:2} skipped, {:2} new fields",
            bad_data_format,
            skipped,
            new_fields
        );
    }

    Ok(())
}

fn collect_fields(
    fields: &mut HashSet<String>,
    root: &String,
    row: &HashMap<String, ScraperDataNode>,
) {
    for (entry_key, entry_value) in row.iter() {
        if entry_value
            .values
            .iter()
            .any(|value| !value.trim().is_empty())
        {
            fields.insert(format!("{}{}", root, entry_key));
        }

        if !entry_value.children.is_empty() {
            let new_root = format!("{}{} > ", root, entry_key);
            collect_fields(fields, &new_root, &entry_value.children);
        }

        if !entry_value.items.is_empty() {
            let new_root = format!("{}{} > ", root, entry_key);
            for item in &entry_value.items {
                collect_fields(fields, &new_root, &item.children);
            }
        }
    }
}

/// Executes a query test and records failures.
///
/// # Arguments
///
/// * `failures` - Vector to accumulate failure messages
/// * `query_source` - The source name for the query
/// * `query_name` - The name of the query being tested
/// * `yaml_file` - Optional YAML configuration file path
/// * `run_query` - Function that executes the query test
///
/// # Errors
///
/// Any errors from the `run_query` function are caught and added to the failures vector.
pub fn assert_query_succeeds<F>(
    failures: &mut Vec<String>,
    query_source: &str,
    query_name: &str,
    yaml_file: Option<&str>,
    run_query: F,
) where
    F: FnOnce(&str, Option<&str>) -> Result<()>,
{
    println!("Test {} for source `{}`...", query_name, query_source);
    let result = run_query(query_source, yaml_file);
    if let Err(error) = result {
        println!("{error:#}");
        failures.push(format!(
            "| Source: {query_source:15} | Query: {query_name:25} | {error:#}"
        ));
    }

    println!("\n--------------------------------------------------------------\n");
}
