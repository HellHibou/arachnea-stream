use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;

use arachnea_core::persistence::PersistenceStore;
use arachnea_proxy::core::{
    ArachneaProxyCore, InventoryConfig, IpCountryResolver, IpCountryResolverConfig,
    ParameterHandlerConfig, ParameterHandlerKind, ProbeConfig, ProxyAvailabilityHint, ProxyChain,
    ProxyConfig, ProxyDataProvider, ProxyInventory, ProxyLoadRequest, ProxyProbe, ProxyProfile,
    ProxyProtocol, ProxyRecord, ProxyRuntimeStatus, Result, RoutePolicy,
    PROXY_HEADER_PARAMETER_COUNTRY, PROXY_PARAMETER_COUNTRY,
};
use tracing::{info, trace};

use crate::scrapyfy::ip_country_provider::ScrapyfyIpCountryDataProvider;
use crate::scrapyfy::ScraperDataNode;
use crate::ScraperAgregator;
use crate::DEFAULT_SERVICES_DIRECTORY;

/// The group name used for the scrapyfy proxy query collection.
const PROXIES_GROUP_NAME: &str = "arachnea-proxies";

/// The group name used for the IP-country query collection.
const IP_COUNTRY_GROUP_NAME: &str = "arachnea-ip-countries";

/// `arachnea-scrapyfy` provider placeholder for dynamic proxy data.
///
/// Data loading is intentionally left unimplemented so source-specific loading
/// can be added separately without coupling this module to one public list
/// format.
///
/// # Safety
///
/// Stores a raw pointer to [`ScraperAgregator`]. The pointer must remain valid
/// for the lifetime of this provider. This is guaranteed because the provider
/// is always stored within the proxy core which is owned by the same
/// [`ScraperAgregator`] instance.
pub struct ScrapyfyProxyDataProvider {
    scraper_agregator: *const ScraperAgregator,
}

// SAFETY: the raw pointer points to the owning ScraperAgregator which lives
// longer than this provider. The provider is used only within the proxy core
// owned by the same aggregator, on the same executor as the aggregator.
unsafe impl Send for ScrapyfyProxyDataProvider {}
unsafe impl Sync for ScrapyfyProxyDataProvider {}

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
            scraper_agregator: scraper_agregator as *const ScraperAgregator,
        }
    }
}

#[async_trait]
impl ProxyDataProvider for ScrapyfyProxyDataProvider {
    async fn load_proxies(&self, request: ProxyLoadRequest) -> Result<Vec<ProxyRecord>> {
        let country = request
            .country
            .map(|country| normalize_proxy_country(&country));
        let Some(ref country) = country else {
            return Ok(Vec::new());
        };

        info!("Loading proxies for country: {}...", country);

        // SAFETY: the raw pointer is valid for the lifetime of the provider
        // because the ScraperAgregator owns the core which owns the inventory
        // which owns this provider.
        let agregator = unsafe { &*self.scraper_agregator };

        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("country".to_string(), country.clone());
        let results = agregator
            .execute_query_async(
                PROXIES_GROUP_NAME,
                "list_proxies_for_country",
                &params,
                None,
                None,
                None,
                None,
                None,
                "load_proxies",
            )
            .await
            .data;

        let mut records: Vec<ProxyRecord> = results
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
            .filter(|r| r.country.as_deref() == Some(country.as_str()))
            .collect();

        let mut seen = HashSet::new();
        let deduped: Vec<ProxyRecord> = filtered
            .into_iter()
            .filter(|r| seen.insert(r.authority()))
            .collect();

        info!("Loaded proxies for country {}: {}", country, deduped.len());
        Ok(deduped)
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
        .and_then(|s| match s.trim().to_ascii_lowercase().as_str() {
            "http" => Some(ProxyProtocol::Http),
            "https" => Some(ProxyProtocol::Https),
            "socks4" => Some(ProxyProtocol::Socks4),
            "socks4a" => Some(ProxyProtocol::Socks4a),
            "socks5" => Some(ProxyProtocol::Socks5),
            _ => None,
        });

    let supports_https = entry.get("supports_https").and_then(|n| n.value_as_bool());
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
        protocol,
        host: host.to_string(),
        port,
        country: entry
            .get("country")
            .and_then(|n| n.value_as_string())
            .map(normalize_proxy_country),
        supports_https,
        status: ProxyRuntimeStatus::Unknown,
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
        cooldown_until: None,
    };

    trace!("Loaded proxy record: {:?}", record);
    Some(record)
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
    persistence_store: Arc<dyn PersistenceStore>,
) -> ProxyInventory {
    let probe_config = ProbeConfig {
        https_probe_url: Some("https://example.com/".to_string()),
        ..ProbeConfig::default()
    };
    let resolver = default_scrapyfy_ip_country_resolver(scraper_agregator);
    ProxyInventory::new(
        InventoryConfig::default(),
        Some(Arc::new(ScrapyfyProxyDataProvider::new(scraper_agregator))),
        Some(Arc::new(ProxyProbe::new(probe_config))),
    )
    .with_ip_country_resolver(Arc::new(resolver))
    .with_persistence_store(persistence_store)
}

/// Builds a default [`ArachneaProxyCore`] with dynamic country routing backed
/// by scrapyfy proxy sources.
///
/// The returned core uses [`DynamicCountryRoutingProxyHandler`] with
/// `DynamicOnly` coexistence policy — any `country` request parameter triggers
/// a `ProxyInventory` lookup that lazily loads proxy data through
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
    persistence_store: Arc<dyn PersistenceStore>,
) -> Result<ArachneaProxyCore> {
    let inventory = default_scrapyfy_proxy_inventory(scraper_agregator, persistence_store);
    let proxy_config = ProxyConfig {
        profile: ProxyProfile::Advanced,
        chains: vec![ProxyChain::direct()],
        routing: RoutePolicy {
            default_chain: Some("direct".to_string()),
            ..RoutePolicy::default()
        },
        parameter_handlers: vec![ParameterHandlerConfig {
            kind: ParameterHandlerKind::DynamicCountryRouting,
            parameter_name: Some(PROXY_PARAMETER_COUNTRY.to_string()),
            http_header: Some(PROXY_HEADER_PARAMETER_COUNTRY.to_string()),
            forward_header: false,
            stop_on_match: true,
            routes: vec![],
        }],
        ..ProxyConfig::default()
    };
    ArachneaProxyCore::from_resolved_with_proxy_inventory(proxy_config.resolve()?, inventory)
}
