use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use arachnea_core::controler::RequestControlerContext;
use arachnea_core::persistence::TypedEntityStore;
use arachnea_proxy::core::{
    ArachneaProxyCore, InventoryConfig, IpCountryResolver, IpCountryResolverConfig,
    ParameterHandlerConfig, ParameterHandlerKind, ProbeConfig, ProxyAvailabilityHint,
    ProxyCapabilityStatus, ProxyChain, ProxyConfig, ProxyDataProvider, ProxyDeclaration,
    ProxyInventory, ProxyLoadRequest, ProxyProbe, ProxyProfile, ProxyProtocol, ProxyRecord,
    ProxyRuntimeStatus, Result, RoutePolicy, PROXY_HEADER_PARAMETER_COUNTRIES,
    PROXY_PARAMETER_COUNTRIES, PROXY_PROTOCOL_PRIORITY,
};
use tracing::{info, trace};

use crate::scrapyfy::ip_country_provider::ScrapyfyIpCountryDataProvider;
use crate::scrapyfy::{CacheType, QueryParameters, ScraperDataNode};
use crate::ScraperAgregator;
use crate::DEFAULT_SERVICES_DIRECTORY;

/// The group name used for the scrapyfy proxy query collection.
const PROXIES_GROUP_NAME: &str = "arachnea-proxies";

/// The group name used for the IP-country query collection.
const IP_COUNTRY_GROUP_NAME: &str = "arachnea-ip-countries";

/// Dynamic proxy provider with an owned snapshot of the configured sources.
///
/// The snapshot remains valid even when the application replaces its aggregator.
pub struct ScrapyfyProxyDataProvider {
    scraper_agregator: Arc<ScraperAgregator>,
}

impl ScrapyfyProxyDataProvider {
    /// Creates a proxy data provider backed by the given aggregator.
    pub fn new(scraper_agregator: &mut ScraperAgregator) -> Self {
        let preferred_config_path = format!(
            "{}/{}/{}",
            DEFAULT_SERVICES_DIRECTORY, PROXIES_GROUP_NAME, "services.json"
        );
        if let Err(preferred_error) = scraper_agregator
            .add_query_collection_from_config_json(PROXIES_GROUP_NAME, &preferred_config_path)
        {
            let legacy_config_path = format!(
                "{}/{}/{}",
                DEFAULT_SERVICES_DIRECTORY, PROXIES_GROUP_NAME, "services.json"
            );
            if let Err(legacy_error) = scraper_agregator
                .add_query_collection_from_config_json(PROXIES_GROUP_NAME, &legacy_config_path)
            {
                tracing::warn!(
                    preferred_error = %preferred_error,
                    legacy_error = %legacy_error,
                    "failed to load scrapyfy proxy source collection"
                );
            }
        }

        let ip_country_config_path = format!(
            "{}/{}/{}",
            DEFAULT_SERVICES_DIRECTORY, IP_COUNTRY_GROUP_NAME, "services.json"
        );
        if let Err(error) = scraper_agregator
            .add_query_collection_from_config_json(IP_COUNTRY_GROUP_NAME, &ip_country_config_path)
        {
            tracing::warn!(
                error = %error,
                "failed to load IP-country query collection; country resolution will be unavailable"
            );
        }

        ScrapyfyProxyDataProvider {
            scraper_agregator: scraper_agregator
                .provider_snapshot(&[PROXIES_GROUP_NAME, IP_COUNTRY_GROUP_NAME]),
        }
    }
}

#[async_trait]
impl ProxyDataProvider for ScrapyfyProxyDataProvider {
    async fn load_proxies(&self, request: ProxyLoadRequest) -> Result<Vec<ProxyRecord>> {
        let countries = request
            .countries
            .iter()
            .map(|country| normalize_proxy_country(country))
            .filter(|country| !country.is_empty())
            .collect::<Vec<_>>();
        if countries.is_empty() {
            return Ok(Vec::new());
        }

        let mut records = Vec::new();
        for country in countries {
            records.extend(self.load_proxies_for_country(&country).await?);
        }
        Ok(records)
    }
}

impl ScrapyfyProxyDataProvider {
    async fn load_proxies_for_country(&self, country: &str) -> Result<Vec<ProxyRecord>> {
        info!("Loading proxies for country: {}...", country);

        let agregator = &self.scraper_agregator;

        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("country".to_string(), country.to_string());
        let result = agregator
            .execute_query_async(
                &RequestControlerContext::default(),
                QueryParameters {
                    cache_type: CacheType::NoCache,
                },
                PROXIES_GROUP_NAME,
                "list_proxies_for_country",
                &params,
                None,
                None,
                None,
                None,
                Some("proxy_source"),
                "load_proxies",
            )
            .await;

        if !result.errors.is_empty() {
            tracing::warn!(
                country,
                error_count = result.errors.len(),
                errors = ?result.errors,
                "dynamic proxy sources reported errors"
            );
        }
        if result.data.is_empty() {
            tracing::warn!(
                country,
                "dynamic proxy sources returned no rows; verify that at least one source is enabled"
            );
        }

        let mut records: Vec<ProxyRecord> = result
            .data
            .into_iter()
            .filter_map(|entry| entry_to_proxy_record(&entry))
            .collect();

        // Resolve country for records whose source did not provide one
        let mut unresolved: Vec<(usize, String)> = Vec::new();
        for (i, r) in records.iter().enumerate() {
            if r.country.is_none() {
                if let Some(ip) = r.host.parse::<std::net::IpAddr>().ok() {
                    unresolved.push((i, ip.to_string()));
                    if unresolved.len() >= 20 {
                        break;
                    }
                }
            }
        }

        if !unresolved.is_empty() {
            let resolved_count = unresolved.len();
            for (idx, ip_str) in &unresolved {
                let mut ip_params = HashMap::new();
                ip_params.insert("ip".to_string(), ip_str.clone());
                let rows = agregator
                    .execute_query_async(
                        &RequestControlerContext::default(),
                        QueryParameters {
                            cache_type: CacheType::NoCache,
                        },
                        IP_COUNTRY_GROUP_NAME,
                        "resolve_ip_country",
                        &ip_params,
                        None,
                        None,
                        None,
                        None,
                        None,
                        "resolve_ip_country",
                    )
                    .await
                    .data;
                if !rows.is_empty() {
                    for row in &rows {
                        if let Some(country) =
                            row.get("country_code").and_then(|n| n.value_as_string())
                        {
                            if !country.is_empty() {
                                records[*idx].country = Some(country.trim().to_ascii_uppercase());
                            }
                            break;
                        }
                    }
                }
            }
            info!(
                resolved_count,
                "resolved missing country codes via ip-api.com YAML query"
            );
        }

        let filtered: Vec<ProxyRecord> = records
            .into_iter()
            .filter(|r| r.country.as_deref() == Some(country))
            .collect();

        info!("Loaded proxies for country {}: {}", country, filtered.len());
        Ok(filtered)
    }
}

fn entry_to_proxy_record(entry: &HashMap<String, ScraperDataNode>) -> Option<ProxyRecord> {
    let host = entry.get("host")?.value_as_string()?.trim();
    if host.is_empty() {
        return None;
    }

    let port = entry.get("port")?.value_as_u16()?;
    let protocol = entry
        .get("protocol")
        .and_then(|node| node.value_as_string())
        .and_then(parse_proxy_protocol)
        .or_else(|| {
            entry
                .get("protocols")
                .and_then(|node| preferred_proxy_protocol(&node.values))
        });

    let supports_https = entry.get("supports_https").and_then(|n| n.value_as_bool());
    let source = entry
        .get("proxy_source")
        .and_then(|node| node.value_as_string())
        .unwrap_or("unknown")
        .trim()
        .to_string();
    let availability = entry
        .get("availability")
        .and_then(|node| node.value_as_string())
        .map(|s| match s.to_lowercase().as_str() {
            "low" => ProxyAvailabilityHint::Low,
            "medium" => ProxyAvailabilityHint::Medium,
            "high" => ProxyAvailabilityHint::High,
            _ => ProxyAvailabilityHint::Unknown,
        })
        .unwrap_or(ProxyAvailabilityHint::Unknown);

    let record = ProxyRecord {
        protocol: protocol.clone(),
        host: host.to_string(),
        port,
        country: entry
            .get("country")
            .and_then(|n| n.value_as_string())
            .map(normalize_proxy_country),
        supports_https,
        declarations: vec![ProxyDeclaration {
            source,
            protocol: protocol.clone(),
            supports_https,
        }],
        status: ProxyRuntimeStatus::Unknown,
        http_forwarding: ProxyCapabilityStatus::Unknown,
        https_tunnel: ProxyCapabilityStatus::Unknown,
        destination_tls: ProxyCapabilityStatus::Unknown,
        proxy_tls_certificate: ProxyCapabilityStatus::Unknown,
        latency_ms: entry.get("latency_ms").and_then(|n| n.value_as_u64()),
        failure_count: entry
            .get("failure_count")
            .and_then(|n| n.value_as_u32())
            .unwrap_or(0),
        authentication_required: entry
            .get("authentication_required")
            .and_then(|n| n.value_as_bool()),
        availability,
        destination_failures: Vec::new(),
        last_checked: None,
        last_validated_at: None,
        cooldown_until: None,
    };

    trace!("Loaded proxy record: {:?}", record);
    Some(record)
}

fn parse_proxy_protocol(value: &str) -> Option<ProxyProtocol> {
    match value.trim().to_ascii_lowercase().as_str() {
        "http" => Some(ProxyProtocol::Http),
        "https" => Some(ProxyProtocol::Https),
        "socks4" => Some(ProxyProtocol::Socks4),
        "socks4a" => Some(ProxyProtocol::Socks4a),
        "socks5" => Some(ProxyProtocol::Socks5),
        _ => None,
    }
}

fn preferred_proxy_protocol(values: &[String]) -> Option<ProxyProtocol> {
    PROXY_PROTOCOL_PRIORITY
        .iter()
        .find(|preferred| {
            values
                .iter()
                .filter_map(|value| parse_proxy_protocol(value))
                .any(|protocol| &protocol == *preferred)
        })
        .cloned()
}

fn normalize_proxy_country(country: &str) -> String {
    country.trim().to_ascii_uppercase()
}

/// Builds a default [`IpCountryResolver`] backed by the scrapyfy IP-country
/// query infrastructure.
///
/// The resolver loads persisted data from `data/ip-countries.json` by default
/// and resolves unknown IPs through the YAML-defined geolocation query on
/// demand, with a 5-second timeout and automatic persistence of newly resolved
/// mappings.
///
/// # Arguments
///
/// * `scraper_agregator` - The aggregator instance with the IP-country query
///   collection already loaded.
///
/// # Returns
///
/// A configured `IpCountryResolver` ready to be attached to a `ProxyInventory`.
pub fn default_scrapyfy_ip_country_resolver(
    scraper_agregator: &mut ScraperAgregator,
) -> IpCountryResolver {
    let provider = ScrapyfyIpCountryDataProvider::new(scraper_agregator);
    let store = Arc::new(arachnea_proxy::core::IpCountrySerdeStore::new(
        "data/ip-countries.json",
        Arc::new(arachnea_proxy::core::JsonIpCountryCodec::new()),
    ));
    IpCountryResolver::new(
        IpCountryResolverConfig::default(),
        Some(Arc::new(provider)),
        Some(store),
    )
}

/// Builds a default dynamic proxy inventory backed by scrapyfy proxy sources.
///
/// The returned inventory uses [`ScrapyfyProxyDataProvider`] and the default
/// [`ProxyProbe`] configuration, with an IP-country resolver pre-configured
/// for strict country selection. `persistence_store` is attached as the
/// persistent cache of dynamic proxies (shared with other domains such as
/// cookies; namespaces keep data families isolated).
///
/// # Arguments
///
/// * `scraper_agregator` - The aggregator instance that owns or will own this
///   inventory.
/// * `persistence_store` - Shared persistence store used as the proxy cache.
pub fn default_scrapyfy_proxy_inventory(
    scraper_agregator: &mut ScraperAgregator,
    proxy_store: Arc<dyn TypedEntityStore<ProxyRecord>>,
) -> ProxyInventory {
    let probe_config = ProbeConfig {
        https_probe_url: Some("https://example.com/".to_string()),
        ..ProbeConfig::default()
    };
    let resolver = default_scrapyfy_ip_country_resolver(scraper_agregator);
    ProxyInventory::new(
        InventoryConfig {
            probe_batch_size: 32,
            ..InventoryConfig::default()
        },
        Some(Arc::new(ScrapyfyProxyDataProvider::new(scraper_agregator))),
        Some(Arc::new(ProxyProbe::new(probe_config))),
    )
    .with_ip_country_resolver(Arc::new(resolver))
    .with_proxy_repository(Arc::new(arachnea_proxy::TypedProxyRepository::new(
        proxy_store,
    )))
}

/// Builds a default [`ArachneaProxyCore`] with dynamic country routing backed
/// by scrapyfy proxy sources.
///
/// The returned core uses [`DynamicCountryRoutingProxyHandler`] with
/// `DynamicOnly` coexistence policy — an ordered JSON `countries` request
/// parameter triggers a `ProxyInventory` lookup that lazily loads proxy data through
/// [`ScrapyfyProxyDataProvider`].
///
/// # Arguments
///
/// * `scraper_agregator` - The aggregator instance that owns or will own this core.
/// * `persistence_store` - Shared persistence store used as the proxy cache.
///
/// # Returns
///
/// A configured `ArachneaProxyCore` ready to route proxy requests.
///
/// # Errors
///
/// Returns an error when proxy configuration validation fails.
pub fn default_scrapyfy_proxy_core(
    scraper_agregator: &mut ScraperAgregator,
    proxy_store: Arc<dyn TypedEntityStore<ProxyRecord>>,
) -> Result<ArachneaProxyCore> {
    let inventory = default_scrapyfy_proxy_inventory(scraper_agregator, proxy_store);
    let proxy_config = ProxyConfig {
        profile: ProxyProfile::Advanced,
        chains: vec![ProxyChain::direct()],
        routing: RoutePolicy {
            default_chain: Some("direct".to_string()),
            ..RoutePolicy::default()
        },
        parameter_handlers: vec![ParameterHandlerConfig {
            kind: ParameterHandlerKind::DynamicCountryRouting,
            parameter_name: Some(PROXY_PARAMETER_COUNTRIES.to_string()),
            http_header: Some(PROXY_HEADER_PARAMETER_COUNTRIES.to_string()),
            forward_header: false,
            stop_on_match: true,
            routes: vec![],
        }],
        ..ProxyConfig::default()
    };
    ArachneaProxyCore::from_resolved_with_proxy_inventory(proxy_config.resolve()?, inventory)
}

#[cfg(test)]
mod tests {
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
}
