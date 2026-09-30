use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::{Mutex, RwLock};

#[cfg(feature = "persistence")]
use crate::core::ProxyRepository;
use crate::core::{
    Destination, IpCountryResolver, ProxyDataProvider, ProxyError, ProxyProbe, ProxyProtocol,
    ProxyRecord, ProxyRuntimeStatus, Result, PROXY_CACHE_TTL,
};

const PROXY_AFFINITY_TTL: Duration = Duration::from_secs(15 * 60);
/// Default duration for excluding a proxy from one exact destination after a
/// destination-scoped failure such as an HTTP 403 origin rejection.
pub const PROXY_DESTINATION_FAILURE_COOLDOWN: Duration = Duration::from_secs(24 * 60 * 60);

struct ProxyAffinityBinding {
    authority: String,
    expires_at: Instant,
}

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
    /// Maximum number of active non-origin-block destination failures before
    /// the proxy is marked globally KO.
    pub max_destination_failures_before_ko: usize,
    /// Maximum number of concurrent probes when testing newly loaded proxies.
    pub probe_batch_size: usize,
    /// Minimum delay between provider refresh attempts for the same country.
    pub provider_refresh_cooldown: Duration,
}

impl Default for InventoryConfig {
    fn default() -> Self {
        Self {
            probe_ttl: Duration::from_secs(600),
            ko_cooldown: Duration::from_secs(900),
            destination_failure_cooldown: PROXY_DESTINATION_FAILURE_COOLDOWN,
            max_destination_failures_before_ko: 10,
            probe_batch_size: 8,
            provider_refresh_cooldown: Duration::from_secs(120),
        }
    }
}

/// Internal mutable state of the proxy inventory.
struct InventoryInner {
    records: HashMap<String, ProxyRecord>,
    country_records: HashMap<String, Vec<String>>,
    country_load_locks: HashMap<String, Arc<Mutex<()>>>,
    country_last_refresh_attempts: HashMap<String, Instant>,
    /// Last successfully validated proxy authority per country and destination.
    preferred_by_destination: HashMap<String, String>,
    /// Opaque request affinity keys mapped to one eligible proxy authority.
    affinity_bindings: HashMap<String, ProxyAffinityBinding>,
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
    /// Persistent cache for dynamic proxy records, when configured.
    #[cfg(feature = "persistence")]
    persistence: Option<Arc<dyn ProxyRepository>>,
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
                country_load_locks: HashMap::new(),
                country_last_refresh_attempts: HashMap::new(),
                preferred_by_destination: HashMap::new(),
                affinity_bindings: HashMap::new(),
            }),
            provider,
            probe,
            #[cfg(feature = "persistence")]
            persistence: None,
            config,
            ip_country_resolver: None,
        }
    }

    /// Attaches a typed proxy repository used as a persistent cache.
    ///
    /// When configured, selection falls back to the store (filtered by country)
    /// before triggering a provider load, and mutations are written back
    /// through namespace-bound transactions committed at the end of each
    /// processing batch.
    #[cfg(feature = "persistence")]
    pub fn with_proxy_repository(mut self, persistence: Arc<dyn ProxyRepository>) -> Self {
        self.persistence = Some(persistence);
        self
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

    /// Returns the proxy repository, if one is configured.
    #[cfg(feature = "persistence")]
    pub fn persistence(&self) -> Option<&Arc<dyn ProxyRepository>> {
        self.persistence.as_ref()
    }

    /// Persists records through one transaction committed after the whole
    /// batch has been written into the namespace cache.
    #[cfg(feature = "persistence")]
    async fn persist_records(&self, records: &[ProxyRecord]) {
        let Some(persistence) = &self.persistence else {
            return;
        };
        if let Err(error) = persist_proxy_records(persistence, records).await {
            tracing::warn!(%error, count = records.len(), "failed to persist proxy records");
        }
    }

    /// Deletes the persisted record of an endpoint key, if persistence is
    /// configured.
    #[cfg(feature = "persistence")]
    async fn delete_persisted_record(&self, key: &crate::core::ProxyKey) {
        let Some(persistence) = &self.persistence else {
            return;
        };
        let result = persistence.delete(key).await;
        if let Err(error) = result {
            tracing::warn!(host = %key.host, port = key.port, %error, "failed to delete persisted proxy record");
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
    ///
    /// # Returns
    ///
    /// The canonical records retained by the inventory after merge and
    /// country-index updates, in input order.
    pub async fn add_or_update(&self, records: Vec<ProxyRecord>) -> Vec<ProxyRecord> {
        // Resolve missing countries for records whose host is an IP address
        let records = self.resolve_ip_countries(records).await;

        let mut inner = self.inner.write().await;
        let mut retained = Vec::with_capacity(records.len());
        let mut removed_keys = Vec::new();
        for record in records {
            let key = record.authority();
            let old_country = inner.records.get(&key).and_then(|r| r.country.clone());
            let remove_stale_failed_probe = inner.records.get(&key).is_some_and(|existing| {
                proxy_validation_is_stale(existing, SystemTime::now())
                    && has_newer_runtime_update(existing, &record)
                    && !matches!(record.status, ProxyRuntimeStatus::Ok)
            });
            if remove_stale_failed_probe {
                if let Some(existing) = inner.records.remove(&key) {
                    if let Some(country) = &existing.country {
                        if let Some(keys) = inner.country_records.get_mut(country) {
                            keys.retain(|authority| authority != &key);
                        }
                    }
                    removed_keys.push(existing.key());
                }
                tracing::debug!(
                    authority = %key,
                    "removed stale dynamic proxy after failed provider probe"
                );
                continue;
            }
            let retained_record = if let Some(existing) = inner.records.get(&key) {
                let mut merged = record.clone();
                if should_preserve_runtime_exclusion(existing) || !has_fresh_runtime_update(&record)
                {
                    merged.status = existing.status.clone();
                    merged.latency_ms = existing.latency_ms;
                    merged.failure_count = existing.failure_count;
                    merged.authentication_required = existing.authentication_required;
                    merged.last_checked = existing.last_checked;
                    merged.last_validated_at = existing.last_validated_at;
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

                inner.records.insert(key.clone(), merged.clone());
                merged
            } else {
                inner.records.insert(key.clone(), record.clone());
                record
            };

            if let Some(country) = &retained_record.country {
                let keys = inner.country_records.entry(country.clone()).or_default();
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            retained.push(retained_record);
        }
        drop(inner);

        #[cfg(feature = "persistence")]
        for key in removed_keys {
            self.delete_persisted_record(&key).await;
        }
        #[cfg(not(feature = "persistence"))]
        drop(removed_keys);

        retained
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
    ///
    /// The persisted record, when a persistence store is configured, is deleted
    /// as well.
    pub async fn remove(&self, authority: &str) {
        let removed_key = {
            let mut inner = self.inner.write().await;
            inner.records.remove(authority).map(|record| {
                if let Some(ref country) = record.country {
                    if let Some(keys) = inner.country_records.get_mut(country) {
                        keys.retain(|k| k != authority);
                    }
                }
                record.key()
            })
        };
        #[cfg(feature = "persistence")]
        if let Some(key) = removed_key {
            self.delete_persisted_record(&key).await;
        }
        #[cfg(not(feature = "persistence"))]
        let _ = removed_key;
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

    /// Selects a proxy from the first requested country that yields an eligible
    /// candidate. Countries are tried in caller-provided order.
    pub async fn select_any(
        &self,
        countries: &[String],
        require_https: bool,
    ) -> Result<ProxyRecord> {
        self.select_any_for_destination(countries, require_https, None)
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

        // Cache-first: import persisted records of this country from the
        // persistence store before falling back to the provider.
        #[cfg(feature = "persistence")]
        if self.load_cached_country(country).await {
            if let Some(record) = self
                .select_cached(country, require_https, destination)
                .await
            {
                return Ok(record);
            }
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

    /// Selects a proxy for a destination from the first country with an
    /// eligible candidate.
    ///
    /// All requested countries are searched in memory first, then in the
    /// persistent cache when enabled. The provider is consulted only if those
    /// complete cache passes yield no candidate. Provider loads then progress
    /// once through the countries in caller-provided order until one produces
    /// an eligible candidate.
    pub async fn select_any_for_destination(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Result<ProxyRecord> {
        self.select_any_for_destination_with_affinity(countries, require_https, destination, None)
            .await
    }

    /// Selects a proxy for a destination while reusing an eligible proxy bound
    /// to the supplied opaque affinity key.
    pub async fn select_any_for_destination_with_affinity(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
        affinity: Option<&str>,
    ) -> Result<ProxyRecord> {
        let mut attempted = Vec::new();
        for country in countries {
            let country = country.trim();
            if country.is_empty() || attempted.iter().any(|attempted| attempted == country) {
                continue;
            }
            attempted.push(country.to_string());
        }
        if attempted.is_empty() {
            return Err(ProxyError::RouteUnavailable(
                "no proxy countries were requested".to_string(),
            ));
        }

        let affinity = affinity.map(str::trim).filter(|value| !value.is_empty());
        if let Some(record) = self
            .select_affinity_binding(&attempted, require_https, destination, affinity)
            .await
        {
            return Ok(record);
        }

        if let Some(record) = self
            .select_cached_any(&attempted, require_https, destination)
            .await
        {
            self.bind_affinity(affinity, &record).await;
            return Ok(record);
        }

        #[cfg(feature = "persistence")]
        {
            for country in &attempted {
                self.load_cached_country(country).await;
            }
            if let Some(record) = self
                .select_cached_any(&attempted, require_https, destination)
                .await
            {
                self.bind_affinity(affinity, &record).await;
                return Ok(record);
            }
        }

        for country in &attempted {
            self.load_if_needed(country).await;
            if let Some(record) = self
                .select_cached_any(&attempted, require_https, destination)
                .await
            {
                self.bind_affinity(affinity, &record).await;
                return Ok(record);
            }
        }

        Err(ProxyError::RouteUnavailable(format!(
            "no working proxy available for requested countries [{}]",
            attempted.join(", ")
        )))
    }

    async fn select_affinity_binding(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
        affinity: Option<&str>,
    ) -> Option<ProxyRecord> {
        let affinity = affinity?;
        let now = Instant::now();
        let system_now = SystemTime::now();
        let mut inner = self.inner.write().await;
        inner
            .affinity_bindings
            .retain(|_, binding| binding.expires_at > now);
        let authority = inner.affinity_bindings.get(affinity)?.authority.clone();
        let record = inner.records.get(&authority).filter(|record| {
            record
                .country
                .as_ref()
                .is_some_and(|country| countries.iter().any(|wanted| wanted == country))
                && is_eligible(
                    record,
                    record.country.as_deref().unwrap_or_default(),
                    require_https,
                    destination,
                    system_now,
                )
        });
        if let Some(record) = record {
            return Some(record.clone());
        }
        inner.affinity_bindings.remove(affinity);
        None
    }

    async fn bind_affinity(&self, affinity: Option<&str>, record: &ProxyRecord) {
        let Some(affinity) = affinity else {
            return;
        };
        self.inner.write().await.affinity_bindings.insert(
            affinity.to_string(),
            ProxyAffinityBinding {
                authority: record.authority(),
                expires_at: Instant::now() + PROXY_AFFINITY_TTL,
            },
        );
    }

    /// Removes an affinity binding when its selected proxy is rejected.
    pub async fn clear_affinity(&self, affinity: &str, authority: &str) {
        let mut inner = self.inner.write().await;
        if inner
            .affinity_bindings
            .get(affinity)
            .is_some_and(|binding| binding.authority == authority)
        {
            inner.affinity_bindings.remove(affinity);
        }
    }

    /// Renews an affinity binding after an accepted origin response.
    pub async fn renew_affinity(&self, affinity: &str, authority: &str) {
        self.inner.write().await.affinity_bindings.insert(
            affinity.to_string(),
            ProxyAffinityBinding {
                authority: authority.to_string(),
                expires_at: Instant::now() + PROXY_AFFINITY_TTL,
            },
        );
    }

    /// Selects from the first requested country with a cached eligible record.
    async fn select_cached_any(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Option<ProxyRecord> {
        for country in countries {
            if let Some(record) = self
                .select_cached(country, require_https, destination)
                .await
            {
                return Some(record);
            }
        }
        None
    }

    /// Imports cached records for `country` from the persistence store into
    /// the in-memory inventory.
    ///
    /// # Returns
    ///
    /// `true` when at least one valid cached record was imported.
    #[cfg(feature = "persistence")]
    async fn load_cached_country(&self, country: &str) -> bool {
        let Some(persistence) = &self.persistence else {
            return false;
        };
        let result = persistence.find_by_country(country).await;
        let converted = match result {
            Ok(converted) => converted,
            Err(error) => {
                tracing::warn!(country = %country, %error, "failed to read cached proxy records");
                return false;
            }
        };
        if converted.is_empty() {
            return false;
        }
        let count = converted.len();
        self.add_or_update(converted).await;
        tracing::debug!(country = %country, count, "loaded cached proxy records from persistence");
        true
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
            .filter(|r| is_eligible(r, country, require_https, destination, now))
            .collect();

        if candidates.is_empty() {
            return None;
        }

        let preferred_authority = destination.and_then(|destination| {
            inner
                .preferred_by_destination
                .get(&destination_preference_key(country, destination))
        });
        candidates.sort_by(|a, b| {
            let a_preferred =
                preferred_authority.is_some_and(|authority| authority == &a.authority());
            let b_preferred =
                preferred_authority.is_some_and(|authority| authority == &b.authority());
            b_preferred.cmp(&a_preferred).then_with(|| {
                a.latency_ms
                    .unwrap_or(u64::MAX)
                    .cmp(&b.latency_ms.unwrap_or(u64::MAX))
                    .then(a.failure_count.cmp(&b.failure_count))
                    .then_with(|| a.authority().cmp(&b.authority()))
            })
        });

        let selected = candidates[0];
        tracing::debug!(
            country = %country,
            require_https,
            selected = %selected.authority(),
            protocol = ?selected.protocol,
            supports_https = ?selected.supports_https,
            latency_ms = ?selected.latency_ms,
            destination_preferred = preferred_authority
                .is_some_and(|authority| authority == &selected.authority()),
            "selected dynamic proxy candidate"
        );
        Some(selected.clone())
    }

    /// Loads proxies from the provider after selection found no eligible cached
    /// candidate.
    ///
    /// Calls for the same country share a per-country asynchronous lock and a
    /// refresh-attempt cooldown. One call performs the network work while
    /// concurrent or subsequent calls reuse inventory state until the cooldown
    /// expires, including when the provider failed or returned no candidates.
    /// Provider records are deduplicated against the inventory before probing:
    /// cached runtime capabilities are retained while their probe is fresh, and
    /// only new or expired records are tested.
    async fn load_if_needed(&self, country: &str) {
        let provider = match &self.provider {
            Some(p) => p,
            None => return,
        };

        let load_lock = {
            let mut inner = self.inner.write().await;
            Arc::clone(
                inner
                    .country_load_locks
                    .entry(country.to_string())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let _load_guard = load_lock.lock().await;

        {
            let mut inner = self.inner.write().await;
            let now = Instant::now();
            if let Some(last_attempt) = inner.country_last_refresh_attempts.get(country) {
                let elapsed = now.duration_since(*last_attempt);
                if elapsed < self.config.provider_refresh_cooldown {
                    tracing::debug!(
                        country = %country,
                        elapsed_ms = elapsed.as_millis(),
                        cooldown_ms = self.config.provider_refresh_cooldown.as_millis(),
                        "dynamic proxy provider refresh suppressed by inventory cooldown"
                    );
                    return;
                }
            }
            inner
                .country_last_refresh_attempts
                .insert(country.to_string(), now);
        }

        tracing::info!(
            country = %country,
            cooldown_seconds = self.config.provider_refresh_cooldown.as_secs(),
            "refreshing dynamic proxy provider after cache yielded no eligible candidate"
        );

        let result = provider
            .load_proxies(crate::core::ProxyLoadRequest {
                countries: vec![country.to_string()],
            })
            .await;

        match result {
            Ok(records) => {
                let loaded_count = records.len();
                let existing = self.inner.read().await.records.clone();
                let (mut records, probe_indices, duplicate_count, cached_count) =
                    prepare_provider_records(
                        records,
                        &existing,
                        SystemTime::now(),
                        self.config.probe_ttl,
                    );
                let probe_count = probe_indices.len();
                if let Some(probe) = &self.probe {
                    let mut records_to_probe = probe_indices
                        .iter()
                        .map(|index| records[*index].clone())
                        .collect::<Vec<_>>();
                    probe
                        .probe_batch(&mut records_to_probe, self.config.probe_batch_size)
                        .await;
                    for (index, probed) in probe_indices.into_iter().zip(records_to_probe) {
                        records[index] = probed;
                    }
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
                    deduplicated_count = records.len(),
                    duplicate_count,
                    cached_count,
                    probe_count,
                    ok = probe_stats.ok,
                    unknown = probe_stats.unknown,
                    ko = probe_stats.ko,
                    auth_required = probe_stats.authentication_required,
                    auth_flag = probe_stats.authentication_required_flag,
                    "loaded dynamic proxies prepared and probed"
                );
                let records_to_persist = self.add_or_update(records).await;

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

                #[cfg(feature = "persistence")]
                self.persist_records(&records_to_persist).await;
                #[cfg(not(feature = "persistence"))]
                let _ = records_to_persist;
            }
            Err(error) => {
                tracing::warn!(country = %country, %error, "failed to load proxies");
            }
        }
    }

    /// Allows the provider to be consulted again after an observed proxy
    /// connection failure.
    ///
    /// This does not load any data itself. It only clears the per-country
    /// refresh-attempt cooldown so a later selection may refresh the provider
    /// if all cached and persisted candidates have become ineligible. The
    /// failed proxy keeps its runtime exclusion and is therefore not revived by
    /// a provider response while its cached probe remains fresh.
    ///
    /// # Arguments
    ///
    /// - `countries`: Country codes associated with the failed dynamic pool.
    pub(crate) async fn allow_provider_refresh_after_observed_failure(&self, countries: &[String]) {
        let mut inner = self.inner.write().await;
        for country in countries {
            let country = country.trim();
            if country.is_empty() {
                continue;
            }
            if inner
                .country_last_refresh_attempts
                .remove(country)
                .is_some()
            {
                tracing::debug!(
                    country,
                    "dynamic proxy provider refresh cooldown cleared after observed connection failure"
                );
            }
        }
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
    /// When the number of active non-origin-block destination failures reaches
    /// `max_destination_failures_before_ko`, the proxy is marked globally KO
    /// and its destination failures list is cleared. `BlockedByOrigin`
    /// failures remain scoped to their destination and never contribute to the
    /// global failure threshold.
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
        let (outcome, removed_key) = {
            let mut inner = self.inner.write().await;
            let stale = inner
                .records
                .get(authority)
                .is_some_and(|record| proxy_validation_is_stale(record, SystemTime::now()));
            if stale {
                let removed = inner.records.remove(authority);
                if let Some(record) = &removed {
                    if let Some(country) = &record.country {
                        if let Some(keys) = inner.country_records.get_mut(country) {
                            keys.retain(|key| key != authority);
                        }
                        let preference_key = destination_preference_key(country, destination);
                        if inner
                            .preferred_by_destination
                            .get(&preference_key)
                            .is_some_and(|preferred| preferred == authority)
                        {
                            inner.preferred_by_destination.remove(&preference_key);
                        }
                    }
                }
                (None, removed.map(|record| record.key()))
            } else {
                let record = match inner.records.get_mut(authority) {
                    Some(record) => record,
                    None => return false,
                };

                let now = SystemTime::now();
                let failure_host = destination.host_for_protocol();
                let country = record.country.clone();

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
                        record
                            .destination_failures
                            .push(crate::core::ProxyDestinationFailure {
                                scheme: destination_scheme(destination),
                                host: failure_host,
                                port: destination.port,
                                reason,
                                failure_count: 1,
                                last_failed: now,
                                cooldown_until: Some(
                                    now + self.config.destination_failure_cooldown,
                                ),
                            });
                    }
                }

                let global_failure_count = record
                    .destination_failures
                    .iter()
                    .filter(|failure| {
                        failure.reason
                            != crate::core::ProxyDestinationFailureReason::BlockedByOrigin
                    })
                    .count();
                let outcome =
                    if global_failure_count >= self.config.max_destination_failures_before_ko {
                        record.status = ProxyRuntimeStatus::Ko;
                        record.failure_count += 1;
                        record.cooldown_until = Some(now + self.config.ko_cooldown);
                        record.destination_failures.clear();
                        (Some((true, record.clone())), None)
                    } else {
                        (Some((false, record.clone())), None)
                    };
                if let Some(country) = country {
                    let preference_key = destination_preference_key(&country, destination);
                    if inner
                        .preferred_by_destination
                        .get(&preference_key)
                        .is_some_and(|preferred| preferred == authority)
                    {
                        inner.preferred_by_destination.remove(&preference_key);
                    }
                }
                outcome
            }
        };

        #[cfg(feature = "persistence")]
        if let Some(key) = removed_key {
            self.delete_persisted_record(&key).await;
        }
        #[cfg(not(feature = "persistence"))]
        let _ = removed_key;

        let Some(outcome) = outcome else {
            tracing::debug!(
                authority,
                "removed stale dynamic proxy after observed failure"
            );
            return true;
        };
        #[cfg(feature = "persistence")]
        self.persist_records(std::slice::from_ref(&outcome.1)).await;
        outcome.0
    }

    /// Marks a proxy as globally failed (KO) with cooldown.
    pub async fn record_global_failure(&self, authority: &str) {
        if self.remove_if_stale(authority).await {
            return;
        }
        let updated = {
            let mut inner = self.inner.write().await;
            if let Some(record) = inner.records.get_mut(authority) {
                let now = SystemTime::now();
                record.status = ProxyRuntimeStatus::Ko;
                record.failure_count += 1;
                record.cooldown_until = Some(now + self.config.ko_cooldown);
                record.destination_failures.clear();
                Some(record.clone())
            } else {
                None
            }
        };
        if let Some(record) = updated {
            #[cfg(feature = "persistence")]
            self.persist_records(std::slice::from_ref(&record)).await;
            #[cfg(not(feature = "persistence"))]
            drop(record);
        }
    }

    /// Records that a proxy requires authentication.
    pub async fn record_auth_required(&self, authority: &str) {
        if self.remove_if_stale(authority).await {
            return;
        }
        let updated = {
            let mut inner = self.inner.write().await;
            if let Some(record) = inner.records.get_mut(authority) {
                record.status = ProxyRuntimeStatus::AuthenticationRequired;
                record.authentication_required = Some(true);
                record.failure_count += 1;
                Some(record.clone())
            } else {
                None
            }
        };
        if let Some(record) = updated {
            #[cfg(feature = "persistence")]
            self.persist_records(std::slice::from_ref(&record)).await;
            #[cfg(not(feature = "persistence"))]
            drop(record);
        }
    }

    /// Records a successful proxy use and renews its 24-hour validation age.
    pub async fn record_ok(&self, authority: &str) {
        let updated = {
            let mut inner = self.inner.write().await;
            if let Some(record) = inner.records.get_mut(authority) {
                record.status = ProxyRuntimeStatus::Ok;
                record.failure_count = 0;
                record.cooldown_until = None;
                record.destination_failures.clear();
                record.last_validated_at = Some(SystemTime::now());
                Some(record.clone())
            } else {
                None
            }
        };
        if let Some(record) = updated {
            #[cfg(feature = "persistence")]
            self.persist_records(std::slice::from_ref(&record)).await;
            #[cfg(not(feature = "persistence"))]
            drop(record);
        }
    }

    /// Renews the validation age after an accepted origin response and records
    /// the proxy as the preferred candidate for that exact destination without
    /// clearing unrelated proxy or destination failure state.
    pub async fn record_validated_response(&self, authority: &str, destination: &Destination) {
        let updated = {
            let mut inner = self.inner.write().await;
            let updated = inner.records.get_mut(authority).map(|record| {
                record.last_validated_at = Some(SystemTime::now());
                record.clone()
            });
            if let Some(country) = updated.as_ref().and_then(|record| record.country.as_ref()) {
                inner.preferred_by_destination.insert(
                    destination_preference_key(country, destination),
                    authority.to_string(),
                );
            }
            updated
        };
        if let Some(record) = updated {
            #[cfg(feature = "persistence")]
            self.persist_records(std::slice::from_ref(&record)).await;
            #[cfg(not(feature = "persistence"))]
            drop(record);
        }
    }

    /// Removes a proxy only when its last successful validation is older than
    /// [`PROXY_CACHE_TTL`].
    async fn remove_if_stale(&self, authority: &str) -> bool {
        let removed_key = {
            let mut inner = self.inner.write().await;
            let stale = inner
                .records
                .get(authority)
                .is_some_and(|record| proxy_validation_is_stale(record, SystemTime::now()));
            if !stale {
                return false;
            }
            let removed = inner.records.remove(authority);
            if let Some(record) = &removed {
                if let Some(country) = &record.country {
                    if let Some(keys) = inner.country_records.get_mut(country) {
                        keys.retain(|key| key != authority);
                    }
                }
            }
            removed.map(|record| record.key())
        };

        #[cfg(feature = "persistence")]
        if let Some(key) = removed_key {
            self.delete_persisted_record(&key).await;
        }
        #[cfg(not(feature = "persistence"))]
        let _ = removed_key;

        tracing::debug!(
            authority,
            "removed stale dynamic proxy after observed failure"
        );
        true
    }
}

/// Writes `records` through the typed proxy repository.
#[cfg(feature = "persistence")]
async fn persist_proxy_records(
    persistence: &Arc<dyn ProxyRepository>,
    records: &[ProxyRecord],
) -> anyhow::Result<()> {
    persistence.save_many(records).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "persistence")]
    use crate::core::proxy_repository::memory_proxy_store;
    #[cfg(feature = "persistence")]
    use crate::core::TypedProxyRepository;
    use crate::core::{ProxyAvailabilityHint, ProxyDestinationFailure};
    use anyhow::Result as AnyResult;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[cfg(feature = "persistence")]
    struct StaticProvider {
        records: Vec<ProxyRecord>,
    }

    #[cfg(feature = "persistence")]
    #[async_trait]
    impl ProxyDataProvider for StaticProvider {
        async fn load_proxies(
            &self,
            _request: crate::core::ProxyLoadRequest,
        ) -> crate::core::Result<Vec<ProxyRecord>> {
            Ok(self.records.clone())
        }
    }

    struct CountingProvider {
        records: Vec<ProxyRecord>,
        calls: AtomicUsize,
        delay: Duration,
    }

    #[async_trait]
    impl ProxyDataProvider for CountingProvider {
        async fn load_proxies(
            &self,
            _request: crate::core::ProxyLoadRequest,
        ) -> crate::core::Result<Vec<ProxyRecord>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            Ok(self.records.clone())
        }
    }

    struct FailingProvider {
        calls: AtomicUsize,
        delay: Duration,
    }

    #[async_trait]
    impl ProxyDataProvider for FailingProvider {
        async fn load_proxies(
            &self,
            _request: crate::core::ProxyLoadRequest,
        ) -> crate::core::Result<Vec<ProxyRecord>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            Err(ProxyError::RouteUnavailable(
                "test provider unavailable".to_string(),
            ))
        }
    }

    fn record(status: ProxyRuntimeStatus) -> ProxyRecord {
        ProxyRecord {
            protocol: Some(ProxyProtocol::Socks5),
            host: "merged.example".to_string(),
            port: 1080,
            country: Some("BE".to_string()),
            supports_https: Some(true),
            status,
            latency_ms: Some(25),
            failure_count: 0,
            authentication_required: Some(false),
            availability: ProxyAvailabilityHint::High,
            destination_failures: Vec::new(),
            last_checked: Some(SystemTime::now()),
            last_validated_at: Some(SystemTime::now()),
            cooldown_until: None,
        }
    }

    #[test]
    fn provider_records_reuse_fresh_cache_and_probe_only_unknown_or_expired_endpoints() {
        let config = InventoryConfig::default();
        let now = SystemTime::now();

        let mut fresh = record(ProxyRuntimeStatus::Ko);
        fresh.failure_count = 3;
        fresh.authentication_required = Some(true);
        fresh.cooldown_until = Some(now + Duration::from_secs(60));
        fresh.destination_failures = vec![ProxyDestinationFailure {
            scheme: "https".to_string(),
            host: "origin.example".to_string(),
            port: 443,
            reason: crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
            failure_count: 2,
            last_failed: now,
            cooldown_until: None,
        }];

        let mut untested = record(ProxyRuntimeStatus::Unknown);
        untested.host = "untested.example".to_string();
        untested.last_checked = None;

        let mut expired = record(ProxyRuntimeStatus::Ok);
        expired.host = "expired.example".to_string();
        expired.last_checked = Some(now - config.probe_ttl - Duration::from_secs(1));

        let mut provider_fresh = record(ProxyRuntimeStatus::Unknown);
        provider_fresh.protocol = Some(ProxyProtocol::Http);
        provider_fresh.supports_https = None;
        provider_fresh.latency_ms = None;
        provider_fresh.last_checked = None;
        provider_fresh.last_validated_at = None;

        let mut provider_untested = provider_fresh.clone();
        provider_untested.host = untested.host.clone();
        let mut provider_expired = provider_fresh.clone();
        provider_expired.host = expired.host.clone();
        let mut provider_new = provider_fresh.clone();
        provider_new.host = "new.example".to_string();

        let existing = HashMap::from([
            (fresh.authority(), fresh.clone()),
            (untested.authority(), untested),
            (expired.authority(), expired),
        ]);
        let (prepared, probe_indices, duplicate_count, cached_count) = prepare_provider_records(
            vec![
                provider_fresh.clone(),
                provider_fresh,
                provider_untested,
                provider_expired,
                provider_new,
            ],
            &existing,
            now,
            config.probe_ttl,
        );

        assert_eq!(prepared.len(), 4);
        assert_eq!(prepared[0], fresh);
        assert_eq!(probe_indices, vec![1, 2, 3]);
        assert_eq!(duplicate_count, 1);
        assert_eq!(cached_count, 1);
    }

    #[cfg(feature = "persistence")]
    #[tokio::test]
    async fn provider_reload_persists_post_merge_runtime_exclusion() -> AnyResult<()> {
        let store = memory_proxy_store()?;
        let repository = Arc::new(TypedProxyRepository::new(Arc::clone(&store)));
        let provider = Arc::new(StaticProvider {
            records: vec![record(ProxyRuntimeStatus::Ok)],
        });
        let inventory = ProxyInventory::new(InventoryConfig::default(), Some(provider), None)
            .with_proxy_repository(repository);

        let mut retained = record(ProxyRuntimeStatus::Ko);
        retained.failure_count = 3;
        retained.authentication_required = Some(true);
        retained.last_validated_at =
            Some(SystemTime::now() - PROXY_CACHE_TTL - Duration::from_secs(1));
        retained.cooldown_until = Some(SystemTime::now() + Duration::from_secs(60));
        retained.destination_failures = vec![ProxyDestinationFailure {
            scheme: "https".to_string(),
            host: "origin.example".to_string(),
            port: 443,
            reason: crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
            failure_count: 2,
            last_failed: SystemTime::now(),
            cooldown_until: None,
        }];
        let key = retained.key();
        inventory.add_or_update(vec![retained.clone()]).await;

        inventory.load_if_needed("BE").await;

        assert_eq!(store.get(&key).await?, Some(retained));
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_country_loads_wait_for_one_provider_refresh() -> AnyResult<()> {
        let provider = Arc::new(CountingProvider {
            records: vec![record(ProxyRuntimeStatus::Ok)],
            calls: AtomicUsize::new(0),
            delay: Duration::from_millis(50),
        });
        let inventory = Arc::new(ProxyInventory::new(
            InventoryConfig::default(),
            Some(provider.clone()),
            None,
        ));

        let first = {
            let inventory = Arc::clone(&inventory);
            tokio::spawn(async move { inventory.select("BE", true).await })
        };
        let second = {
            let inventory = Arc::clone(&inventory);
            tokio::spawn(async move { inventory.select("BE", true).await })
        };

        assert!(first.await??.supports_https.unwrap_or(false));
        assert!(second.await??.supports_https.unwrap_or(false));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_country_loads_share_one_provider_failure() -> AnyResult<()> {
        let provider = Arc::new(FailingProvider {
            calls: AtomicUsize::new(0),
            delay: Duration::from_millis(50),
        });
        let inventory = Arc::new(ProxyInventory::new(
            InventoryConfig::default(),
            Some(provider.clone()),
            None,
        ));

        let first = {
            let inventory = Arc::clone(&inventory);
            tokio::spawn(async move { inventory.select("BE", true).await })
        };
        let second = {
            let inventory = Arc::clone(&inventory);
            tokio::spawn(async move { inventory.select("BE", true).await })
        };

        assert!(first.await?.is_err());
        assert!(second.await?.is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert!(inventory.select("BE", true).await.is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[tokio::test]
    async fn successive_selections_share_provider_refresh_cooldown() {
        let mut http_only = record(ProxyRuntimeStatus::Ok);
        http_only.protocol = Some(ProxyProtocol::Http);
        http_only.supports_https = Some(false);
        let provider = Arc::new(CountingProvider {
            records: vec![http_only],
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
        });
        let inventory = ProxyInventory::new(
            InventoryConfig {
                provider_refresh_cooldown: Duration::from_secs(60),
                ..InventoryConfig::default()
            },
            Some(provider.clone()),
            None,
        );

        assert!(inventory.select("BE", true).await.is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert!(inventory.select("BE", true).await.is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn provider_refresh_resumes_after_cooldown() {
        let provider = Arc::new(CountingProvider {
            records: Vec::new(),
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
        });
        let inventory = ProxyInventory::new(
            InventoryConfig {
                provider_refresh_cooldown: Duration::from_millis(20),
                ..InventoryConfig::default()
            },
            Some(provider.clone()),
            None,
        );

        assert!(inventory.select("BE", true).await.is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(inventory.select("BE", true).await.is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn validated_proxy_is_preferred_only_for_its_destination() {
        let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
        let mut fastest = record(ProxyRuntimeStatus::Ok);
        fastest.host = "fastest.example".to_string();
        fastest.latency_ms = Some(5);
        let mut validated = record(ProxyRuntimeStatus::Ok);
        validated.host = "validated.example".to_string();
        validated.latency_ms = Some(100);
        inventory
            .add_or_update(vec![fastest.clone(), validated.clone()])
            .await;
        let preferred_destination = Destination::host_port("preferred.example", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        let other_destination = Destination::host_port("other.example", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);

        inventory
            .record_validated_response(&validated.authority(), &preferred_destination)
            .await;

        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&preferred_destination))
                .await
                .unwrap()
                .authority(),
            validated.authority()
        );
        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&other_destination))
                .await
                .unwrap()
                .authority(),
            fastest.authority()
        );
        assert_eq!(
            inventory.select("BE", true).await.unwrap().authority(),
            fastest.authority()
        );
    }

    #[tokio::test]
    async fn validated_proxy_preference_never_bypasses_eligibility() {
        let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
        let mut fallback = record(ProxyRuntimeStatus::Ok);
        fallback.host = "fallback.example".to_string();
        fallback.latency_ms = Some(50);
        let mut validated = record(ProxyRuntimeStatus::Ok);
        validated.host = "validated.example".to_string();
        validated.latency_ms = Some(5);
        let validated_authority = validated.authority();
        inventory
            .add_or_update(vec![fallback.clone(), validated])
            .await;
        let destination = Destination::host_port("blocked.example", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        inventory
            .record_validated_response(&validated_authority, &destination)
            .await;
        inventory
            .record_destination_failure(
                &validated_authority,
                &destination,
                crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
            )
            .await;

        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&destination))
                .await
                .unwrap()
                .authority(),
            fallback.authority()
        );

        inventory.record_global_failure(&validated_authority).await;
        assert_eq!(
            inventory.select("BE", true).await.unwrap().authority(),
            fallback.authority()
        );
    }

    #[test]
    fn default_destination_failure_cooldown_is_24_hours() {
        assert_eq!(
            InventoryConfig::default().destination_failure_cooldown,
            PROXY_DESTINATION_FAILURE_COOLDOWN
        );
        assert_eq!(
            PROXY_DESTINATION_FAILURE_COOLDOWN,
            Duration::from_secs(24 * 60 * 60)
        );
    }

    #[tokio::test]
    async fn multi_country_selection_checks_all_cached_countries_before_loading() {
        let provider = Arc::new(CountingProvider {
            records: Vec::new(),
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
        });
        let inventory =
            ProxyInventory::new(InventoryConfig::default(), Some(provider.clone()), None);
        let mut cached = record(ProxyRuntimeStatus::Ok);
        cached.host = "fr-proxy.example".to_string();
        cached.country = Some("FR".to_string());
        inventory.add_or_update(vec![cached.clone()]).await;

        let countries = vec!["AD".to_string(), "FR".to_string()];
        let selected = inventory.select_any(&countries, true).await.unwrap();

        assert_eq!(selected.authority(), cached.authority());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn multi_country_selection_loads_each_country_at_most_once_per_call() {
        let provider = Arc::new(CountingProvider {
            records: Vec::new(),
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
        });
        let inventory =
            ProxyInventory::new(InventoryConfig::default(), Some(provider.clone()), None);
        let countries = vec!["AD".to_string(), "FR".to_string(), "WF".to_string()];

        assert!(inventory.select_any(&countries, true).await.is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), countries.len());
    }

    #[tokio::test]
    async fn blocked_by_origin_remains_scoped_to_its_destination() {
        let mut provider_record = record(ProxyRuntimeStatus::Ok);
        provider_record.host = "replacement.example".to_string();
        let provider = Arc::new(CountingProvider {
            records: vec![provider_record.clone()],
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
        });
        let inventory = ProxyInventory::new(
            InventoryConfig {
                max_destination_failures_before_ko: 1,
                ..InventoryConfig::default()
            },
            Some(provider.clone()),
            None,
        );
        let cached = record(ProxyRuntimeStatus::Ok);
        let authority = cached.authority();
        inventory.add_or_update(vec![cached]).await;
        let blocked_destination = Destination::host_port("blocked.example", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        let other_destination = Destination::host_port("allowed.example", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);

        let marked_globally_ko = inventory
            .record_destination_failure(
                &authority,
                &blocked_destination,
                crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
            )
            .await;

        assert!(!marked_globally_ko);
        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&blocked_destination))
                .await
                .unwrap()
                .authority(),
            provider_record.authority()
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&other_destination))
                .await
                .unwrap()
                .authority(),
            authority
        );
    }

    #[cfg(feature = "persistence")]
    #[tokio::test]
    async fn blocked_by_origin_is_restored_from_persistence() -> AnyResult<()> {
        let store = memory_proxy_store()?;
        let repository = Arc::new(TypedProxyRepository::new(Arc::clone(&store)));
        let initial = ProxyInventory::new(InventoryConfig::default(), None, None)
            .with_proxy_repository(repository.clone());
        let mut blocked = record(ProxyRuntimeStatus::Ok);
        blocked.host = "blocked-proxy.example".to_string();
        blocked.latency_ms = Some(5);
        let mut fallback = record(ProxyRuntimeStatus::Ok);
        fallback.host = "fallback-proxy.example".to_string();
        fallback.latency_ms = Some(50);
        let blocked_authority = blocked.authority();
        initial
            .add_or_update(vec![blocked.clone(), fallback.clone()])
            .await;
        initial.persist_records(&[blocked, fallback.clone()]).await;
        let destination = Destination::host_port("origin.example", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);

        initial
            .record_destination_failure(
                &blocked_authority,
                &destination,
                crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
            )
            .await;

        let restored = ProxyInventory::new(InventoryConfig::default(), None, None)
            .with_proxy_repository(repository);
        assert_eq!(
            restored
                .select_for_destination("BE", true, Some(&destination))
                .await?
                .authority(),
            fallback.authority()
        );
        Ok(())
    }

    #[tokio::test]
    async fn multi_country_failure_reports_every_attempted_country() {
        let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
        let countries = vec!["AD".to_string(), "FR".to_string(), "WF".to_string()];

        let error = inventory
            .select_any(&countries, true)
            .await
            .expect_err("selection should fail without records or provider")
            .to_string();

        assert!(error.contains("AD, FR, WF"));
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

        if is_eligible(record, country, require_https, destination, now) {
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

fn has_newer_runtime_update(existing: &ProxyRecord, incoming: &ProxyRecord) -> bool {
    has_fresh_runtime_update(incoming)
        && incoming
            .last_checked
            .is_some_and(|incoming_checked| existing.last_checked < Some(incoming_checked))
}

fn probe_is_expired(record: &ProxyRecord, now: SystemTime, probe_ttl: Duration) -> bool {
    let Some(last_checked) = record.last_checked else {
        return true;
    };

    now.duration_since(last_checked)
        .map(|age| age > probe_ttl)
        .unwrap_or(false)
}

/// Deduplicates provider records and identifies the entries that require a
/// network probe.
///
/// A cached endpoint whose last probe is still within `probe_ttl` retains all
/// runtime-derived fields. Provider-owned metadata such as country and
/// availability still comes from the refreshed source record.
fn prepare_provider_records(
    records: Vec<ProxyRecord>,
    existing: &HashMap<String, ProxyRecord>,
    now: SystemTime,
    probe_ttl: Duration,
) -> (Vec<ProxyRecord>, Vec<usize>, usize, usize) {
    let mut seen = HashSet::new();
    let mut prepared = Vec::with_capacity(records.len());
    let mut probe_indices = Vec::new();
    let mut duplicate_count = 0;
    let mut cached_count = 0;

    for mut record in records {
        let authority = record.authority();
        if !seen.insert(authority.clone()) {
            duplicate_count += 1;
            continue;
        }

        if let Some(cached) = existing.get(&authority) {
            if !probe_is_expired(cached, now, probe_ttl) {
                retain_cached_runtime(&mut record, cached);
                cached_count += 1;
            } else {
                probe_indices.push(prepared.len());
            }
        } else {
            probe_indices.push(prepared.len());
        }
        prepared.push(record);
    }

    (prepared, probe_indices, duplicate_count, cached_count)
}

fn retain_cached_runtime(record: &mut ProxyRecord, cached: &ProxyRecord) {
    record.protocol = cached.protocol.clone();
    record.supports_https = cached.supports_https;
    record.status = cached.status.clone();
    record.latency_ms = cached.latency_ms;
    record.failure_count = cached.failure_count;
    record.authentication_required = cached.authentication_required;
    record.destination_failures = cached.destination_failures.clone();
    record.last_checked = cached.last_checked;
    record.last_validated_at = cached.last_validated_at;
    record.cooldown_until = cached.cooldown_until;
}

fn proxy_validation_is_stale(record: &ProxyRecord, now: SystemTime) -> bool {
    let Some(last_validated_at) = record.last_validated_at.or(record.last_checked) else {
        return false;
    };

    now.duration_since(last_validated_at)
        .map(|age| age > PROXY_CACHE_TTL)
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

fn destination_preference_key(country: &str, destination: &Destination) -> String {
    format!(
        "{}|{}|{}|{}",
        country,
        destination_scheme(destination),
        destination.host_for_protocol(),
        destination.port
    )
}
