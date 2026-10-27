use super::test_support::*;
use anyhow::Result;

#[test]
#[ignore = "requires live third-party providers; run manually with --ignored"]
fn test_all_services() {
    let enabled_services = load_enabled_services();
    let enabled_services_refs: Vec<&str> = enabled_services.iter().map(|s| s.as_str()).collect();
    test_all(enabled_services_refs);
}

#[test]
fn test_service() {
    test_all(Vec::from([query_source().as_str()]));
}

#[test]
pub fn test_query_service_stream_metadata() -> Result<()> {
    service_stream_metadata(query_source().as_str())
}

#[test]
pub fn test_query_load_home() -> Result<()> {
    load_home(query_source().as_str())
}

#[test]
pub fn test_query_search() -> Result<()> {
    search(search_term(), query_source().as_str())
}

#[test]
pub fn test_query_get_entry() -> Result<()> {
    get_entry(get_entry_url(), query_source().as_str())
}
