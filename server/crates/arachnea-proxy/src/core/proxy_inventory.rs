use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::RwLock;

use crate::core::proxy_store::ProxyStore;
use crate::core::{
    Destination, IpCountryResolver, ProxyDataProvider, ProxyError, ProxyProbe, ProxyProtocol,
    ProxyRecord, ProxyRuntimeStatus, Result,
};

/// Policy controlling how static and dynamic proxy pools coexist.
#[derive(Clone, Debug, Eq, PartialEq)]
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
    /// Maximum number of concurrent probes when testing newly loaded proxies.
    pub probe_batch_size: usize,
}

impl Default for InventoryConfig {
    fn default() -> Self {
        Self {
            probe_ttl: Duration::from_secs(600),
            ko_cooldown: Duration::from_secs(900),
            destination_failure_cooldown: Duration::from_secs(300),
            max_destination_failures_before_ko: 10,
            negative_cache_duration: Duration::from_secs(120),
            probe_batch_size: 8,
        }
    }
}

/// Internal mutable state of the proxy inventory.
struct InventoryInner {
    records: HashMap<String, ProxyRecord>,
    country_records: HashMap<String, Vec<String>>,
    loading_countries: HashSet<String>,
    negative_cache: HashMap<String, Instant>,
    /// Last selected proxy authority per country, for sticky selection.
    #[allow(dead_code)]
    last_selected: HashMap<String, String>,
}

/// Runtime inventory of dynamic proxy records.
///
/// Manages proxy storage by country, lazy loading via [`ProxyDataProvider`],
/// candidate selection with freshness and cooldown checks, and per-destination
/// failure tracking.
///
/// When a resolver is configured, records whose `country` is `None` but whose
/// `host` is a valid IP address may be resolved during strict country selection.
/// The resolution is bounded by the resolver's timeout and deduplicated to
/// prevent concurrent duplicate lookups.
pub struct ProxyInventory {
    inner: RwLock<InventoryInner>,
    provider: Option<Arc<dyn ProxyDataProvider>>,
    probe: Option<Arc<ProxyProbe>>,
    store: Option<Arc<dyn ProxyStore>>,
    config: InventoryConfig,
    ip_country_resolver: Option<Arc<IpCountryResolver>>,
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
                last_selected: HashMap::new(),
            }),
            provider,
            probe,
            store: None,
            config,
            ip_country_resolver: None,
        }
    }

    /// Creates a new proxy inventory with a persistent store.
    ///
    /// # Parameters
    ///
    /// - `config`: Inventory configuration (TTLs, cooldowns, thresholds).
    /// - `provider`: Optional data provider for lazy loading per country.
    /// - `probe`: Optional probe for testing newly loaded records.
    /// - `store`: Persistent store used for explicit loads and automatic saves
    ///   after lazy loading.
    pub fn with_store(
        config: InventoryConfig,
        provider: Option<Arc<dyn ProxyDataProvider>>,
        probe: Option<Arc<ProxyProbe>>,
        store: Arc<dyn ProxyStore>,
    ) -> Self {
        let mut inventory = Self::new(config, provider, probe);
        inventory.store = Some(store);
        inventory
    }

    /// Sets an IP-country resolver for on-demand geolocation during strict
    /// country selection.
    ///
    /// When a resolver is configured and the inventory encounters a proxy record
    /// whose `country` is `None` but whose `host` parses as a valid IP address,
    /// it attempts a synchronous bounded resolution via the resolver before
    /// excluding the record from selection.
    ///
    /// # Parameters
    ///
    /// - `resolver`: IP-country resolver.
    ///
    /// # Returns
    ///
    /// Self for chaining.
    pub fn with_ip_country_resolver(mut self, resolver: Arc<IpCountryResolver>) -> Self {
        self.ip_country_resolver = Some(resolver);
        self
    }

    /// Returns the IP-country resolver, if one is configured.
    pub fn ip_country_resolver(&self) -> Option<&Arc<IpCountryResolver>> {
        self.ip_country_resolver.as_ref()
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

    /// Returns the persistent store, if one is configured.
    pub fn store(&self) -> Option<&Arc<dyn ProxyStore>> {
        self.store.as_ref()
    }

    async fn save_configured_store(&self) {
        let Some(store) = &self.store else {
            return;
        };
        if let Err(error) = self.save_to_store(store.as_ref()).await {
            tracing::warn!(%error, "failed to persist proxy inventory");
        }
    }

    // ── Record management ────────────────────────────────────────────

    /// Adds or updates a batch of proxy records.
    ///
    /// Existing records with the same authority are merged. Hard exclusion
    /// state (`Ko`, authentication required, cooldowns, and destination
    /// failures) is preserved, while a fresh probed source record may refresh
    /// latency, HTTPS support and `last_checked` for otherwise eligible
    /// records.
    ///
    /// When an IP-country resolver is configured, records whose `country` is
    /// `None` and whose `host` is a valid IP address are resolved through the
    /// resolver before insertion. The resolution is bounded by the resolver's
    /// timeout.
    pub async fn add_or_update(&self, records: Vec<ProxyRecord>) {
        // Resolve missing countries for records whose host is an IP address
        let records = self.resolve_ip_countries(records).await;

        let mut inner = self.inner.write().await;
        for record in records {
            let key = record.authority();
            let old_country = inner.records.get(&key).and_then(|r| r.country.clone());
            if let Some(existing) = inner.records.get(&key) {
                let mut merged = record.clone();
                if should_preserve_runtime_exclusion(existing) || !has_fresh_runtime_update(&record)
                {
                    merged.status = existing.status.clone();
                    merged.latency_ms = existing.latency_ms;
                    merged.failure_count = existing.failure_count;
                    merged.authentication_required = existing.authentication_required;
                    merged.last_checked = existing.last_checked;
                    merged.cooldown_until = existing.cooldown_until;
                }
                merged.destination_failures = existing.destination_failures.clone();

                let country_changed = existing.country.as_deref() != merged.country.as_deref();
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

    /// Resolves missing country codes for records whose host is a valid IP
    /// address, using the configured IP-country resolver.
    ///
    /// This is a bounded synchronous resolution: it waits for the resolver's
    /// response with a configurable timeout and deduplicates concurrent
    /// lookups for the same IP.
    async fn resolve_ip_countries(&self, mut records: Vec<ProxyRecord>) -> Vec<ProxyRecord> {
        let Some(resolver) = &self.ip_country_resolver else {
            return records;
        };

        // Load the resolver from store if not already loaded
        if let Err(error) = resolver.load_from_store().await {
            tracing::warn!(%error, "failed to load IP-country resolver from store");
        }

        let mut resolved_any = false;
        for record in records.iter_mut() {
            if record.country.is_some() {
                continue;
            }

            // Only attempt resolution for records whose host is a valid IP
            let ip: IpAddr = match record.host.parse() {
                Ok(ip) => ip,
                Err(_) => continue,
            };

            match resolver.resolve(&ip).await {
                Ok(Some(country)) => {
                    tracing::debug!(
                        host = %record.host,
                        country = %country,
                        authority = %record.authority(),
                        "resolved missing country code for proxy record"
                    );
                    record.country = Some(country);
                    resolved_any = true;
                }
                Ok(None) => {
                    tracing::trace!(
                        host = %record.host,
                        "IP-country resolver returned no country for proxy record"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        host = %record.host,
                        %error,
                        "IP-country resolution failed for proxy record"
                    );
                }
            }
        }

        if resolved_any {
            tracing::info!(
                total = records.len(),
                "completed IP-country resolution for proxy records during add_or_update"
            );
        }

        records
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
        self.inner.read().await.records.values().cloned().collect()
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
        self.select_for_destination(country, require_https, None)
            .await
    }

    /// Selects the best available proxy for a country and destination.
    ///
    /// Destination-specific cooldowns are applied when `destination` is
    /// supplied. When no candidate is available and a provider is configured,
    /// this triggers a lazy load and retries once.
    ///
    /// # Arguments
    ///
    /// - `country`: ISO country code.
    /// - `require_https`: If `true`, only proxies that can reach HTTPS
    ///   destinations are eligible.
    /// - `destination`: Optional final destination used to exclude proxies
    ///   temporarily cooled down for that origin.
    ///
    /// # Errors
    ///
    /// Returns an error when no candidate is available after loading.
    pub async fn select_for_destination(
        &self,
        country: &str,
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Result<ProxyRecord> {
        if let Some(record) = self
            .select_cached(country, require_https, destination)
            .await
        {
            return Ok(record);
        }

        self.load_if_needed(country).await;

        self.select_cached(country, require_https, destination)
            .await
            .ok_or_else(|| {
                ProxyError::RouteUnavailable(format!(
                    "no working proxy available for country '{country}'"
                ))
            })
    }

    /// Selects from cached records only, without triggering a load.
    async fn select_cached(
        &self,
        country: &str,
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Option<ProxyRecord> {
        let inner = self.inner.read().await;
        let now = SystemTime::now();

        let Some(keys) = inner.country_records.get(country) else {
            tracing::debug!(
                country = %country,
                require_https,
                "proxy inventory has no records for country"
            );
            return None;
        };
        let records: Vec<&ProxyRecord> = keys.iter().filter_map(|k| inner.records.get(k)).collect();
        let stats = selection_stats(
            records.iter().copied(),
            country,
            require_https,
            destination,
            now,
            self.config.probe_ttl,
        );
        tracing::debug!(
            country = %country,
            require_https,
            total = stats.total,
            ok = stats.ok,
            unknown = stats.unknown,
            ko = stats.ko,
            auth_required = stats.authentication_required,
            auth_flag = stats.authentication_required_flag,
            cooldown = stats.cooldown,
            probe_expired = stats.probe_expired,
            destination_cooldown = stats.destination_cooldown,
            https_rejected = stats.https_rejected,
            eligible = stats.eligible,
            "proxy inventory selection candidates"
        );

        let mut candidates: Vec<&ProxyRecord> = records
            .into_iter()
            .filter(|r| {
                is_eligible(
                    r,
                    country,
                    require_https,
                    destination,
                    now,
                    self.config.probe_ttl,
                )
            })
            .collect();

        if candidates.is_empty() {
            return None;
        }

        candidates.sort_by(|a, b| {
            a.latency_ms
                .unwrap_or(u64::MAX)
                .cmp(&b.latency_ms.unwrap_or(u64::MAX))
                .then(a.failure_count.cmp(&b.failure_count))
                .then_with(|| a.authority().cmp(&b.authority()))
        });

        let selected = candidates[0];
        tracing::debug!(
            country = %country,
            require_https,
            selected = %selected.authority(),
            protocol = ?selected.protocol,
            supports_https = ?selected.supports_https,
            latency_ms = ?selected.latency_ms,
            "selected dynamic proxy candidate"
        );
        Some(selected.clone())
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
                let loaded_count = records.len();
                if let Some(probe) = &self.probe {
                    probe
                        .probe_batch(&mut records, self.config.probe_batch_size)
                        .await;
                }
                let probe_stats = selection_stats(
                    records.iter(),
                    country,
                    false,
                    None,
                    SystemTime::now(),
                    self.config.probe_ttl,
                );
                tracing::info!(
                    country = %country,
                    loaded_count,
                    ok = probe_stats.ok,
                    unknown = probe_stats.unknown,
                    ko = probe_stats.ko,
                    auth_required = probe_stats.authentication_required,
                    auth_flag = probe_stats.authentication_required_flag,
                    "loaded dynamic proxies probed"
                );
                self.add_or_update(records).await;

                let http_stats = self
                    .selection_stats_for_country(country, false)
                    .await
                    .unwrap_or_default();
                let https_stats = self
                    .selection_stats_for_country(country, true)
                    .await
                    .unwrap_or_default();
                tracing::info!(
                    country = %country,
                    total = http_stats.total,
                    ok = http_stats.ok,
                    eligible_http = http_stats.eligible,
                    eligible_https = https_stats.eligible,
                    https_rejected = https_stats.https_rejected,
                    ko = http_stats.ko,
                    auth_required = http_stats.authentication_required,
                    cooldown = http_stats.cooldown,
                    "dynamic proxy inventory updated"
                );

                let has_usable = self.select_cached(country, false, None).await.is_some();
                if !has_usable {
                    let mut inner = self.inner.write().await;
                    inner.negative_cache.insert(
                        country.to_string(),
                        Instant::now() + self.config.negative_cache_duration,
                    );
                }

                self.save_configured_store().await;
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

    async fn selection_stats_for_country(
        &self,
        country: &str,
        require_https: bool,
    ) -> Option<SelectionStats> {
        let inner = self.inner.read().await;
        let now = SystemTime::now();
        let keys = inner.country_records.get(country)?;
        let records = keys.iter().filter_map(|k| inner.records.get(k));
        Some(selection_stats(
            records,
            country,
            require_https,
            None,
            now,
            self.config.probe_ttl,
        ))
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
        let marked_ko = {
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
                    failure.cooldown_until = Some(now + self.config.destination_failure_cooldown);
                }
                None => {
                    record
                        .destination_failures
                        .push(crate::core::ProxyDestinationFailure {
                            scheme: destination_scheme(destination),
                            host: failure_host,
                            port: destination.port,
                            reason,
                            failure_count: 1,
                            last_failed: now,
                            cooldown_until: Some(now + self.config.destination_failure_cooldown),
                        });
                }
            }

            if record.destination_failures.len() >= self.config.max_destination_failures_before_ko {
                record.status = ProxyRuntimeStatus::Ko;
                record.failure_count += 1;
                record.cooldown_until = Some(now + self.config.ko_cooldown);
                record.destination_failures.clear();
                true
            } else {
                false
            }
        };

        self.save_configured_store().await;
        marked_ko
    }

    /// Marks a proxy as globally failed (KO) with cooldown.
    pub async fn record_global_failure(&self, authority: &str) {
        {
            let mut inner = self.inner.write().await;
            if let Some(record) = inner.records.get_mut(authority) {
                let now = SystemTime::now();
                record.status = ProxyRuntimeStatus::Ko;
                record.failure_count += 1;
                record.cooldown_until = Some(now + self.config.ko_cooldown);
                record.destination_failures.clear();
            }
        }
        self.save_configured_store().await;
    }

    /// Records that a proxy requires authentication.
    pub async fn record_auth_required(&self, authority: &str) {
        {
            let mut inner = self.inner.write().await;
            if let Some(record) = inner.records.get_mut(authority) {
                record.status = ProxyRuntimeStatus::AuthenticationRequired;
                record.authentication_required = Some(true);
                record.failure_count += 1;
            }
        }
        self.save_configured_store().await;
    }

    /// Updates a record's status to `Ok` and clears failure state.
    pub async fn record_ok(&self, authority: &str) {
        {
            let mut inner = self.inner.write().await;
            if let Some(record) = inner.records.get_mut(authority) {
                record.status = ProxyRuntimeStatus::Ok;
                record.failure_count = 0;
                record.cooldown_until = None;
                record.destination_failures.clear();
            }
        }
        self.save_configured_store().await;
    }
}

// ── Eligibility helpers ───────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default)]
struct SelectionStats {
    total: usize,
    ok: usize,
    unknown: usize,
    ko: usize,
    authentication_required: usize,
    authentication_required_flag: usize,
    cooldown: usize,
    probe_expired: usize,
    destination_cooldown: usize,
    https_rejected: usize,
    eligible: usize,
}

fn selection_stats<'a>(
    records: impl IntoIterator<Item = &'a ProxyRecord>,
    country: &str,
    require_https: bool,
    destination: Option<&Destination>,
    now: SystemTime,
    probe_ttl: Duration,
) -> SelectionStats {
    let mut stats = SelectionStats::default();

    for record in records {
        stats.total += 1;
        match record.status {
            ProxyRuntimeStatus::Ok => stats.ok += 1,
            ProxyRuntimeStatus::Unknown => stats.unknown += 1,
            ProxyRuntimeStatus::Ko => stats.ko += 1,
            ProxyRuntimeStatus::AuthenticationRequired => stats.authentication_required += 1,
        }

        if record.authentication_required == Some(true) {
            stats.authentication_required_flag += 1;
        }

        if record.cooldown_until.is_some_and(|cooldown| cooldown > now) {
            stats.cooldown += 1;
        }

        if probe_is_expired(record, now, probe_ttl) {
            stats.probe_expired += 1;
        }

        if destination
            .is_some_and(|destination| has_active_destination_cooldown(record, destination, now))
        {
            stats.destination_cooldown += 1;
        }

        if require_https && !can_reach_https_destination(record) {
            stats.https_rejected += 1;
        }

        if is_eligible(record, country, require_https, destination, now, probe_ttl) {
            stats.eligible += 1;
        }
    }

    stats
}

fn is_eligible(
    record: &ProxyRecord,
    country: &str,
    require_https: bool,
    destination: Option<&Destination>,
    now: SystemTime,
    probe_ttl: Duration,
) -> bool {
    if record.country.as_deref() != Some(country) {
        return false;
    }

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

    if probe_is_expired(record, now, probe_ttl) {
        return false;
    }

    if destination
        .is_some_and(|destination| has_active_destination_cooldown(record, destination, now))
    {
        return false;
    }

    if require_https && !can_reach_https_destination(record) {
        return false;
    }

    true
}

fn should_preserve_runtime_exclusion(record: &ProxyRecord) -> bool {
    matches!(
        record.status,
        ProxyRuntimeStatus::Ko | ProxyRuntimeStatus::AuthenticationRequired
    ) || record.authentication_required == Some(true)
        || record.cooldown_until.is_some()
}

fn has_fresh_runtime_update(record: &ProxyRecord) -> bool {
    record.last_checked.is_some() && !matches!(record.status, ProxyRuntimeStatus::Unknown)
}

fn probe_is_expired(record: &ProxyRecord, now: SystemTime, probe_ttl: Duration) -> bool {
    let Some(last_checked) = record.last_checked else {
        return true;
    };

    now.duration_since(last_checked)
        .map(|age| age > probe_ttl)
        .unwrap_or(false)
}

fn has_active_destination_cooldown(
    record: &ProxyRecord,
    destination: &Destination,
    now: SystemTime,
) -> bool {
    let scheme = destination_scheme(destination);
    let host = destination.host_for_protocol();
    record.destination_failures.iter().any(|failure| {
        failure.scheme == scheme
            && failure.host == host
            && failure.port == destination.port
            && failure
                .cooldown_until
                .is_some_and(|cooldown| cooldown > now)
    })
}

fn can_reach_https_destination(record: &ProxyRecord) -> bool {
    if matches!(
        record.protocol,
        Some(ProxyProtocol::Socks4 | ProxyProtocol::Socks4a | ProxyProtocol::Socks5)
    ) {
        return true;
    }

    record.supports_https != Some(false)
}

fn destination_scheme(destination: &Destination) -> String {
    match destination.protocol {
        crate::core::ApplicationProtocol::Http => "http".to_string(),
        crate::core::ApplicationProtocol::Https => "https".to_string(),
        _ => "tcp".to_string(),
    }
}