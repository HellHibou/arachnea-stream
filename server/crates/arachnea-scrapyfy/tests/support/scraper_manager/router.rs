use super::*;
#[cfg(any(test, feature = "test-support"))]
use tracing::debug;

#[cfg(any(test, feature = "test-support"))]
pub(super) static MOCK_DATA_FOLDER: &str = "mock_data";

#[cfg(any(test, feature = "test-support"))]
pub(super) static TEST_DATA_FOLDER: &str = "data-test";

#[cfg(any(test, feature = "test-support"))]
/// Installs a file-backed mock router resolving `<source>-<query>.html` fixtures.
pub fn init_mock(query_source: &str, query: &str) {
    let query_str = query.to_string();
    let query_source_str = query_source.to_string();

    http_client::set_router(move |_client, _route| {
        let file = format!(
            "{}/{}/{}-{}.html",
            arachnea_core::application::get_application_root(),
            MOCK_DATA_FOLDER,
            query_source_str,
            query_str
        );

        debug!(target: "arachnea_scrapyfy::scrapyfy::scraper_manager", "{}", file);
        Ok(std::fs::read_to_string(file)?)
    });
}
