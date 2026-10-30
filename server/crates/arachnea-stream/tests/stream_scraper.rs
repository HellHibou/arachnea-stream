use super::test_support::*;
use anyhow::Result;

#[test]
#[ignore = "requires live third-party providers; run manually with --ignored"]
fn test_all_services() {
    super::test_support::test_all_services().expect("Discovered service checks failed");
}

#[test]
#[ignore = "requires live third-party providers; select sources with ARACHNEA_TEST_QUERY_SOURCE and run with --ignored"]
fn test_service() {
    super::test_support::test_service().expect("Selected source checks failed");
}

#[test]
pub fn test_query_service_stream_metadata() -> Result<()> {
    service_stream_metadata()
}

#[test]
#[ignore = "requires live third-party providers; select sources with ARACHNEA_TEST_QUERY_SOURCE and run with --ignored"]
pub fn test_query_load_home() -> Result<()> {
    load_home()
}

#[test]
#[ignore = "requires live third-party providers; select sources with ARACHNEA_TEST_QUERY_SOURCE and run with --ignored"]
pub fn test_query_search() -> Result<()> {
    search()
}

#[test]
#[ignore = "requires live third-party providers; select sources with ARACHNEA_TEST_QUERY_SOURCE and run with --ignored"]
pub fn test_query_get_entry() -> Result<()> {
    get_entry()
}
