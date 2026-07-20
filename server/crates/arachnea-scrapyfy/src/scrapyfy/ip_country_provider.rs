use std::collections::HashMap;
use std::net::IpAddr;

use async_trait::async_trait;

use arachnea_proxy::core::{
    IpCountryDataProvider, IpCountryRecord, IpCountrySerdeStore, IpCountryStore,
    JsonIpCountryCodec, Result,
};
use tracing::{info, warn};

use crate::scrapyfy::ScraperAgregator;

const IP_COUNTRY_GROUP_NAME: &str = "arachnea-ip-countries";

/// IP geolocation provider backed by the scrapyfy scraper infrastructure.
///
/// Uses a YAML-defined query to resolve IP addresses to country codes.
/// The provider stores a raw pointer to [`ScraperAgregator`], which must
/// outlive this provider.
pub struct ScrapyfyIpCountryDataProvider {
    scraper_agregator: *const ScraperAgregator,
}

// SAFETY: the raw pointer points to the owning ScraperAgregator which lives
// longer than this provider.
unsafe impl Send for ScrapyfyIpCountryDataProvider {}
unsafe impl Sync for ScrapyfyIpCountryDataProvider {}

impl ScrapyfyIpCountryDataProvider {
    /// Creates a new provider backed by the given aggregator.
    ///
    /// The aggregator must be pinned and must outlive this provider.
    pub fn new(scraper_agregator: &mut ScraperAgregator) -> Self {
        let preferred_config_path = format!(
            "{}/{}/{}",
            crate::DEFAULT_SERVICES_DIRECTORY,
            IP_COUNTRY_GROUP_NAME,
            "services.json"
        );
        if let Err(preferred_error) = scraper_agregator
            .add_query_collection_from_config_json(IP_COUNTRY_GROUP_NAME, &preferred_config_path)
        {
            warn!(
                error = %preferred_error,
                "failed to load IP-country query collection"
            );
        }

        ScrapyfyIpCountryDataProvider {
            scraper_agregator: scraper_agregator as *const ScraperAgregator,
        }
    }

    /// Resolves a batch of IP addresses to their country codes.
    ///
    /// Calls the YAML-defined geolocation query for each IP and returns
    /// records for successfully resolved addresses.
    ///
    /// # Parameters
    ///
    /// - `ips`: IP addresses to resolve.
    ///
    /// # Returns
    ///
    /// A list of IP-country records for successfully resolved addresses.
    pub async fn resolve_ips(&self, ips: &[IpAddr]) -> Result<Vec<IpCountryRecord>> {
        if ips.is_empty() {
            return Ok(Vec::new());
        }

        let agregator = unsafe { &*self.scraper_agregator };

        let mut results = Vec::with_capacity(ips.len());

        for ip in ips {
            let ip_str = ip.to_string();
            let mut params = HashMap::new();
            params.insert("ip".to_string(), ip_str.clone());

            let query_results = agregator
                .execute_query_async(
                    IP_COUNTRY_GROUP_NAME,
                    "resolve_ip_country",
                    &params,
                    None,
                    None,
                    None,
                    None,
                    None,
                    "resolve_ip_country",
                )
                .await;

            for row in &query_results.data {
                if let Some(record) = row_to_ip_country_record(row, ip) {
                    results.push(record);
                    break; // one record per IP
                }
            }
        }

        info!(
            resolved = results.len(),
            total = ips.len(),
            "IP-country resolution complete"
        );
        Ok(results)
    }
}

#[async_trait]
impl IpCountryDataProvider for ScrapyfyIpCountryDataProvider {
    async fn load_ip_countries(&self) -> Result<Vec<IpCountryRecord>> {
        // Full bulk refresh is delegated to the dedicated `refresh_ip_country_store`
        // function which coordinates with the proxy inventory. The trait method
        // returns empty here because this provider does not own a list of IPs to
        // resolve in isolation.
        Ok(Vec::new())
    }

    /// Resolves a single IP address to a country code via the YAML-defined
    /// geolocation query.
    ///
    /// This method is called by [`IpCountryResolver`] during bounded synchronous
    /// resolution when a proxy record's country is missing and a strict country
    /// selection is in progress.
    async fn resolve_ip_country(&self, ip: &IpAddr) -> Result<Option<String>> {
        let agregator = unsafe { &*self.scraper_agregator };

        let ip_str = ip.to_string();
        let mut params = HashMap::new();
        params.insert("ip".to_string(), ip_str.clone());

        let rows = agregator
            .execute_query_async(
                IP_COUNTRY_GROUP_NAME,
                "resolve_ip_country",
                &params,
                None,
                None,
                None,
                None,
                None,
                "resolve_ip_country",
            )
            .await
            .data;

        for row in &rows {
            if let Some(country) = row.get("country_code").and_then(|n| n.value_as_string()) {
                if !country.is_empty() {
                    let country = country.trim().to_ascii_uppercase();
                    return Ok(Some(country));
                }
            }
        }

        Ok(None)
    }
}

/// Converts a scraper result row into an `IpCountryRecord`.
fn row_to_ip_country_record(
    row: &HashMap<String, crate::scrapyfy::ScraperDataNode>,
    expected_ip: &IpAddr,
) -> Option<IpCountryRecord> {
    let country = row.get("country_code").and_then(|n| n.value_as_string())?;

    if country.is_empty() {
        return None;
    }

    Some(IpCountryRecord {
        ip: *expected_ip,
        country: country.trim().to_ascii_uppercase(),
        source: Some("ip-api.com".to_string()),
    })
}

/// Configuration for the IP-country refresh operation.
pub struct IpCountryRefreshConfig {
    /// Path to persist the IP-country data file.
    pub store_path: String,
}

impl Default for IpCountryRefreshConfig {
    fn default() -> Self {
        Self {
            store_path: "data/ip-countries.json".to_string(),
        }
    }
}

/// Refreshes the IP-country store by resolving all known proxy IPs.
///
/// This is the main entry point for populating the IP-country mapping.
/// It resolves IPs without a known country and persists the results.
///
/// # Parameters
///
/// - `aggregator`: Scraper aggregator with the IP-country query collection loaded.
/// - `provider`: IP-country data provider used for resolution.
/// - `ip_addresses`: List of IP addresses to resolve.
/// - `config`: Refresh configuration (store path, etc.).
///
/// # Errors
///
/// Returns an error when the store cannot be written.
pub async fn refresh_ip_country_store(
    aggregator: &ScraperAgregator,
    ip_addresses: &[IpAddr],
    config: &IpCountryRefreshConfig,
) -> Result<()> {
    let provider = ScrapyfyIpCountryDataProvider {
        scraper_agregator: aggregator as *const ScraperAgregator,
    };

    let records = provider.resolve_ips(ip_addresses).await?;

    let store = IpCountrySerdeStore::new(
        &config.store_path,
        std::sync::Arc::new(JsonIpCountryCodec::new()),
    );

    store.save_ip_countries(&records).await?;

    info!(
        path = %config.store_path,
        count = records.len(),
        "IP-country store refreshed"
    );

    Ok(())
}
