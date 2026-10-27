use super::*;
use serde_yaml::Value;

const PROXYCOMPASS_YAML: &str =
    include_str!("../../../../services/arachnea-proxies/proxycompass.yaml");

fn proxy_entry(protocol: Option<&str>, protocols: &[&str]) -> HashMap<String, ScraperDataNode> {
    let mut entry = HashMap::from([
        (
            "host".to_string(),
            ScraperDataNode::from_values(vec!["192.0.2.10".to_string()]),
        ),
        (
            "port".to_string(),
            ScraperDataNode::from_values(vec!["8080".to_string()]),
        ),
    ]);

    if let Some(protocol) = protocol {
        entry.insert(
            "protocol".to_string(),
            ScraperDataNode::from_values(vec![protocol.to_string()]),
        );
    }
    if !protocols.is_empty() {
        entry.insert(
            "protocols".to_string(),
            ScraperDataNode::from_values(
                protocols.iter().map(|value| (*value).to_string()).collect(),
            ),
        );
    }

    entry
}

#[test]
fn entry_to_proxy_record_keeps_valid_singular_protocol() {
    let entry = proxy_entry(Some("SOCKS5"), &["HTTP", "SOCKS4"]);

    let record = entry_to_proxy_record(&entry).expect("proxy record");

    assert_eq!(record.protocol, Some(ProxyProtocol::Socks5));
}

#[test]
fn entry_to_proxy_record_selects_plural_protocol_by_priority() {
    let entry = proxy_entry(None, &["SOCKS4", "SOCKS5", "HTTP"]);

    let record = entry_to_proxy_record(&entry).expect("proxy record");

    assert_eq!(record.protocol, Some(ProxyProtocol::Socks5));
}

#[test]
fn entry_to_proxy_record_falls_back_to_plural_protocol() {
    let entry = proxy_entry(Some("UNKNOWN"), &["SOCKS4", "SOCKS5"]);

    let record = entry_to_proxy_record(&entry).expect("proxy record");

    assert_eq!(record.protocol, Some(ProxyProtocol::Socks5));
}

#[test]
fn proxycompass_maps_countries_and_limits_the_first_page() {
    let _: crate::scrapyfy::ScraperQueryCollection =
        serde_yaml::from_str(PROXYCOMPASS_YAML).expect("ProxyCompass runtime collection");
    let config: Value = serde_yaml::from_str(PROXYCOMPASS_YAML).expect("ProxyCompass YAML");
    let query = &config["queries"][0];
    let values = &query["query_param_mappings"][0]["values"];

    assert_eq!(values["FR"].as_str(), Some("France"));
    assert_eq!(values["US"].as_str(), Some("United%20States"));
    assert_eq!(values["TR"].as_str(), Some("T%C3%BCrkiye"));
    assert_eq!(
        query["query_url"].as_str(),
        Some("{base_url}/live?country={country_name}&page=1&page_size=1000")
    );
    assert!(query.get("pagination").is_none());
}
