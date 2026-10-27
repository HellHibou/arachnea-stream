use super::StreamScraper;

use crate::stream_scraper::DEFAULT_SERVICES_CONFIG_PATH;
use crate::stream_scraper::STREAM_SERVICE_GROUP_NAME;
use anyhow::Result;
use arachnea_core::controler::RequestControlerContext;
use arachnea_scrapyfy::scrapyfy::scraper_agregator::resolve_manifest_sources;
use arachnea_scrapyfy::scrapyfy::scraper_data_node::ScraperDataNode;
use arachnea_scrapyfy::scrapyfy::scraper_manager::tests::assert_query_succeeds;
use arachnea_scrapyfy::scrapyfy::scraper_manager::tests::test_query;
use arachnea_scrapyfy::scrapyfy::scraper_manager::tests::TestParams;
use std::path::Path;

use crate::test_support::configure_test_application_data_dir;

static DEFAULT_SEARCH_TERM: &str = "inf";
static DEFAULT_QUERY_SOURCE: &str = "arachnea-stream/dark-stream/anime-sama.yaml";

pub(super) fn test_params() -> TestParams {
    TestParams {
        use_mock_file: false,
        ignore_entry_not_mapped: true,
        log_response: false,
    }
}

pub(super) fn query_source() -> String {
    std::env::var("ARACHNEA_TEST_QUERY_SOURCE").unwrap_or_else(|_| DEFAULT_QUERY_SOURCE.to_string())
}

pub(super) fn search_term() -> String {
    std::env::var("ARACHNEA_TEST_SEARCH_TERM").unwrap_or_else(|_| DEFAULT_SEARCH_TERM.to_string())
}

pub(super) fn get_entry_url() -> Option<String> {
    std::env::var("ARACHNEA_TEST_ENTRY_URL").ok()
}

pub(super) fn test_all(tests: Vec<&str>) {
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

pub(super) fn service_stream_metadata(yaml_file: &str) -> Result<()> {
    configure_test_application_data_dir();
    test_query(
        &mut StreamScraper::default(),
        STREAM_SERVICE_GROUP_NAME,
        "service_stream_metadata",
        test_params(),
        yaml_file,
        move |scraper| {
            Box::pin(async move {
                let (result, _) = scraper
                    .get_service(RequestControlerContext::default())
                    .await?;
                assert!(
                    result.is_ok(),
                    "service_stream_metadata should complete without errors, got {:?}",
                    result.errors,
                );
                let source = result.data[0]["id"].value_as_string().unwrap();
                let advertised = &result.data[0]["supported_queries"].values;
                assert_eq!(
                    advertised.contains(&"load_home".to_string()),
                    scraper.scraper_agregator.source_has_query(
                        STREAM_SERVICE_GROUP_NAME,
                        source,
                        "load_home"
                    ),
                );
                let (empty, etag) = scraper
                    .load_home(RequestControlerContext::default(), Some(Vec::new()))
                    .await?;
                assert!(empty.data.is_empty() && empty.errors.is_empty() && etag.is_none());
                let (unknown, etag) = scraper
                    .load_home(
                        RequestControlerContext::default(),
                        Some(vec!["unknown-source-for-filter-validation".to_string()]),
                    )
                    .await?;
                assert!(unknown.data.is_empty() && unknown.errors.len() == 1 && etag.is_none());
                assert_eq!(
                    unknown.errors[0].source.as_deref(),
                    Some("unknown-source-for-filter-validation")
                );
                for invalid in [
                    r#"{"sources":null}"#,
                    r#"{"sources":"arte-fr"}"#,
                    r#"{"sources":[""]}"#,
                    r#"{"sources":[" "]}"#,
                ] {
                    assert!(serde_json::from_str::<super::LoadHomeRequest>(invalid).is_err());
                }
                assert!(serde_json::from_str::<super::LoadHomeRequest>("{}")?
                    .sources
                    .is_none());
                assert_eq!(
                    serde_json::from_str::<super::LoadHomeRequest>(r#"{"sources":[]}"#)?.sources,
                    Some(Vec::new())
                );
                Ok(result.data)
            })
        },
    )
}

pub(super) fn load_home(yaml_file: &str) -> Result<()> {
    configure_test_application_data_dir();
    test_query(
        &mut StreamScraper::default(),
        STREAM_SERVICE_GROUP_NAME,
        "load_home",
        test_params(),
        yaml_file,
        move |scraper| {
            Box::pin(async move {
                let (result, _) = scraper
                    .load_home(RequestControlerContext::default(), None)
                    .await?;
                assert!(
                    result.is_ok(),
                    "load_home should complete without errors, got {:?}",
                    result.errors,
                );
                Ok(result.data)
            })
        },
    )
}

pub(super) fn search(search_term: String, yaml_file: &str) -> Result<()> {
    configure_test_application_data_dir();
    test_query(
        &mut StreamScraper::default(),
        STREAM_SERVICE_GROUP_NAME,
        "search",
        test_params(),
        yaml_file,
        move |scraper| {
            let search_term = search_term.clone();
            Box::pin(async move {
                let (result, _) = scraper
                    .search(
                        RequestControlerContext::default(),
                        search_term,
                        Vec::new(),
                        Vec::new(),
                        1,
                        Default::default(),
                    )
                    .await?;

                // Verify envelope contract
                assert!(
                    result.is_ok(),
                    "search should not fail at the envelope level",
                );

                Ok(result
                    .data
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

pub(super) fn get_entry(get_entry_url: Option<String>, yaml_file: &str) -> Result<()> {
    configure_test_application_data_dir();
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
                let yaml_path = Path::new(&yaml_file_inner);
                let source_name = yaml_path
                    .file_stem()
                    .unwrap_or_else(|| yaml_path.as_os_str())
                    .to_string_lossy();
                let entry_result = scraper
                    .get_entry(
                        RequestControlerContext::default(),
                        source_name.to_string(),
                        entry_url,
                    )
                    .await?;
                let (entry_result, _) = entry_result;
                assert!(
                    entry_result.is_ok(),
                    "get_entry should complete without errors, got {:?}",
                    entry_result.errors,
                );
                Ok(vec![entry_result.data])
            })
        },
    )
}

pub(super) fn load_enabled_services() -> Vec<String> {
    let config_file = format!(
        "{}/{}",
        arachnea_core::application::get_application_root(),
        DEFAULT_SERVICES_CONFIG_PATH
    );
    let services_json_path = Path::new(config_file.as_str());
    let services = resolve_manifest_sources(services_json_path)
        .expect("Failed to resolve services.json manifest");

    services
        .into_iter()
        .filter(|s| s.enabled)
        .map(|s| format!("{}/{}", STREAM_SERVICE_GROUP_NAME, s.path))
        .collect()
}

pub(super) async fn load_entry_url_from_home(
    scraper: &mut StreamScraper,
    query_source: &str,
) -> Result<String> {
    let (home_result, _) = scraper
        .load_home(RequestControlerContext::default(), None)
        .await?;
    let home_result = home_result.data;

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
            .get_section(
                RequestControlerContext::default(),
                query_source.to_string(),
                section_url,
                1,
                Default::default(),
            )
            .await?;
        let section_result = section_result.0.data;

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
