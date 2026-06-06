use super::StreamScraper;
use anyhow::Result;
use arachnea_core::persistence::resources;
use arachnea_scrapyfy::scrapyfy::scraper_data_node::ScraperDataNode;
use arachnea_scrapyfy::scrapyfy::scraper_manager::tests::assert_query_succeeds;
use arachnea_scrapyfy::scrapyfy::scraper_manager::tests::test_query;
use arachnea_scrapyfy::scrapyfy::scraper_manager::tests::TestParams;
use std::collections::HashMap;

const DEFAULT_FILE_CREDENTIALS_STORE_PATH: &str = "data/credentials.json";

static DEFAULT_SEARCH_TERM: &str = "inf";
static ALL_QUERY_SOURCES: [&str; 5] = [
    "m6play-fr",
    "tf1-fr",
    "rtlplay-be",
    "rtbf-auvio-be",
    "anime-sama",
];
static DEFAULT_QUERY_SOURCE: &str = "anime-sama";

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
    test_all(ALL_QUERY_SOURCES.to_vec());
}

#[test]
fn test_service() {
    test_all(Vec::from([query_source().as_str()]));
}
fn test_all(tests: Vec<&str>) {
    let mut failures = Vec::new();

    for query_source in tests {
        assert_query_succeeds(&mut failures, query_source, "load_home", |source| {
            load_home(source)
        });

        assert_query_succeeds(&mut failures, query_source, "search", |source| {
            search(source, search_term())
        });

        assert_query_succeeds(&mut failures, query_source, "get_entry", |source| {
            get_entry(source, get_entry_url())
        });
    }

    assert!(
        failures.is_empty(),
        "test_all_services found {} failure(s):\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

#[test]
pub fn test_query_load_home() -> Result<()> {
    load_home(query_source().as_str())
}
fn load_home(query_source: &str) -> Result<()> {
    test_query(
        &mut StreamScraper::new(
            resources::get_application_path(DEFAULT_FILE_CREDENTIALS_STORE_PATH).as_str(),
        ),
        query_source,
        "load_home",
        test_params(),
        move |scraper| Box::pin(scraper.load_home()),
    )
}

#[test]
pub fn test_query_search() -> Result<()> {
    search(query_source().as_str(), search_term())
}
fn search(query_source: &str, search_term: String) -> Result<()> {
    test_query(
        &mut StreamScraper::new(
            resources::get_application_path(DEFAULT_FILE_CREDENTIALS_STORE_PATH).as_str(),
        ),
        query_source,
        "search",
        test_params(),
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
    get_entry(query_source().as_str(), get_entry_url())
}
fn get_entry(query_source: &str, get_entry_url: Option<String>) -> Result<()> {
    let query_source_name = query_source.to_string();
    test_query(
        &mut StreamScraper::new(
            resources::get_application_path(DEFAULT_FILE_CREDENTIALS_STORE_PATH).as_str(),
        ),
        query_source,
        "get_entry",
        test_params(),
        move |scraper| {
            let get_entry_url = get_entry_url.clone();
            let query_source = query_source_name.clone();
            Box::pin(async move {
                let entry_url;
                if get_entry_url.is_none() {
                    entry_url = load_entry_url_from_home(scraper, &query_source).await?;
                } else {
                    entry_url = get_entry_url.clone().unwrap();
                }

                println!("Entry URL: {}", entry_url);
                scraper.get_entry(query_source.clone(), entry_url).await
            })
        },
    )
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
