use super::*;
use arachnea_core::controler::ControlerService;

const LOGGERS: [&str; 3] = [
    "selectors::matching",
    "html5ever::tree_builder",
    "html5ever::tokenizer",
];

/// Contract exposing mutable access to the shared scraper aggregator.
pub trait ScraperManager {

    /// Returns a mutable reference to the underlying query aggregator.
    fn get_scraper_agregator_mut(&mut self) -> &mut ScraperAgregator;

    /// Registers the scraper's functions with the provided controler service.
    ///
    /// # Arguments
    ///
    /// * `controler` - The controler service to register the functions with.
    fn register_service(self, controler: &mut dyn ControlerService)
    where
        Self: Sized;

    
    /// Sets default log levels for carte dependencies.
    /// Must be called before `init_logger()` to take effect.
    fn init_sub_logger_levels() {
        for logger in LOGGERS {
            arachnea_core::logger::set_logger_levels(logger,
                Level::ERROR,
                Level::WARN,
                Level::INFO,
                Level::INFO,
                Level::TRACE,
            );
        }
    }

    /// Sets the log levels for carte dependencies.
    /// Must be called before `init_logger()` to take effect.
    ///
    /// # Arguments
    /// * `target` - The logger target name
    /// * `level` - Level to use
    /// 
    /// # Examples
    /// 
    /// ScraperManager::init_sub_logger_level(
    ///     "my_crate",
    ///     Level::INFO
    /// );
    /// ```
    fn init_sub_logger_level(level: Level) {
        for logger in LOGGERS {
            arachnea_core::logger::set_logger_level(logger, level);
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
use arachnea_core::persistence::resources;
use tracing::Level;
#[cfg(any(test, feature = "test-support"))]
use tracing::debug;

#[cfg(any(test, feature = "test-support"))]
static MOCK_DATA_FOLDER: &str = "mock_data";

#[cfg(any(test, feature = "test-support"))]
static TEST_DATA_FOLDER: &str = "data-test";

#[cfg(any(test, feature = "test-support"))]
/// Installs a file-backed mock router resolving `<source>-<query>.html` fixtures.
pub fn init_mock(query_source: &str, query: &str) {
    let query_str = query.to_string();
    let query_source_str = query_source.to_string();

    http_client::set_router(move |_client, _route| {
        let file = format!(
            "{}/{}/{}-{}.html",
            resources::get_application_root(),
            MOCK_DATA_FOLDER,
            query_source_str,
            query_str
        );

        debug!("{}", file);
        Ok(std::fs::read_to_string(file)?)
    });
}


#[cfg(any(test, feature = "test-support"))]
/// Test helpers used by scraper query integration tests.
pub mod tests {

    use super::*;
    use anyhow::{Context, Result};
    use arachnea_core::persistence::resources;
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
    struct TestQueryCollection {
        #[serde(default, alias = "query")]
        queries: Vec<TestQueryConfig>,
    }

    #[derive(Deserialize)]
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

    pub struct TestParams {
        // When enabled, tests do not fail on expected fields absent in extracted output.
        pub use_mock_file: bool,

        pub ignore_entry_not_mapped: bool,

        pub log_response: bool,
    }

    impl Default for TestParams {
        fn default() -> TestParams {
            TestParams {
                use_mock_file: false,
                ignore_entry_not_mapped: false,
                log_response: false,
            }
        }
    }

    fn dedup_fields(fields: Vec<String>) -> Vec<String> {
        let mut deduped_fields = Vec::new();
        let mut seen_fields = HashSet::new();

        for field in fields {
            if seen_fields.insert(field.clone()) {
                deduped_fields.push(field);
            }
        }

        deduped_fields
    }

    fn load_expected_fields_from_test_data(
        query_source: &str,
        query: &str,
    ) -> Result<Option<TestExpectedFields>> {
        let yaml_file = format!(
            "{}/{}/{}.yaml",
            resources::get_application_root(),
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

        let config: TestQueryCollection = serde_yaml::from_reader(file)
            .with_context(|| format!("Unable to parse test data file `{}`", yaml_file))?;

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

        if query_config.fields_result.is_empty() {
            anyhow::bail!(
                "Query `{}` in test data file `{}` does not define any `fields_result`",
                query,
                yaml_file
            );
        }

        Ok(Some(TestExpectedFields {
            required: dedup_fields(query_config.fields_result),
            optional: dedup_fields(query_config.fields_optional),
        }))
    }

    /// Executes one configured query and validates field-level extraction coverage.
    pub fn test_query<M, F>(
        manager: &mut M,
        query_source: &str,
        query: &str,
        test_params: TestParams,
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

        let yaml_file = format!(
            "{}/services/{}.yaml",
            resources::get_application_root(),
            query_source
        );

        // === SETUP BLOCK (mutable borrow ends here) ===
        let fallback_expected_fields: Vec<String> = {
            let scraper_agregator = manager.get_scraper_agregator_mut();

            scraper_agregator.add_query_collection_from_files_yaml(vec![yaml_file])?;

            let query_collection = scraper_agregator
                .get_query_collection(query_source)
                .ok_or_else(|| anyhow::anyhow!("Query Collection not found: {}", query_source))?;

            let query_exec = query_collection
                .get_query(query)
                .ok_or_else(|| anyhow::anyhow!("Query {} not found in {}", query, query_source))?;

            dedup_fields(query_exec.get_field_names())
        };

        let expected_fields_from_test_data =
            load_expected_fields_from_test_data(query_source, query)?;
        let strict_expected_fields = expected_fields_from_test_data.is_some();
        let expected_fields = expected_fields_from_test_data.unwrap_or(TestExpectedFields {
            required: fallback_expected_fields,
            optional: Vec::new(),
        });

        // === MOCK SETUP ===
        if test_params.use_mock_file {
            init_mock(query_source, query);
        } else {
            http_client::remove_router();
        }

        scopeguard::defer! {
            http_client::remove_router();
        }

        // === CALLBACK ===
        let results = run_query_future(callback(manager))?;

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
                let stat_entry = stats.entry(field).or_insert_with(|| StatsTest::new());
                stat_entry.count += 1;
            }
        }

        let expected_fields_set: HashSet<&str> = expected_fields
            .required
            .iter()
            .chain(expected_fields.optional.iter())
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
        stats_entries.sort_by(|(left_key, _), (right_key, _)| left_key.cmp(right_key));

        for (stat_key, stat_value) in stats_entries {
            let equal_str =
                if strict_expected_fields && !expected_fields_set.contains(stat_key.as_str()) {
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
                "Test count field: OK - {} skipped, {} new fields",
                skipped, new_fields
            );
        }

        if bad_data_format > 0 {
            anyhow::bail!(
                "Test count field: {} errors, {} skipped, {} new fields",
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
            if entry_value.values.len() > 0 {
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

    pub fn assert_query_succeeds<F>(
        failures: &mut Vec<String>,
        query_source: &str,
        query_name: &str,
        run_query: F,
    ) where
        F: FnOnce(&str) -> Result<()>,
    {
        println!("Test {} for source `{}`...", query_name, query_source);
        let result = run_query(query_source);
        if let Err(error) = result {
            failures.push(format!(
                "{query_name} failed for source `{query_source}`: {error:?}"
            ));
        }

        println!("\n--------------------------------------------------------------\n");
    }
}
