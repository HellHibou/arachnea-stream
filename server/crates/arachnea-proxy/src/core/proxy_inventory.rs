use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::RwLock;

use crate::core::{
    Destination, ProxyDataProvider, ProxyProbe, ProxyRecord, ProxyRuntimeStatus, ProxyError,
    Result,
};

/// Policy controlling how static and dynamic proxy pools coexist.
#[derive(Clone, Debug, PartialEq)]
pub enum CoexistencePolicy {
    /// Use only statically configured pools; never load dynamic proxies.
    StaticOnly,
    /// Use only dynamic proxies; ignore statically configured pools.
    DynamicOnly,
    /// Try the static pool first. When it is empty or has no working member,
    /// fall back to dynamic proxies.
    StaticThenDynamic,
    /// Try dynamic proxies first. When no working candidate is found, fall
    /// back to the static pool.
    DynamicThenStatic,
}

/// Configuration for the runtime proxy inventory.
#[derive(Clone, Debug)]
pub struct InventoryConfig {
    /// How long a probed proxy is considered fresh before it must be retested.
    pub probe_ttl: Duration,
    /// How long a failed proxy must wait before it can be retried.
    pub ko_cooldown: Duration,
    /// How long a per-destination failure stays active before retrying.
    pub destination_failure_cooldown: Duration,
    /// Maximum number of active per-destination failures before the proxy is
    /// marked globally KO.
    pub max_destination_failures_before_ko: usize,
    /// How long to remember that a country returned no usable proxies.
    pub negative_cache_duration: Duration,
}

impl Default for InventoryConfig {
    fn default() -> Self {
        Self {
            probe_ttl: Duration::from_secs(600),
            ko_cooldown: Duration::from_secs(900),
            destination_failure_cooldown: Duration::from_secs(300),
            max_destination_failures_before_ko: 10,
            negative_cache_duration: Duration::from_secs(120),
        }
    }
}

/// Internal mutable state of the proxy inventory.
struct InventoryInner {
    records: HashMap<String, ProxyRecord>,
    country_records: HashMap<String, Vec<String>>,
    loading_countries: HashSet<String>,
    negative_cache: HashMap<String, Instant>,
}

/// Runtime inventory of dynamic proxy records.
///
/// Manages proxy storage by country, lazy loading via [`ProxyDataProvider`],
/// candidate selection with freshness and cooldown checks, and per-destination
/// failure tracking.
pub struct ProxyInventory {
    inner: RwLock<InventoryInner>,
    provider: Option<Arc<dyn ProxyDataProvider>>,
    probe: Option<Arc<ProxyProbe>>,
    config: InventoryConfig,
}

impl ProxyInventory {
    /// Creates a new proxy inventory.
    ///
    /// # Parameters
    ///
    /// - `config`: Inventory configuration (TTLs, cooldowns, thresholds).
    /// - `provider`: Optional data provider for lazy loading per country.
    /// - `probe`: Optional probe for testing newly loaded records.
    pub fn new(
        config: InventoryConfig,
        provider: Option<Arc<dyn ProxyDataProvider>>,
        probe: Option<Arc<ProxyProbe>>,
    ) -> Self {
        Self {
            inner: RwLock::new(InventoryInner {
                records: HashMap::new(),
                country_records: HashMap::new(),
                loading_countries: HashSet::new(),
                negative_cache: HashMap::new(),
            }),
            provider,
            probe,
            config,
        }
    }

    /// Returns the inventory configuration.
    pub fn config(&self) -> &InventoryConfig {
        &self.config
    }

    /// Returns the number of records currently stored.
    pub async fn len(&self) -> usize {
        self.inner.read().await.records.len()
    }

    /// Returns `true` when the inventory is empty.
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    /// Returns the provider, if one is configured.
    pub fn provider(&self) -> Option<&Arc<dyn ProxyDataProvider>> {
        self.provider.as_ref()
    }

    /// Returns the probe, if one is configured.
    pub fn probe(&self) -> Option<&Arc<ProxyProbe>> {
        self.probe.as_ref()
    }

    // ── Record management ────────────────────────────────────────────

    /// Adds or updates a batch of proxy records.
    ///
    /// Existing records with the same authority are merged: runtime fields
    /// (`status`, `latency_ms`, `failure_count`, `authentication_required`,
    /// `last_checked`, `cooldown_until`, `destination_failures`) are preserved
    /// from the existing record.
    pub async fn add_or_update(&self, records: Vec<ProxyRecord>) {
        let mut inner = self.inner.write().await;
        for record in records {
            let key = record.authority();
            let old_country = inner.records.get(&key).and_then(|r| r.country.clone());
            if let Some(existing) = inner.records.get(&key) {
                let mut merged = record.clone();
                merged.status = existing.status.clone();
                merged.latency_ms = existing.latency_ms;
                merged.failure_count = existing.failure_count;
                merged.authentication_required = existing.authentication_required;
                merged.last_checked = existing.last_checked;
                merged.cooldown_until = existing.cooldown_until;
                merged.destination_failures = existing.destination_failures.clone();

                let country_changed =
                    existing.country.as_deref() != merged.country.as_deref();
                if country_changed {
                    if let Some(ref old) = old_country {
                        if let Some(keys) = inner.country_records.get_mut(old) {
                            keys.retain(|k| k != &key);
                        }
                    }
                }

                inner.records.insert(key.clone(), merged);
            } else {
                inner.records.insert(key.clone(), record.clone());
            }

            if let Some(ref country) = record.country {
                inner
                    .country_records
                    .entry(country.clone())
                    .or_default()
                    .push(key);
            }
        }
    }

    /// Removes a proxy record by its normalised authority.
    pub async fn remove(&self, authority: &str) {
        let mut inner = self.inner.write().await;
        if let Some(record) = inner.records.remove(authority) {
            if let Some(ref country) = record.country {
                if let Some(keys) = inner.country_records.get_mut(country) {
                    keys.retain(|k| k != authority);
                }
            }
        }
    }

    /// Returns a snapshot of all stored records.
    pub async fn all_records(&self) -> Vec<ProxyRecord> {
        self.inner
            .read()
            .await
            .records
            .values()
            .cloned()
            .collect()
    }

    /// Returns records for a specific country.
    pub async fn records_for_country(&self, country: &str) -> Vec<ProxyRecord> {
        let inner = self.inner.read().await;
        inner
            .country_records
            .get(country)
            .map(|keys| {
                keys.iter()
                    .filter_map(|k| inner.records.get(k).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns the countries that have at least one record.
    pub async fn known_countries(&self) -> Vec<String> {
        let mut countries: Vec<String> = self
            .inner
            .read()
            .await
            .country_records
            .keys()
            .cloned()
            .collect();
        countries.sort();
        countries
    }

    // ── Selection ────────────────────────────────────────────────────

    /// Selects the best available proxy for a country.
    ///
    /// When no candidate is available and a provider is configured, triggers a
    /// lazy load and retries once.
    ///
    /// # Parameters
    ///
    /// - `country`: ISO country code.
    /// - `require_https`: If `true`, only proxies with confirmed HTTPS support
    ///   are eligible.
    ///
    /// # Returns
    ///
    /// The best matching proxy record.
    ///
    /// # Errors
    ///
    /// Returns an error when no candidate is available after loading.
    pub async fn select(&self, country: &str, require_https: bool) -> Result<ProxyRecord> {
        if let Some(record) = self.select_cached(country, require_https).await {
            return Ok(record);
        }

        self.load_if_needed(country).await;

        self.select_cached(country, require_https)
            .await
            .ok_or_else(|| {
                ProxyError::RouteUnavailable(format!(
                    "no working proxy available for country '{country}'"
                ))
            })
    }

    /// Selects from cached records only, without triggering a load.
    async fn select_cached(&self, country: &str, require_https: bool) -> Option<ProxyRecord> {
        let inner = self.inner.read().await;
        let now = SystemTime::now();

        let keys = inner.country_records.get(country)?;
        let mut candidates: Vec<&ProxyRecord> = keys
            .iter()
            .filter_map(|k| inner.records.get(k))
            .filter(|r| is_eligible(r, require_https, now))
            .collect();

        if candidates.is_empty() {
            return None;
        }

        candidates.sort_by(|a, b| {
            a.latency_ms
                .unwrap_or(u64::MAX)
                .cmp(&b.latency_ms.unwrap_or(u64::MAX))
                .then(a.failure_count.cmp(&b.failure_count))
        });

        Some(candidates[0].clone())
    }

    /// Loads proxies from the provider for a country, unless a load is
    /// already in progress or the country is in the negative cache.
    async fn load_if_needed(&self, country: &str) {
        let provider = match &self.provider {
            Some(p) => p,
            None => return,
        };

        {
            let inner = self.inner.read().await;
            if inner.loading_countries.contains(country) {
                return;
            }
            if let Some(expires_at) = inner.negative_cache.get(country) {
                if Instant::now() < *expires_at {
                    return;
                }
            }
        }

        {
            let mut inner = self.inner.write().await;
            if !inner.loading_countries.insert(country.to_string()) {
                return;
            }
        }

        let result = provider
            .load_proxies(crate::core::ProxyLoadRequest {
                country: Some(country.to_string()),
            })
            .await;

        match result {
            Ok(mut records) => {
                if let Some(probe) = &self.probe {
                    for record in &mut records {
                        let _ = probe.probe(record).await;
                    }
                }
                self.add_or_update(records).await;

                let has_usable = self.select_cached(country, false).await.is_some();
                if !has_usable {
                    let mut inner = self.inner.write().await;
                    inner.negative_cache.insert(
                        country.to_string(),
                        Instant::now() + self.config.negative_cache_duration,
                    );
                }
            }
            Err(error) => {
                tracing::warn!(country = %country, %error, "failed to load proxies");
                let mut inner = self.inner.write().await;
                inner.negative_cache.insert(
                    country.to_string(),
                    Instant::now() + self.config.negative_cache_duration,
                );
            }
        }

        let mut inner = self.inner.write().await;
        inner.loading_countries.remove(country);
    }

    // ── Failure tracking ─────────────────────────────────────────────

    /// Records a per-destination failure against a proxy.
    ///
    /// When the number of active destination failures reaches
    /// `max_destination_failures_before_ko`, the proxy is marked globally KO
    /// and its destination failures list is cleared.
    ///
    /// # Returns
    ///
    /// `true` when the proxy was marked globally KO due to the threshold.
    pub async fn record_destination_failure(
        &self,
        authority: &str,
        destination: &Destination,
        reason: crate::core::ProxyDestinationFailureReason,
    ) -> bool {
        let mut inner = self.inner.write().await;
        let record = match inner.records.get_mut(authority) {
            Some(r) => r,
            None => return false,
        };

        let now = SystemTime::now();
        let failure_host = destination.host_for_protocol();

        let entry = record.destination_failures.iter_mut().find(|f| {
            f.scheme == destination_scheme(destination)
                && f.host == failure_host
                && f.port == destination.port
        });

        match entry {
            Some(failure) => {
                failure.failure_count += 1;
                failure.last_failed = now;
                failure.reason = reason;
                failure.cooldown_until =
                    Some(now + self.config.destination_failure_cooldown);
            }
            None => {
                record.destination_failures.push(
                    crate::core::ProxyDestinationFailure {
                        scheme: destination_scheme(destination),
                        host: failure_host,
                        port: destination.port,
                        reason,
                        failure_count: 1,
                        last_failed: now,
                        cooldown_until: Some(now + self.config.destination_failure_cooldown),
                    },
                );
            }
        }

        if record.destination_failures.len() >= self.config.max_destination_failures_before_ko {
            record.status = ProxyRuntimeStatus::Ko;
            record.failure_count += 1;
            record.cooldown_until = Some(now + self.config.ko_cooldown);
            record.destination_failures.clear();
            return true;
        }

        false
    }

    /// Marks a proxy as globally failed (KO) with cooldown.
    pub async fn record_global_failure(&self, authority: &str) {
        let mut inner = self.inner.write().await;
        if let Some(record) = inner.records.get_mut(authority) {
            let now = SystemTime::now();
            record.status = ProxyRuntimeStatus::Ko;
            record.failure_count += 1;
            record.cooldown_until = Some(now + self.config.ko_cooldown);
            record.destination_failures.clear();
        }
    }

    /// Records that a proxy requires authentication.
    pub async fn record_auth_required(&self, authority: &str) {
        let mut inner = self.inner.write().await;
        if let Some(record) = inner.records.get_mut(authority) {
            record.status = ProxyRuntimeStatus::AuthenticationRequired;
            record.authentication_required = Some(true);
            record.failure_count += 1;
        }
    }

    /// Updates a record's status to `Ok` and clears failure state.
    pub async fn record_ok(&self, authority: &str) {
        let mut inner = self.inner.write().await;
        if let Some(record) = inner.records.get_mut(authority) {
            record.status = ProxyRuntimeStatus::Ok;
            record.failure_count = 0;
            record.cooldown_until = None;
            record.destination_failures.clear();
        }
    }
}

// ── Eligibility helpers ───────────────────────────────────────────────

fn is_eligible(record: &ProxyRecord, require_https: bool, now: SystemTime) -> bool {
    if !matches!(record.status, ProxyRuntimeStatus::Ok) {
        return false;
    }

    if record.authentication_required == Some(true) {
        return false;
    }

    if let Some(cooldown) = record.cooldown_until {
        if cooldown > now {
            return false;
        }
    }

    if require_https {
        match record.supports_https {
            Some(true) => {}
            Some(false) => return false,
            None => {}
        }
    }

    true
}

fn destination_scheme(destination: &Destination) -> String {
    match destination.protocol {
        crate::core::ApplicationProtocol::Http => "http".to_string(),
        crate::core::ApplicationProtocol::Https => "https".to_string(),
        _ => "tcp".to_string(),
    }
}
