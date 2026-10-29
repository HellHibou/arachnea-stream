use futures::future::{BoxFuture, WeakShared};
use futures::{FutureExt, StreamExt};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

use async_trait::async_trait;

use arachnea_core::persistence::TypedEntityStore;
use arachnea_proxy::core::{
    ArachneaProxyCore, InventoryConfig, IpCountryResolver, IpCountryResolverConfig,
    ParameterHandlerConfig, ParameterHandlerKind, ProbeConfig, ProxyAvailabilityHint,
    ProxyCapabilityStatus, ProxyChain, ProxyConfig, ProxyDataProvider, ProxyDeclaration,
    ProxyInventory, ProxyLoadRequest, ProxyProbe, ProxyProfile, ProxyProtocol, ProxyRecord,
    ProxyRuntimeStatus, Result, RoutePolicy, PROXY_HEADER_PARAMETER_COUNTRIES,
    PROXY_PARAMETER_COUNTRIES, PROXY_PROTOCOL_PRIORITY,
};
use tracing::trace;

use crate::scrapyfy::ip_country_provider::ScrapyfyIpCountryDataProvider;
use crate::scrapyfy::ScraperDataNode;
use crate::ScraperAgregator;
use crate::DEFAULT_SERVICES_DIRECTORY;

use crate::SCRAPER_PROXIES_GROUP_NAME as PROXIES_GROUP_NAME;

use crate::SCRAPER_IP_COUNTRY_GROUP_NAME as IP_COUNTRY_GROUP_NAME;

/// Weak shared lookup jobs indexed by IP address, without extending job lifetimes.
type GeoLookupJobs = HashMap<IpAddr, WeakShared<BoxFuture<'static, Option<String>>>>;

/// Cached optional country results and their observation times, indexed by IP address.
type GeoLookupCache = HashMap<IpAddr, (tokio::time::Instant, Option<String>)>;

/// Dynamic proxy provider with an owned snapshot of the configured sources.
///
/// The snapshot remains valid even when the application replaces its aggregator.
pub struct ScrapyfyProxyDataProvider {
    scraper_agregator: Arc<ScraperAgregator>,
    geo_jobs: Arc<Mutex<GeoLookupJobs>>,
    geo_slots: Arc<Semaphore>,
    geo_cache: Arc<Mutex<GeoLookupCache>>,
}

#[derive(Default)]
/// In-memory sink collecting progressively published proxy records for compatibility callers.
struct CollectedProxyBatches(Mutex<Vec<ProxyRecord>>);

#[async_trait]
impl arachnea_proxy::core::ProxyBatchSink for CollectedProxyBatches {
    async fn publish(&self, records: Vec<ProxyRecord>) -> Result<()> {
        self.0.lock().await.extend(records);
        Ok(())
    }
}

impl ScrapyfyProxyDataProvider {
    /// Creates a proxy data provider backed by the given aggregator.
    ///
    /// # Arguments
    ///
    /// * `scraper_agregator` - Aggregator whose configured sources and runtime handles are captured for provider queries.
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
            geo_jobs: Arc::new(Mutex::new(HashMap::new())),
            geo_slots: Arc::new(Semaphore::new(crate::PROXY_GEO_QUERY_CONCURRENCY)),
            geo_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ProxyDataProvider for ScrapyfyProxyDataProvider {
    async fn load_proxies(&self, request: ProxyLoadRequest) -> Result<Vec<ProxyRecord>> {
        let sink = Arc::new(CollectedProxyBatches::default());
        self.load_proxies_progressively(request, sink.clone())
            .await?;
        let records = std::mem::take(&mut *sink.0.lock().await);
        Ok(records)
    }

    async fn load_proxies_progressively(
        &self,
        request: ProxyLoadRequest,
        sink: Arc<dyn arachnea_proxy::core::ProxyBatchSink>,
    ) -> Result<()> {
        let countries = arachnea_proxy::core::canonical_proxy_countries(&request.countries);
        let loads = futures::stream::iter(countries.into_iter().map(|country| {
            let sink = sink.clone();
            async move { self.publish_country(country, sink).await }
        }))
        .buffer_unordered(crate::PROXY_COUNTRY_LOAD_CONCURRENCY);
        futures::pin_mut!(loads);
        while let Some(result) = loads.next().await {
            result?;
        }
        Ok(())
    }
}

impl ScrapyfyProxyDataProvider {
    /// Publishes provider batches as they complete, resolving unknown IP countries within shared limits.
    ///
    /// # Arguments
    ///
    /// * `country` - Country-code text, trimmed and converted to uppercase ASCII.
    /// * `sink` - Subscriber receiving progressively available proxy-record batches.
    async fn publish_country(
        &self,
        country: String,
        sink: Arc<dyn arachnea_proxy::core::ProxyBatchSink>,
    ) -> Result<()> {
        let params = HashMap::from([("country".to_string(), country.clone())]);
        let mut sources = self.scraper_agregator.proxy_source_batches(
            PROXIES_GROUP_NAME,
            "list_proxies_for_country",
            params,
        );
        let mut source_done = false;
        let mut pending = HashMap::<IpAddr, Vec<ProxyRecord>>::new();
        let mut resolved = HashMap::<IpAddr, Option<String>>::new();
        let mut requested = HashSet::new();
        let mut seen = HashSet::new();
        let mut geos =
            futures::stream::FuturesUnordered::<BoxFuture<'_, (IpAddr, Option<String>)>>::new();
        loop {
            tokio::select! {
                batch = sources.next(), if !source_done => {
                    let rows = match batch {
                        None => { source_done = true; continue; }
                        Some(Err(error)) => { tracing::warn!(%error, country, "proxy source failed; retaining other sources"); continue; }
                        Some(Ok(rows)) => rows,
                    };
                    let mut ready = Vec::new();
                    for row in rows {
                        let Some(mut record) = entry_to_proxy_record(&row) else { continue; };
                        if !seen.insert((record.authority(), format!("{:?}", record.declarations))) { continue; }
                        if record.country.is_some() {
                            if record.country.as_deref() == Some(country.as_str()) { ready.push(record); }
                            continue;
                        }
                        let Ok(ip) = record.host.parse::<IpAddr>() else { continue; };
                        if let Some(known) = resolved.get(&ip) {
                            record.country = known.clone();
                            if record.country.as_deref() == Some(country.as_str()) { ready.push(record); }
                        } else if let Some(records) = pending.get_mut(&ip) {
                            records.push(record);
                        } else if requested.len() < 20 {
                            requested.insert(ip);
                            pending.insert(ip, vec![record]);
                            geos.push(async move { (ip, self.resolve_shared_ip(ip).await) }.boxed());
                        }
                    }
                    if !ready.is_empty() { sink.publish(ready).await?; }
                }
                result = geos.next(), if !geos.is_empty() => {
                    if let Some((ip, known)) = result {
                        resolved.insert(ip, known.clone());
                        let records = pending.remove(&ip).unwrap_or_default().into_iter().filter_map(|mut record| {
                            record.country = known.clone();
                            (record.country.as_deref() == Some(country.as_str())).then_some(record)
                        }).collect::<Vec<_>>();
                        if !records.is_empty() { sink.publish(records).await?; }
                    }
                }
                else => break,
            }
        }
        Ok(())
    }

    /// Resolves one IP through shared subscriber-owned work, caching positive and negative outcomes.
    ///
    /// # Arguments
    ///
    /// * `ip` - IP address to resolve without guessing a country from the requested partition.
    async fn resolve_shared_ip(&self, ip: IpAddr) -> Option<String> {
        {
            let mut cache = self.geo_cache.lock().await;
            cache.retain(|_, (at, country)| {
                at.elapsed()
                    < if country.is_some() {
                        crate::PROXY_GEO_SUCCESS_TTL
                    } else {
                        crate::PROXY_GEO_FAILURE_TTL
                    }
            });
            if let Some((_, country)) = cache.get(&ip) {
                return country.clone();
            }
        }
        let future = {
            let mut jobs = self.geo_jobs.lock().await;
            jobs.retain(|_, job| job.upgrade().is_some());
            if let Some(job) = jobs.get(&ip).and_then(WeakShared::upgrade) {
                job
            } else {
                let aggregator = self.scraper_agregator.clone();
                let slots = self.geo_slots.clone();
                let cache = self.geo_cache.clone();
                let job = async move {
                    let _slot = slots.acquire().await.ok()?;
                    let query = async {
                        let params = HashMap::from([("ip".to_string(), ip.to_string())]);
                        let mut batches = aggregator.proxy_source_batches(
                            IP_COUNTRY_GROUP_NAME,
                            "resolve_ip_country",
                            params,
                        );
                        while let Some(batch) = batches.next().await {
                            if let Ok(rows) = batch {
                                if let Some(country) = rows
                                    .iter()
                                    .filter_map(|row| {
                                        row.get("country_code")
                                            .and_then(|node| node.value_as_string())
                                    })
                                    .map(normalize_proxy_country)
                                    .find(|country| {
                                        country.len() == 2
                                            && country
                                                .bytes()
                                                .all(|byte| byte.is_ascii_alphabetic())
                                    })
                                {
                                    return Some(country);
                                }
                            }
                        }
                        None
                    };
                    let country =
                        tokio::time::timeout(crate::PROXY_GEO_QUERY_TIMEOUT, Box::pin(query))
                            .await
                            .ok()
                            .flatten();
                    let mut cache = cache.lock().await;
                    if cache.len() >= 1024 {
                        if let Some(oldest) = cache
                            .iter()
                            .min_by_key(|(_, (at, _))| *at)
                            .map(|(ip, _)| *ip)
                        {
                            cache.remove(&oldest);
                        }
                    }
                    cache.insert(ip, (tokio::time::Instant::now(), country.clone()));
                    country
                }
                .boxed()
                .shared();
                jobs.insert(ip, job.downgrade()?);
                job
            }
        };
        future.await
    }
}

/// Builds a proxy record from a row with a non-empty host and a valid numeric port.
///
/// # Arguments
///
/// * `entry` - Source result row converted into a proxy record.
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
        health_evidence: Default::default(),
        observations: Vec::new(),
        probe_attempts: Default::default(),
        tcp_reachable: None,
        protocol: protocol.clone(),
        host: host.to_string(),
        port,
        country: entry
            .get("country")
            .and_then(|n| n.value_as_string())
            .map(normalize_proxy_country)
            .filter(|country| {
                country.len() == 2 && country.bytes().all(|byte| byte.is_ascii_alphabetic())
            }),
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

/// Parses a supported proxy protocol name case-insensitively, returning `None` for unknown names.
///
/// # Arguments
///
/// * `value` - Input value interpreted by this operation.
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

/// Selects the highest-priority supported protocol present in the provider list.
///
/// # Arguments
///
/// * `values` - Input values converted or inspected in their existing order.
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

/// Trims and uppercases a provider country hint without verifying its location.
///
/// # Arguments
///
/// * `country` - Country-code text, trimmed and converted to uppercase ASCII.
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
/// Uses [`ScrapyfyProxyDataProvider`], the default HTTP probe target and an HTTPS
/// probe at `https://example.com/`, with an IP-country resolver for country-aware
/// selection. Matching the aggregator's proxy store reuses its coordinated
/// repository; another store gets a repository with in-memory failure retention.
/// Accepted-history persistence is shared with the aggregator in either case.
/// Other data families retain their own typed entity schemas.
///
/// # Arguments
///
/// * `scraper_agregator` - The aggregator instance that owns or will own this
///   inventory.
/// * `proxy_store` - Typed proxy-record cache; matching the aggregator store reuses its repository.
pub fn default_scrapyfy_proxy_inventory(
    scraper_agregator: &mut ScraperAgregator,
    proxy_store: Arc<dyn TypedEntityStore<ProxyRecord>>,
) -> ProxyInventory {
    let probe_config = ProbeConfig {
        https_probe_url: Some("https://example.com/".to_string()),
        ..ProbeConfig::default()
    };
    let resolver = default_scrapyfy_ip_country_resolver(scraper_agregator);
    let repository = if Arc::ptr_eq(&proxy_store, &scraper_agregator.proxy_store) {
        scraper_agregator.proxy_repository.clone()
    } else {
        Arc::new(arachnea_proxy::TypedProxyRepository::new(proxy_store))
    };
    let preferences = scraper_agregator.proxy_preferences.clone();
    ProxyInventory::new(
        InventoryConfig::default(),
        Some(Arc::new(ScrapyfyProxyDataProvider::new(scraper_agregator))),
        Some(Arc::new(ProxyProbe::new(probe_config))),
    )
    .with_ip_country_resolver(Arc::new(resolver))
    .with_proxy_repository(repository)
    .with_preference_repository(preferences)
}

/// Builds a default [`ArachneaProxyCore`] with dynamic country routing backed
/// by scrapyfy proxy sources.
///
/// The returned core uses [`arachnea_proxy::core::DynamicCountryRoutingProxyHandler`] with
/// `DynamicOnly` coexistence policy — a JSON set of allowed `countries` in the request
/// parameter triggers a `ProxyInventory` lookup that lazily loads proxy data through
/// [`ScrapyfyProxyDataProvider`].
///
/// # Arguments
///
/// * `scraper_agregator` - The aggregator instance that owns or will own this core.
/// * `proxy_store` - Typed proxy-record cache; matching the aggregator store reuses its repository.
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
#[path = "../../tests/scrapyfy/proxy_provider.rs"]
mod tests;
