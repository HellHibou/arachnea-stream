use super::StreamScraper;
use crate::stream_scraper::DEFAULT_SERVICES_CONFIG_PATH;
use crate::stream_scraper::STREAM_SERVICE_GROUP_NAME;
use anyhow::Result;
use arachnea_core::persistence::resources;
use arachnea_scrapyfy::scrapyfy::scraper_data_node::ScraperDataNode;
use arachnea_scrapyfy::scrapyfy::scraper_manager::tests::assert_query_succeeds;
use arachnea_scrapyfy::scrapyfy::scraper_manager::tests::test_query;
use arachnea_scrapyfy::scrapyfy::scraper_manager::tests::TestParams;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

static DEFAULT_SEARCH_TERM: &str = "inf";
static DEFAULT_QUERY_SOURCE: &str = "arachnea-stream/dark-stream/anime-sama.yaml";

#[derive(Deserialize)]
struct ServiceConfig {
    path: String,
    enabled: bool,
    #[allow(dead_code)]
    parameters: Option<Vec<HashMap<String, String>>>,
}

fn test_params() -> TestParams {
    TestParams {
        use_mock_file: false,
        ignore_entry_not_mapped: false,
        log_response: false,
    }
}

fn query_source() -> String {
    std::env::var("ARACHNEA_TEST_QUERY_SOURCE").unwrap_or_else(|_| DEFAULT_QUERY_SOURCE.to_string())
}

fn search_term() -> String {
    std::env::var("ARACHNEA_TEST_SEARCH_TERM").unwrap_or_else(|_| DEFAULT_SEARCH_TERM.to_string())
}

fn get_entry_url() -> Option<String> {
    std::env::var("ARACHNEA_TEST_ENTRY_URL").ok()
}

#[test]
fn test_all_services() {
    let enabled_services = load_enabled_services();
    let enabled_services_refs: Vec<&str> = enabled_services.iter().map(|s| s.as_str()).collect();
    test_all(enabled_services_refs);
}

#[test]
fn test_service() {
    test_all(Vec::from([query_source().as_str()]));
}
fn test_all(tests: Vec<&str>) {
    let mut failures = Vec::new();

    for yaml_file in tests {
        // Extract service name from yaml_file path for display purposes
        // e.g., "darkstream/coflix" -> "coflix" or "rtlplay-be" -> "rtlplay-be"
        let service_name = Path::new(yaml_file)
            .file_stem()
            .unwrap_or_else(|| yaml_file.as_ref())
            .to_string_lossy();

        assert_query_succeeds(
            &mut failures,
            &service_name,
            "service_stream_metadata",
            Some(yaml_file),
            |_, _| service_stream_metadata(yaml_file),
        );

        assert_query_succeeds(
            &mut failures,
            &service_name,
            "load_home",
            Some(yaml_file),
            |_, _| load_home(yaml_file),
        );

        assert_query_succeeds(
            &mut failures,
            &service_name,
            "search",
            Some(yaml_file),
            |_, _| search(search_term(), yaml_file),
        );

        assert_query_succeeds(
            &mut failures,
            &service_name,
            "get_entry",
            Some(yaml_file),
            |_, _| get_entry(get_entry_url(), yaml_file),
        );
    }

    assert!(
        failures.is_empty(),
        "test_all_services found {} failure(s):\n {}\n",
        failures.len(),
        failures.join("\n ")
    );
}

#[test]
pub fn test_query_service_stream_metadata() -> Result<()> {
    service_stream_metadata(query_source().as_str())
}
fn service_stream_metadata(yaml_file: &str) -> Result<()> {
    test_query(
        &mut StreamScraper::default(),
        STREAM_SERVICE_GROUP_NAME,
        "service_stream_metadata",
        test_params(),
        yaml_file,
        move |scraper| Box::pin(scraper.get_service()),
    )
}

#[test]
pub fn test_query_load_home() -> Result<()> {
    load_home(query_source().as_str())
}
fn load_home(yaml_file: &str) -> Result<()> {
    test_query(
        &mut StreamScraper::default(),
        STREAM_SERVICE_GROUP_NAME,
        "load_home",
        test_params(),
        yaml_file,
        move |scraper| Box::pin(scraper.load_home()),
    )
}

#[test]
pub fn test_query_search() -> Result<()> {
    search(search_term(), query_source().as_str())
}
fn search(search_term: String, yaml_file: &str) -> Result<()> {
    test_query(
        &mut StreamScraper::default(),
        STREAM_SERVICE_GROUP_NAME,
        "search",
        test_params(),
        yaml_file,
        move |scraper| {
            let search_term = search_term.clone();
            Box::pin(async move {
                let groups = scraper
                    .search(search_term, Vec::new(), Vec::new(), 1, Default::default())
                    .await?;

                Ok(groups
                    .into_iter()
                    .flat_map(|mut group| {
                        let source = group.get("source").cloned();

                        group
                            .remove("entries")
                            .map(|entries| {
                                entries
                                    .items
                                    .into_iter()
                                    .map(|item| {
                                        let mut item = item.children;
                                        if let Some(source) = source.clone() {
                                            item.entry("source".to_string()).or_insert(source);
                                        }
                                        item
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default()
                    })
                    .collect())
            })
        },
    )
}

#[test]
pub fn test_query_get_entry() -> Result<()> {
    get_entry(get_entry_url(), query_source().as_str())
}
fn get_entry(get_entry_url: Option<String>, yaml_file: &str) -> Result<()> {
    let yaml_file_owned = yaml_file.to_string();
    let yaml_file_for_test = yaml_file_owned.clone();
    test_query(
        &mut StreamScraper::default(),
        STREAM_SERVICE_GROUP_NAME,
        "get_entry",
        test_params(),
        &yaml_file_for_test,
        move |scraper| {
            let get_entry_url = get_entry_url.clone();
            let yaml_file_inner = yaml_file_owned.clone();
            Box::pin(async move {
                let entry_url;
                if get_entry_url.is_none() {
                    // We need to pass a source name to load_entry_url_from_home
                    // For now, extract it from yaml_file
                    let yaml_path = Path::new(&yaml_file_inner);
                    let source_name = yaml_path
                        .file_stem()
                        .unwrap_or_else(|| yaml_path.as_os_str())
                        .to_string_lossy();
                    entry_url = load_entry_url_from_home(scraper, &source_name).await?;
                } else {
                    entry_url = get_entry_url.clone().unwrap();
                }

                println!("Entry URL: {}", entry_url);
                // Use yaml_file without extension as source
                let source_path = Path::new(&yaml_file_inner).with_extension("");
                let source_name = source_path.to_string_lossy();
                scraper
                    .get_entry(source_name.to_string(), entry_url)
                    .await
                    .map(|entry| vec![entry])
            })
        },
    )
}

fn load_enabled_services() -> Vec<String> {
    let config_file = format!(
        "{}/{}",
        resources::get_application_root(),
        DEFAULT_SERVICES_CONFIG_PATH
    );
    let services_json_path = Path::new(config_file.as_str());
    let content =
        fs::read_to_string(services_json_path).expect("Failed to read services.json file");

    let services: Vec<ServiceConfig> =
        serde_json::from_str(&content).expect("Failed to parse services.json file");

    services
        .into_iter()
        .filter(|s| s.enabled)
        .map(|s| format!("{}/{}", STREAM_SERVICE_GROUP_NAME, s.path))
        .collect()
}

async fn load_entry_url_from_home(
    scraper: &mut StreamScraper,
    query_source: &str,
) -> Result<String> {
    let home_result: Vec<HashMap<String, ScraperDataNode>> = scraper.load_home().await?;

    let section: ScraperDataNode = home_result
        .iter()
        .find(|root| {
            root.get("source")
                .and_then(|source| source.values.first())
                .map(|source| source == query_source)
                .unwrap_or(false)
        })
        .and_then(|root| root.get("sections"))
        .and_then(|sections| sections.items.first())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Missing section in load_home response"))?;

    let entry = if let Some(entry) = section
        .children
        .get("entries")
        .and_then(|entries| entries.items.first())
        .cloned()
    {
        entry
    } else {
        let section_url = section
            .children
            .get("link")
            .and_then(|link| link.values.first())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Missing link in load_home section"))?;
        let section_result = scraper
            .get_section(query_source.to_string(), section_url, 1, Default::default())
            .await?;

        section_result
            .get("entries")
            .and_then(|entries| entries.items.first())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Missing entry in get_section response"))?
    };

    let entry_url = entry
        .children
        .get("link")
        .and_then(|link| link.values.first())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Missing link in load_home entry"))?;

    Ok(entry_url)
}
