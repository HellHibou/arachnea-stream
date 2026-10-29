use super::*;

impl ScraperAgregator {
    /// Returns the last loaded query collection within a group.
    pub fn get_last_query_collection(&self, group_name: &str) -> Option<&ScraperQueryCollection> {
        self.queries_collection.get(group_name)?.services.last()
    }

    /// Returns one loaded query collection by source name within a group.
    ///
    /// # Arguments
    ///
    /// * `group_name` - Group name to look up.
    /// * `name` - Source name to look up within the group.
    pub fn get_query_collection(
        &self,
        group_name: &str,
        name: &str,
    ) -> Option<&ScraperQueryCollection> {
        self.queries_collection
            .get(group_name)?
            .services
            .iter()
            .find(|entry| entry.name() == name)
    }
}

#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
pub(crate) fn configure_test_application_data_dir() {
    TEST_APPLICATION_DATA_DIR.call_once(|| {
        arachnea_core::application::configure_application_data_dir_name("arachnea-scrapyfy-tests")
            .expect("test application data directory should be configured");
    });
}

#[cfg(test)]
pub(super) fn unique_temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after UNIX_EPOCH")
        .as_nanos();
    let sequence = TEMP_DIR_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "arachnea-scrapyfy-aggregator-test-{}-{nanos}-{sequence}",
        std::process::id()
    ))
}

#[cfg(test)]
pub(super) fn setup_aggregator_sources(sources: &[(&str, &str)]) -> (ScraperAgregator, PathBuf) {
    configure_test_application_data_dir();
    let dir = unique_temp_dir();
    std::fs::create_dir_all(&dir).expect("test service directory should be created");

    let mut service_entries = Vec::new();
    for (name, yaml) in sources {
        let file_name = format!("{name}.yaml");
        std::fs::write(dir.join(&file_name), yaml).expect("test YAML should be written");
        service_entries.push(serde_json::json!({
            "path": file_name,
            "enabled": true,
        }));
    }

    let config_path = dir.join("services.json");
    std::fs::write(
        &config_path,
        serde_json::to_string(&service_entries).expect("test services config should serialize"),
    )
    .expect("test services config should be written");

    let mut aggregator = ScraperAgregator::new_with_proxy_handle(SharedProxyConfigHandle::new());
    aggregator
        .add_query_collection_from_config_json("test", &config_path)
        .expect("test sources should load");

    (aggregator, dir)
}

#[cfg(test)]
pub(super) fn static_source(id: &str, value: &str) -> String {
    format!(
        r#"
id: {id}
queries:
  - name: probe
    scraper_type: static
    entries:
      - name: value
        type: string
        value: {value}
"#
    )
}

#[cfg(test)]
pub(super) fn failing_html_source(id: &str, url: &str) -> String {
    format!(
        r#"
id: {id}
queries:
  - name: probe
    scraper_type: html
    base_url: {url}
    query_url: {url}
    row_selector: html
    entries:
      - name: value
        type: string
        selector: title
        actions:
          - type: get_text
"#
    )
}

#[cfg(test)]
pub(super) fn value_from_row(row: &HashMap<String, ScraperDataNode>) -> Option<&str> {
    row.get("value")
        .and_then(|node| node.values.first())
        .map(String::as_str)
}

#[cfg(test)]
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Once,
};
#[cfg(test)]
static TEST_APPLICATION_DATA_DIR: Once = Once::new();
#[cfg(test)]
static TEMP_DIR_SEQUENCE: AtomicU64 = AtomicU64::new(0);
