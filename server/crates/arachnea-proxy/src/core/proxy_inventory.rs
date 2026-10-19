use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crate::core::ProxySearchContext;
use futures::future::{BoxFuture, WeakShared};
use futures::{stream, FutureExt, StreamExt};
use tokio::sync::{watch, Mutex, RwLock, Semaphore};

#[cfg(feature = "persistence")]
use crate::core::ProxyRepository;
use crate::core::{
    Destination, IpCountryResolver, ProbeMode, ProxyCapabilityStatus, ProxyDataProvider,
    ProxyDeclaration, ProxyError, ProxyProbe, ProxyProtocol, ProxyRecord, ProxyRuntimeStatus,
    Result, PROXY_CACHE_TTL, PROXY_PROTOCOL_PRIORITY,
};

type SharedProbe = WeakShared<BoxFuture<'static, Option<ProxyRecord>>>;

struct SharedExecution {
    key: String,
    completed: bool,
}

impl Drop for SharedExecution {
    fn drop(&mut self) {
        if !self.completed {
            tracing::debug!(work = %self.key, "cancelled shared proxy work after last subscriber departed");
        }
    }
}

/// Releases unfinished request-local work when a candidate pipeline is cancelled.
struct SearchReservation {
    search: ProxySearchContext,
    key: String,
    validation: bool,
    completed: bool,
}

struct DiscoveryReservation {
    search: ProxySearchContext,
    selected: bool,
}

impl Drop for DiscoveryReservation {
    fn drop(&mut self) {
        if !self.selected {
            self.search.take_discovery();
        }
    }
}

impl Drop for SearchReservation {
    fn drop(&mut self) {
        if !self.completed {
            self.search
                .release_incomplete_work(self.validation, &self.key);
        }
    }
}

struct InventoryBatchSink {
    inventory: ProxyInventory,
    country: String,
}

#[async_trait::async_trait]
impl crate::core::proxy_record::ProxyBatchSink for InventoryBatchSink {
    async fn publish(&self, records: Vec<ProxyRecord>) -> Result<()> {
        self.inventory
            .merge_provider_batch(&self.country, records)
            .await;
        Ok(())
    }
}

const PROXY_AFFINITY_TTL: Duration = Duration::from_secs(15 * 60);
/// Default duration for excluding a proxy from one exact destination after a
/// destination-scoped failure such as an HTTP 403 origin rejection.
pub const PROXY_DESTINATION_FAILURE_COOLDOWN: Duration = Duration::from_secs(24 * 60 * 60);
/// Maximum suspension after an ambiguous transport failure for one destination.
pub const PROXY_TRANSIENT_FAILURE_COOLDOWN: Duration = Duration::from_secs(30);

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
    /// Origin rejection cooldown; ambiguous failures are capped at 30 seconds.
    pub destination_failure_cooldown: Duration,
    /// Maximum number of active non-origin-block destination failures before
    /// the proxy was formerly marked globally KO. Retained for configuration
    /// compatibility; destination observations no longer promote global failure.
    pub max_destination_failures_before_ko: usize,
    /// Maximum concurrent candidate pipelines in one selection pass, clamped to 1..=8.
    /// TCP and validation stages are driven together; full validations retain
    /// their separate concurrency limit of two per search.
    pub probe_batch_size: usize,
    /// Minimum delay between provider refresh attempts for the same country.
    pub provider_refresh_cooldown: Duration,
    /// Lower bound retained when an observed endpoint failure requests early refresh.
    pub provider_min_refresh_interval: Duration,
    /// Cache exploration time before discovery overlaps the remaining candidates.
    pub exploration_before_refresh: Duration,
    /// Completed unsuccessful validations before discovery overlaps the cache.
    pub failures_before_refresh: usize,
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
            provider_min_refresh_interval: Duration::from_secs(10),
            exploration_before_refresh: Duration::from_secs(10),
            failures_before_refresh: 8,
        }
    }
}

/// Internal mutable state of the proxy inventory.
struct InventoryInner {
    records: HashMap<String, ProxyRecord>,
    country_records: HashMap<String, Vec<String>>,
    country_load_locks: HashMap<String, Arc<Mutex<()>>>,
    country_last_refresh_attempts: HashMap<String, Instant>,
    country_early_refresh: std::collections::HashSet<String>,
    /// Bounded accepted histories per exact destination, independent of countries.
    preferred_by_destination: HashMap<String, crate::core::ProxyDestinationPreference>,
    evicted: HashMap<String, ProxyRecord>,
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
/// prevent concurrent duplicate lookups. Clones share records, operation registries,
/// concurrency permits, commit locks and persistence metrics.
#[derive(Clone)]
pub struct ProxyInventory {
    inner: Arc<RwLock<InventoryInner>>,
    provider: Option<Arc<dyn ProxyDataProvider>>,
    probe: Option<Arc<ProxyProbe>>,
    selection_probe_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    shared_work: Arc<Mutex<HashMap<String, SharedProbe>>>,
    tcp_slots: Arc<Semaphore>,
    validation_slots: Arc<Semaphore>,
    provider_slots: Arc<Semaphore>,
    discovery_revision: watch::Sender<u64>,
    /// Persistent cache for dynamic proxy records, when configured.
    #[cfg(feature = "persistence")]
    persistence: Option<Arc<dyn ProxyRepository>>,
    #[cfg(feature = "persistence")]
    preferences: Option<Arc<dyn crate::core::ProxyPreferenceRepository>>,
    destination_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    destination_searches: Arc<Mutex<HashMap<String, std::sync::Weak<Mutex<()>>>>>,
    persistence_commits: Arc<Mutex<()>>,
    preference_persistence_failures: Arc<AtomicU64>,
    config: InventoryConfig,
    ip_country_resolver: Option<Arc<IpCountryResolver>>,
}

impl ProxyInventory {
    fn probe_mode(&self) -> ProbeMode {
        self.probe
            .as_ref()
            .map_or(ProbeMode::Relaxed, |probe| probe.config().mode)
    }

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
            inner: Arc::new(RwLock::new(InventoryInner {
                records: HashMap::new(),
                country_records: HashMap::new(),
                country_load_locks: HashMap::new(),
                country_last_refresh_attempts: HashMap::new(),
                country_early_refresh: Default::default(),
                preferred_by_destination: HashMap::new(),
                evicted: HashMap::new(),
                affinity_bindings: HashMap::new(),
            })),
            provider,
            probe,
            selection_probe_locks: Arc::new(Mutex::new(HashMap::new())),
            shared_work: Arc::new(Mutex::new(HashMap::new())),
            tcp_slots: Arc::new(Semaphore::new(16)),
            validation_slots: Arc::new(Semaphore::new(4)),
            provider_slots: Arc::new(Semaphore::new(2)),
            discovery_revision: watch::channel(0).0,
            destination_searches: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(feature = "persistence")]
            persistence: None,
            #[cfg(feature = "persistence")]
            preferences: None,
            destination_locks: Arc::new(Mutex::new(HashMap::new())),
            persistence_commits: Arc::new(Mutex::new(())),
            preference_persistence_failures: Arc::new(AtomicU64::new(0)),
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

    /// Attaches the application-owned durable accepted-history repository.
    ///
    /// # Arguments
    /// * `repository` - Shared repository whose commits are awaited by successful calls.
    #[cfg(feature = "persistence")]
    pub fn with_preference_repository(
        mut self,
        repository: Arc<dyn crate::core::ProxyPreferenceRepository>,
    ) -> Self {
        self.preferences = Some(repository);
        self
    }

    async fn destination_lock(&self, key: &str) -> Arc<Mutex<()>> {
        let mut locks = self.destination_locks.lock().await;
        locks.retain(|_, lock| Arc::strong_count(lock) > 1);
        locks
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
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

    /// Shares an Arachnea DNS core with the configured proxy probe.
    #[cfg(feature = "arachnea-dns")]
    pub(crate) fn with_arachnea_dns(
        mut self,
        dns_core: Arc<arachnea_dns::core::ArachneaDnsCore>,
    ) -> Self {
        self.probe = self.probe.map(|probe| {
            Arc::new(
                probe
                    .as_ref()
                    .clone()
                    .with_arachnea_dns(Arc::clone(&dns_core)),
            )
        });
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

    /// Returns failed accepted-history commits since this inventory was constructed.
    /// Successful business responses are preserved despite these storage failures.
    pub fn preference_persistence_failures(&self) -> u64 {
        self.preference_persistence_failures.load(Ordering::Relaxed)
    }

    pub(crate) fn notify_destination_success(&self) {
        self.discovery_revision
            .send_modify(|revision| *revision = revision.wrapping_add(1));
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
        let _guard = self.persistence_commits.lock().await;
        let current = {
            let inner = self.inner.read().await;
            records
                .iter()
                .filter_map(|record| inner.records.get(&record.authority()).cloned())
                .collect::<Vec<_>>()
        };
        if let Err(error) = persist_proxy_records(persistence, &current).await {
            tracing::warn!(%error, count = records.len(), "failed to persist proxy records");
        }
    }

    /// Persists acceptance without letting the repository's conservative failure
    /// merge restore the superseded exclusion. Both writes use the latest state.
    #[cfg(feature = "persistence")]
    async fn persist_accepted_record(
        &self,
        record: &ProxyRecord,
        destination: &Destination,
        accepted_at: SystemTime,
    ) {
        let Some(persistence) = &self.persistence else {
            return;
        };
        let _guard = self.persistence_commits.lock().await;
        let current = self
            .inner
            .read()
            .await
            .records
            .get(&record.authority())
            .cloned();
        let Some(current) = current else {
            return;
        };
        if let Err(error) = persistence.save_many(std::slice::from_ref(&current)).await {
            tracing::warn!(%error, "failed to persist accepted proxy record");
            return;
        }
        if let Err(error) = persistence
            .clear_superseded_destination_failures(&current.key(), destination, accepted_at)
            .await
        {
            tracing::warn!(%error, "failed to persist superseded proxy exclusion repair");
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
        #[cfg(feature = "persistence")]
        let mut records = records;
        #[cfg(feature = "persistence")]
        if let Some(repository) = &self.persistence {
            for record in &mut records {
                if self
                    .inner
                    .read()
                    .await
                    .records
                    .contains_key(&record.authority())
                {
                    continue;
                }
                match repository.get(&record.key()).await {
                    Ok(Some(current)) => {
                        if current.health_evidence.last_positive_at.is_some()
                            || current.cooldown_until.is_some()
                        {
                            let country = record.country.clone();
                            *record = current;
                            if country.is_some() {
                                record.country = country;
                            }
                            continue;
                        }
                    }
                    Err(error) => tracing::warn!(%error, "failed to restore current proxy health"),
                    _ => {}
                }
                match repository.get_failure(&record.key()).await {
                    Ok(Some(failure)) => {
                        let country = record.country.clone();
                        *record = failure;
                        if country.is_some() {
                            record.country = country;
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to restore proxy rediscovery protection")
                    }
                    _ => {}
                }
            }
        }

        let mut inner = self.inner.write().await;
        let mut retained = Vec::with_capacity(records.len());
        let now = SystemTime::now();
        inner.evicted.retain(|_, record| {
            record.health_evidence.evicted_at.is_some_and(|date| {
                now.duration_since(date).unwrap_or_default() < Duration::from_secs(48 * 3600)
            })
        });
        for mut record in records {
            let key = record.authority();
            if !inner.records.contains_key(&key) {
                if let Some(failure) = inner.evicted.get(&key) {
                    let country = record.country.clone();
                    record = failure.clone();
                    if country.is_some() {
                        record.country = country;
                    }
                }
            }
            if record.health_evidence.evicted_at.is_some() {
                if record.cooldown_until.is_some_and(|date| date > now) {
                    continue;
                }
                record.health_evidence.evicted_at = None;
            }
            let old_country = inner.records.get(&key).and_then(|r| r.country.clone());
            let retained_record = if let Some(existing) = inner.records.get(&key) {
                let mut merged = record.clone();
                merged.health_evidence.first_discovered_at = existing
                    .health_evidence
                    .first_discovered_at
                    .or(record.health_evidence.first_discovered_at)
                    .or(Some(now));
                merged.health_evidence.last_positive_at = existing
                    .health_evidence
                    .last_positive_at
                    .max(record.health_evidence.last_positive_at);
                for observation in &existing.observations {
                    merged.record_observation(observation.clone());
                }
                for (phase, checked) in &existing.probe_attempts {
                    let incoming = merged.probe_attempts.entry(phase.clone()).or_default();
                    *incoming = (*incoming).max(*checked);
                }
                if record.probe_attempts.get("tcp") < existing.probe_attempts.get("tcp") {
                    merged.tcp_reachable = existing.tcp_reachable;
                }
                if (should_preserve_runtime_exclusion(existing)
                    && !(record.tcp_reachable == Some(true)
                        && record.health_evidence.last_positive_at
                            > existing.health_evidence.last_positive_at
                        && existing.authentication_required != Some(true)))
                    || !has_fresh_runtime_update(&record)
                {
                    merged.status = existing.status.clone();
                    merged.http_forwarding = existing.http_forwarding;
                    merged.https_tunnel = existing.https_tunnel;
                    merged.destination_tls = existing.destination_tls;
                    merged.proxy_tls_certificate = existing.proxy_tls_certificate;
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
                let mut record = record;
                record
                    .health_evidence
                    .first_discovered_at
                    .get_or_insert_with(SystemTime::now);
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
        let _commit = self.persistence_commits.lock().await;
        let removed_key = {
            let mut inner = self.inner.write().await;
            inner.records.remove(authority).map(|record| {
                if let Some(ref country) = record.country {
                    if let Some(keys) = inner.country_records.get_mut(country) {
                        keys.retain(|k| k != authority);
                    }
                }
                for preference in inner.preferred_by_destination.values_mut() {
                    preference
                        .endpoints
                        .retain(|item| item.endpoint != record.key());
                }
                inner
                    .affinity_bindings
                    .retain(|_, binding| binding.authority != authority);
                record.key()
            })
        };
        #[cfg(feature = "persistence")]
        if let Some(key) = removed_key {
            self.delete_persisted_record(&key).await;
            if let Some(repository) = &self.preferences {
                if let Err(error) = repository.remove_endpoint(&key).await {
                    tracing::warn!(%error, endpoint = %authority, "failed to clean removed proxy preferences");
                }
            }
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

    /// Selects the best cached candidate across the allowed countries,
    /// independently of their input order, before consulting providers.
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
        self.select_any_for_destination(&[country.to_string()], require_https, destination)
            .await
    }

    /// Selects the best eligible candidate across an unordered set of countries.
    ///
    /// Accepted history is consulted first. Persistent country caches are imported
    /// before redirect evidence, coordinated affinity reuse and candidate exploration.
    /// Cold business discovery is exclusive per origin/country set until its outcome;
    /// accepted endpoints remain available for parallel requests.
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
        let search = ProxySearchContext::current().unwrap_or_default();
        // A new pool selection supersedes any selection not yet claimed by a token.
        search.take_discovery();
        let mut reservation = DiscoveryReservation {
            search: search.clone(),
            selected: false,
        };
        let result = search
            .scope(search.technical(self.select_any_with_search(
                countries,
                require_https,
                destination,
                affinity,
            )))
            .await
            .and_then(|result| result);
        reservation.selected = result.is_ok();
        if let Err(error) = &result {
            let failure_kind = match error {
                ProxyError::Timeout(_) => "timeout",
                ProxyError::RouteUnavailable(_) => "route_unavailable",
                _ => "selection",
            };
            tracing::warn!(
                search_id = search.id(),
                failure_kind,
                phase = "selection",
                remaining_ms = search.remaining().as_millis(),
                "localized proxy selection failed"
            );
        }
        result
    }

    async fn select_any_with_search(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
        affinity: Option<&str>,
    ) -> Result<ProxyRecord> {
        let attempted = crate::core::canonical_proxy_countries(countries);
        if attempted.is_empty() {
            return Err(ProxyError::RouteUnavailable(
                "no proxy countries were requested".to_string(),
            ));
        }

        let affinity = affinity.map(str::trim).filter(|value| !value.is_empty());
        if let Some(record) = self
            .select_preferred_destination(&attempted, require_https, destination)
            .await
        {
            self.bind_affinity(affinity, &record).await;
            return Ok(record);
        }
        #[cfg(feature = "persistence")]
        for country in &attempted {
            self.load_cached_country(country).await;
        }
        let search = ProxySearchContext::current().expect("selection is search scoped");
        if let Some(record) = self
            .select_redirect_destination(&attempted, require_https, destination)
            .await
        {
            self.bind_affinity(affinity, &record).await;
            return Ok(record);
        }
        if let Some(destination) = destination {
            let key = format!(
                "{}|{}",
                crate::core::ProxyDestinationPreference::destination_key(destination),
                attempted.join(",")
            );
            let gate = {
                let mut gates = self.destination_searches.lock().await;
                gates.retain(|_, gate| gate.strong_count() > 0);
                if let Some(gate) = gates.get(&key).and_then(std::sync::Weak::upgrade) {
                    gate
                } else {
                    let gate = Arc::new(Mutex::new(()));
                    gates.insert(key, Arc::downgrade(&gate));
                    gate
                }
            };
            let mut changes = self.discovery_revision.subscribe();
            let acquire = gate.lock_owned();
            tokio::pin!(acquire);
            tracing::info!(search_id = search.id(), %destination, countries = ?attempted, "coordinating destination proxy discovery");
            loop {
                if let Some(record) = self
                    .select_preferred_destination(&attempted, require_https, Some(destination))
                    .await
                {
                    self.bind_affinity(affinity, &record).await;
                    return Ok(record);
                }
                if let Some(record) = self
                    .select_redirect_destination(&attempted, require_https, Some(destination))
                    .await
                {
                    self.bind_affinity(affinity, &record).await;
                    return Ok(record);
                }
                tokio::select! {
                    guard = &mut acquire => {
                        search.hold_discovery(guard);
                        break;
                    }
                    _ = changes.changed() => {}
                }
            }
            // The previous leader may have published a success immediately before release.
            if let Some(record) = self
                .select_preferred_destination(&attempted, require_https, Some(destination))
                .await
            {
                search.take_discovery();
                self.bind_affinity(affinity, &record).await;
                return Ok(record);
            }
            if let Some(record) = self
                .select_redirect_destination(&attempted, require_https, Some(destination))
                .await
            {
                search.take_discovery();
                self.bind_affinity(affinity, &record).await;
                return Ok(record);
            }
        }
        if let Some(record) = self
            .select_affinity_binding(&attempted, require_https, destination, affinity)
            .await
        {
            return Ok(record);
        }
        let mut failures = search.validation_failures();
        let initial_failures = *failures.borrow();
        let mut revisions = self.discovery_revision.subscribe();
        let refresh_after = tokio::time::sleep(self.config.exploration_before_refresh);
        futures::pin_mut!(refresh_after);
        let mut selectors =
            futures::stream::FuturesUnordered::<BoxFuture<'_, Option<ProxyRecord>>>::new();
        selectors.push(
            self.select_cached_any(&attempted, require_https, destination)
                .boxed(),
        );
        let countries = self
            .discovery_order(&attempted, require_https, destination)
            .await;
        let loads = async {
            let loads = stream::iter(countries.into_iter().map(|country| async move {
                self.load_if_needed(&country).await;
            }))
            .buffer_unordered(2);
            loads.collect::<Vec<_>>().await;
        }
        .boxed();
        futures::pin_mut!(loads);
        let mut refreshing = false;
        let mut loaded = false;
        let mut dirty = false;
        loop {
            if !refreshing
                && (selectors.is_empty()
                    || (*failures.borrow()).saturating_sub(initial_failures)
                        >= self.config.failures_before_refresh.max(1))
            {
                refreshing = true;
            }
            if dirty && selectors.len() < 2 {
                dirty = false;
                selectors.push(
                    self.select_cached_any(&attempted, require_https, destination)
                        .boxed(),
                );
            }
            if loaded && selectors.is_empty() {
                break;
            }
            tokio::select! {
                result = selectors.next(), if !selectors.is_empty() => {
                    if let Some(Some(record)) = result {
                        if let Some(preferred) = self.select_preferred_destination(&attempted, require_https, destination).await {
                            search.take_discovery();
                            self.bind_affinity(affinity, &preferred).await;
                            return Ok(preferred);
                        }
                        self.bind_affinity(affinity, &record).await;
                        return Ok(record);
                    }
                }
                _ = &mut refresh_after, if !refreshing => { refreshing = true; }
                _ = failures.changed(), if !refreshing => {}
                _ = revisions.changed() => {
                    if let Some(record) = self.select_preferred_destination(&attempted, require_https, destination).await {
                        search.take_discovery();
                        self.bind_affinity(affinity, &record).await;
                        return Ok(record);
                    }
                    if let Some(record) = self.select_redirect_destination(&attempted, require_https, destination).await {
                        search.take_discovery();
                        self.bind_affinity(affinity, &record).await;
                        return Ok(record);
                    }
                    dirty = true;
                }
                _ = &mut loads, if refreshing && !loaded => { loaded = true; dirty = true; }
            }
        }

        if self.probe_mode() == ProbeMode::Strict
            && self.inner.read().await.records.values().any(|record| {
                record
                    .country
                    .as_ref()
                    .is_some_and(|country| attempted.contains(country))
                    && record.tcp_reachable == Some(true)
                    && needs_capability_probe(record, require_https)
                    && is_selection_candidate(
                        record,
                        record.country.as_deref().unwrap_or_default(),
                        require_https,
                        destination,
                        SystemTime::now(),
                        ProbeMode::Strict,
                    )
            })
        {
            return Err(ProxyError::RouteUnavailable(format!(
                "strict proxy validation remained inconclusive for countries [{}]",
                attempted.join(", ")
            )));
        }
        Err(ProxyError::RouteUnavailable(format!(
            "no working proxy available for requested countries [{}]",
            attempted.join(", ")
        )))
    }

    async fn discovery_order(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Vec<String> {
        let inner = self.inner.read().await;
        let now = SystemTime::now();
        let mut ranked: Vec<_> = countries
            .iter()
            .map(|country| {
                let usable = inner
                    .country_records
                    .get(country)
                    .into_iter()
                    .flatten()
                    .filter_map(|key| inner.records.get(key))
                    .filter(|record| {
                        is_selection_candidate(
                            record,
                            country,
                            require_https,
                            destination,
                            now,
                            self.probe_mode(),
                        )
                    })
                    .count();
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                use std::hash::{Hash, Hasher};
                country.hash(&mut hash);
                (
                    country.clone(),
                    usable,
                    inner.country_last_refresh_attempts.get(country).copied(),
                    hash.finish(),
                )
            })
            .collect();
        ranked.sort_by_key(|(_, usable, refreshed, tie)| (*usable, *refreshed, *tie));
        ranked
            .into_iter()
            .map(|(country, _, _, _)| country)
            .collect()
    }

    /// Reuses the proxy accepted by this exact origin before any new endpoint
    /// prechecks. Probe age alone does not invalidate a successful origin binding.
    async fn select_preferred_destination(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Option<ProxyRecord> {
        let destination = destination?;
        let key = crate::core::ProxyDestinationPreference::destination_key(destination);
        let lock = self.destination_lock(&key).await;
        let _guard = lock.lock().await;
        #[cfg(feature = "persistence")]
        if !self
            .inner
            .read()
            .await
            .preferred_by_destination
            .contains_key(&key)
        {
            if let Some(repository) = &self.preferences {
                match repository.get(&key).await {
                    Ok(Some(mut preference)) => {
                        let mut present = Vec::new();
                        if let Some(inventory) = &self.persistence {
                            for item in &preference.endpoints {
                                if self
                                    .inner
                                    .read()
                                    .await
                                    .records
                                    .values()
                                    .any(|record| record.key() == item.endpoint)
                                {
                                    present.push(item.clone());
                                    continue;
                                }
                                match inventory.get(&item.endpoint).await {
                                    Ok(Some(record)) => {
                                        self.add_or_update(vec![record]).await;
                                        present.push(item.clone());
                                    }
                                    Ok(None) => {
                                        if let Err(error) =
                                            repository.remove_endpoint(&item.endpoint).await
                                        {
                                            tracing::warn!(%error, destination = %key, "failed to clean orphan proxy preference");
                                        }
                                    }
                                    Err(error) => {
                                        tracing::warn!(%error, destination = %key, "failed to restore preferred proxy endpoint");
                                        present.push(item.clone());
                                    }
                                }
                            }
                            preference.endpoints = present;
                        }
                        self.inner
                            .write()
                            .await
                            .preferred_by_destination
                            .insert(key.clone(), preference);
                    }
                    Err(error) => {
                        tracing::warn!(%error, destination = %key, "failed to restore proxy preference")
                    }
                    _ => {}
                }
            }
        }
        let repaired = {
            let mut inner = self.inner.write().await;
            let endpoints = inner.preferred_by_destination.get(&key)?.endpoints.clone();
            let mut repaired = Vec::new();
            for item in endpoints {
                if let Some(record) = inner
                    .records
                    .values_mut()
                    .find(|record| record.key() == item.endpoint)
                {
                    if clear_superseded_destination_failures(record, destination, item.accepted_at)
                    {
                        repaired.push((record.clone(), item.accepted_at));
                    }
                }
            }
            repaired
        };
        #[cfg(feature = "persistence")]
        for (record, accepted_at) in &repaired {
            self.persist_accepted_record(record, destination, *accepted_at)
                .await;
        }
        #[cfg(not(feature = "persistence"))]
        drop(repaired);
        let inner = self.inner.read().await;
        let preference = inner.preferred_by_destination.get(&key)?;
        for item in &preference.endpoints {
            let Some(record) = inner
                .records
                .values()
                .find(|record| record.key() == item.endpoint)
            else {
                continue;
            };
            let Some(country) = record.country.as_ref().filter(|country| {
                countries
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(country))
            }) else {
                continue;
            };
            if !ProxySearchContext::current()
                .is_some_and(|search| search.attempted(&record.authority(), Some(destination)))
                && is_eligible(
                    record,
                    country,
                    require_https,
                    Some(destination),
                    SystemTime::now(),
                    self.probe_mode(),
                )
            {
                if let Some(search) = ProxySearchContext::current() {
                    search.mark_selection(
                        &record.authority(),
                        destination,
                        crate::core::ProxySelectionProvenance::AcceptedDestination,
                    );
                }
                tracing::info!(
                    search_id = ProxySearchContext::current().map(|search| search.id()),
                    provenance = "accepted_destination",
                    destination = %destination,
                    proxy = %record.authority(),
                    country,
                    "reusing proxy validated for destination without discovery"
                );
                return Some(record.clone());
            }
            tracing::info!(search_id = ProxySearchContext::current().map(|search| search.id()),
                %destination, proxy = %record.authority(),
                destination_cooldown = has_active_destination_cooldown(record, destination, SystemTime::now()),
                status = ?record.status,
                already_attempted = ProxySearchContext::current().is_some_and(|search| search.attempted(&record.authority(), Some(destination))),
                "preferred proxy currently ineligible");
        }
        None
    }

    async fn select_redirect_destination(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Option<ProxyRecord> {
        let destination = destination?;
        let target = destination.to_string();
        let now = SystemTime::now();
        let search = ProxySearchContext::current();
        let inner = self.inner.read().await;
        let record = inner
            .records
            .values()
            .filter(|record| {
                record.country.as_ref().is_some_and(|country| {
                    countries.contains(country)
                        && is_eligible(
                            record,
                            country,
                            require_https,
                            Some(destination),
                            now,
                            self.probe_mode(),
                        )
                }) && !search
                    .as_ref()
                    .is_some_and(|search| search.attempted(&record.authority(), Some(destination)))
            })
            .filter_map(|record| {
                record
                    .observations
                    .iter()
                    .filter(|observation| {
                        observation.context.configuration == "business-redirect-v1"
                            && observation.context.target.as_deref() == Some(target.as_str())
                            && observation.outcome
                                == crate::core::ProxyObservationOutcome::Succeeded
                    })
                    .map(|observation| (observation.completed_at, record))
                    .max_by_key(|(date, _)| *date)
            })
            .max_by(|(a_date, a), (b_date, b)| {
                a_date
                    .cmp(b_date)
                    .then_with(|| b.authority().cmp(&a.authority()))
            })?
            .1;
        if let Some(search) = &search {
            search.mark_selection(
                &record.authority(),
                destination,
                crate::core::ProxySelectionProvenance::RedirectDestination,
            );
        }
        tracing::info!(search_id = search.map(|search| search.id()), %destination, proxy = %record.authority(), provenance = "redirect_destination", "reusing proxy with followed redirect evidence");
        Some(record.clone())
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
                && !ProxySearchContext::current()
                    .is_some_and(|search| search.attempted(&record.authority(), destination))
                && is_selection_candidate(
                    record,
                    record.country.as_deref().unwrap_or_default(),
                    require_https,
                    destination,
                    system_now,
                    self.probe_mode(),
                )
        });
        if let Some(record) = record {
            let record = record.clone();
            drop(inner);
            if !needs_capability_probe(&record, require_https)
                && !probe_is_expired(&record, system_now, self.config.probe_ttl)
            {
                return is_eligible(
                    &record,
                    record.country.as_deref()?,
                    require_https,
                    destination,
                    system_now,
                    self.probe_mode(),
                )
                .then_some(record);
            }
            return None;
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

    /// Ranks cached candidates across all allowed countries before any probes.
    async fn select_cached_any(
        &self,
        countries: &[String],
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Option<ProxyRecord> {
        if let Some(probe) = &self.probe {
            let mut inner = self.inner.write().await;
            for record in inner.records.values_mut().filter(|record| {
                record
                    .country
                    .as_ref()
                    .is_some_and(|country| countries.contains(country))
            }) {
                probe.prepare_context(record);
            }
        }
        let inner = self.inner.read().await;
        let now = SystemTime::now();

        let records: Vec<&ProxyRecord> = countries
            .iter()
            .filter_map(|country| inner.country_records.get(country))
            .flatten()
            .filter_map(|key| inner.records.get(key))
            .collect();
        for country in countries {
            let stats = selection_stats(
                records
                    .iter()
                    .copied()
                    .filter(|record| record.country.as_deref() == Some(country.as_str())),
                country,
                require_https,
                destination,
                now,
                self.config.probe_ttl,
                self.probe_mode(),
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
                http_forward_available = stats.http_forward_available,
                http_forward_unavailable = stats.http_forward_unavailable,
                http_forward_unknown = stats.http_forward_unknown,
                https_tunnel_available = stats.https_tunnel_available,
                https_tunnel_unavailable = stats.https_tunnel_unavailable,
                https_tunnel_unknown = stats.https_tunnel_unknown,
                destination_tls_available = stats.destination_tls_available,
                destination_tls_unavailable = stats.destination_tls_unavailable,
                destination_tls_unknown = stats.destination_tls_unknown,
                eligible = stats.eligible,
                "proxy inventory selection candidates"
            );
        }

        let mut candidates: Vec<&ProxyRecord> = records
            .into_iter()
            .filter(|r| {
                !ProxySearchContext::current()
                    .is_some_and(|search| search.attempted(&r.authority(), destination))
                    && is_selection_candidate(
                        r,
                        r.country.as_deref().unwrap_or_default(),
                        require_https,
                        destination,
                        now,
                        self.probe_mode(),
                    )
            })
            .collect();

        if candidates.is_empty() {
            return None;
        }

        candidates.sort_by(|a, b| {
            let rank = |record: &ProxyRecord| {
                if !needs_capability_probe(record, require_https) {
                    if probe_is_expired(record, now, self.config.probe_ttl) {
                        1
                    } else {
                        0
                    }
                } else if if require_https {
                    record.https_tunnel == ProxyCapabilityStatus::Available
                } else {
                    record.http_forwarding == ProxyCapabilityStatus::Available
                } {
                    2
                } else {
                    3
                }
            };
            rank(a)
                .cmp(&rank(b))
                .then_with(|| {
                    a.latency_ms
                        .unwrap_or(u64::MAX)
                        .cmp(&b.latency_ms.unwrap_or(u64::MAX))
                })
                .then(a.failure_count.cmp(&b.failure_count))
                .then_with(|| a.authority().cmp(&b.authority()))
        });

        let candidates: Vec<ProxyRecord> = candidates.into_iter().cloned().collect();
        drop(inner);
        let mut pending = Vec::new();
        for candidate in candidates {
            let Some(candidate) = self
                .inner
                .read()
                .await
                .records
                .get(&candidate.authority())
                .cloned()
            else {
                continue;
            };
            let Some(country) = candidate
                .country
                .as_ref()
                .filter(|country| countries.contains(country))
            else {
                continue;
            };
            if !is_selection_candidate(
                &candidate,
                country,
                require_https,
                destination,
                SystemTime::now(),
                self.probe_mode(),
            ) {
                continue;
            }
            if !needs_capability_probe(&candidate, require_https)
                && !probe_is_expired(&candidate, SystemTime::now(), self.config.probe_ttl)
            {
                tracing::info!(country = ?candidate.country, proxy = %candidate.authority(), require_https,
                    "selected validated cached proxy without prechecks");
                return Some(candidate);
            }
            pending.push(candidate);
        }
        // Poll both stages together: a suspended precheck can own a commit lock
        // needed by a validation, even while all validation slots are occupied.
        let validations = stream::iter(pending.into_iter().map(|candidate| async move {
            let candidate = async {
                let Some(probe) = &self.probe else {
                    return Some(candidate);
                };
                if recent_probe_attempt(&candidate, "tcp", self.config.probe_ttl) {
                    return (candidate.tcp_reachable == Some(true)).then_some(candidate);
                }
                let key = format!("tcp:{}", candidate.authority());
                let inventory = self.clone();
                let probe = probe.clone();
                self.shared_operation(
                    key,
                    false,
                    async move {
                        let _slot = inventory.tcp_slots.acquire().await.ok()?;
                        let started = epoch_millis();
                        let result = probe.precheck_endpoint(&candidate).await;
                        let reachable = result.is_ok();
                        inventory
                            .save_probe_attempt(
                                &candidate.authority(),
                                "tcp",
                                started,
                                Some(reachable),
                            )
                            .await;
                        reachable.then_some(candidate)
                    }
                    .boxed(),
                )
                .await
            }
            .await?;
            if self.probe.is_none() {
                return self
                    .validate_selection_candidate(candidate, require_https, destination)
                    .await;
            }
            let key = format!(
                "validate:{}:{}:{}",
                candidate.authority(),
                require_https,
                self.probe
                    .as_ref()
                    .map(|p| p.configuration_id())
                    .unwrap_or_default()
            );
            let inventory = self.clone();
            let validated = self
                .shared_operation(
                    key,
                    true,
                    async move {
                        let _slot = inventory.validation_slots.acquire().await.ok()?;
                        inventory
                            .validate_selection_candidate(candidate, require_https, None)
                            .await
                    }
                    .boxed(),
                )
                .await?;
            let latest = self
                .inner
                .read()
                .await
                .records
                .get(&validated.authority())
                .cloned()?;
            (latest
                .country
                .as_ref()
                .is_some_and(|country| countries.contains(country))
                && is_eligible(
                    &latest,
                    latest.country.as_deref()?,
                    require_https,
                    destination,
                    SystemTime::now(),
                    self.probe_mode(),
                ))
            .then_some(latest)
        }))
        .buffer_unordered(self.config.probe_batch_size.clamp(1, 8));
        futures::pin_mut!(validations);
        while let Some(result) = validations.next().await {
            if result.is_some() {
                return result;
            }
        }
        None
    }

    /// Shares an owned probe task without extending the caller's polling stack.
    /// The last subscriber aborts the task; caller deadlines and slots stay local.
    async fn shared_operation(
        &self,
        key: String,
        validation: bool,
        operation: BoxFuture<'static, Option<ProxyRecord>>,
    ) -> Option<ProxyRecord> {
        let search = ProxySearchContext::current();
        let wait = async {
            let _local = if let Some(search) = &search {
                let slots = if validation {
                    &search.validation_slots
                } else {
                    &search.tcp_slots
                };
                Some(slots.acquire().await.ok()?)
            } else {
                None
            };
            if search
                .as_ref()
                .is_some_and(|search| !search.reserve(validation, &key))
            {
                return None;
            }
            let mut reservation = search.as_ref().map(|search| SearchReservation {
                search: search.clone(),
                key: key.clone(),
                validation,
                completed: false,
            });
            let shared = {
                let mut registry = self.shared_work.lock().await;
                registry.retain(|_, weak| weak.upgrade().is_some());
                if let Some(existing) = registry.get(&key).and_then(WeakShared::upgrade) {
                    existing
                } else {
                    let work_key = key.clone();
                    let future = async move {
                        let mut tasks = tokio::task::JoinSet::new();
                        tasks.spawn(ProxySearchContext::unscoped(async move {
                            let mut execution = SharedExecution {
                                key: work_key,
                                completed: false,
                            };
                            let result = operation.await;
                            execution.completed = true;
                            result
                        }));
                        match tasks.join_next().await {
                            Some(Ok(result)) => result,
                            Some(Err(error)) => {
                                tracing::warn!(
                                    panicked = error.is_panic(),
                                    "shared proxy probe task failed"
                                );
                                None
                            }
                            None => None,
                        }
                    }
                    .boxed()
                    .shared();
                    registry.insert(key, future.downgrade()?);
                    future
                }
            };
            let result = shared.await;
            if let Some(reservation) = &mut reservation {
                reservation.completed = true;
            }
            if let Some(search) = &search {
                search.completed_work(validation, result.is_some());
                if validation {
                    search.log_progress("capability validation completed");
                }
            }
            if validation && result.is_none() {
                if let Some(search) = &search {
                    search.failed_validation();
                }
            }
            result
        };
        match &search {
            Some(search) => search.technical(wait).await.ok().flatten(),
            None => wait.await,
        }
    }

    /// Persists phase attempts independently of health and capability results,
    /// so completed inconclusive validations remain throttled after restart.
    /// Cancellation without a completed result never writes a synthetic attempt.
    async fn save_probe_attempt(
        &self,
        authority: &str,
        phase: &str,
        started: u64,
        tcp: Option<bool>,
    ) {
        let updated = {
            let mut inner = self.inner.write().await;
            let Some(record) = inner.records.get_mut(authority) else {
                return;
            };
            record.probe_attempts.insert(phase.to_string(), started);
            if let Some(reachable) = tcp {
                record.tcp_reachable = Some(reachable);
                if reachable && record.status == ProxyRuntimeStatus::Ko {
                    record.status = ProxyRuntimeStatus::Unknown;
                    record.cooldown_until = None;
                }
                let now = SystemTime::now();
                record.record_observation(crate::core::ProxyObservation {
                    context: crate::core::ProxyProbeContext {
                        phase: crate::core::ProxyObservationPhase::EndpointConnect,
                        protocol: None,
                        target: None,
                        method: None,
                        configuration: "tcp-precheck-v1".into(),
                    },
                    outcome: if reachable {
                        crate::core::ProxyObservationOutcome::Succeeded
                    } else {
                        crate::core::ProxyObservationOutcome::Failed {
                            scope: crate::core::ProxyFailureScope::Proxy,
                            cause: "endpoint_connection_failed".into(),
                        }
                    },
                    completed_at: now,
                    last_succeeded_at: reachable.then_some(now),
                    http_status: None,
                });
            }
            record.clone()
        };
        #[cfg(feature = "persistence")]
        self.persist_records(std::slice::from_ref(&updated)).await;
        #[cfg(not(feature = "persistence"))]
        drop(updated);
        if tcp == Some(false) {
            self.record_global_failure(authority).await;
        }
    }

    /// Validates missing capabilities on demand without holding inventory data
    /// locks during network I/O. Inconclusive attempts are throttled by probe TTL.
    async fn validate_selection_candidate(
        &self,
        candidate: ProxyRecord,
        require_https: bool,
        destination: Option<&Destination>,
    ) -> Option<ProxyRecord> {
        let Some(probe) = &self.probe else {
            return is_eligible(
                &candidate,
                candidate.country.as_deref()?,
                require_https,
                destination,
                SystemTime::now(),
                self.probe_mode(),
            )
            .then_some(candidate);
        };
        let authority = candidate.authority();
        let endpoint_lock = {
            let mut locks = self.selection_probe_locks.lock().await;
            locks.retain(|_, lock| Arc::strong_count(lock) > 1);
            Arc::clone(
                locks
                    .entry(authority.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let _guard = endpoint_lock.lock().await;
        let mut record = self.inner.read().await.records.get(&authority)?.clone();
        let country = record.country.clone()?;
        if !is_selection_candidate(
            &record,
            &country,
            require_https,
            destination,
            SystemTime::now(),
            self.probe_mode(),
        ) {
            return None;
        }
        if needs_capability_probe(&record, require_https)
            || probe_is_expired(&record, SystemTime::now(), self.config.probe_ttl)
        {
            let phase = if require_https { "https" } else { "http" };
            if recent_probe_attempt(&record, phase, self.config.probe_ttl) {
                return (complementary_trial_allowed(&record, self.probe_mode())
                    && is_eligible(
                        &record,
                        &country,
                        require_https,
                        destination,
                        SystemTime::now(),
                        self.probe_mode(),
                    ))
                .then_some(record);
            }
            let started = epoch_millis();
            tracing::info!(proxy = %authority, require_https, "probing proxy capabilities before selection");
            let result = probe.probe_required(&mut record, require_https).await;
            tracing::info!(
                proxy = %authority,
                require_https,
                http_forwarding = ?record.http_forwarding,
                https_tunnel = ?record.https_tunnel,
                destination_tls = ?record.destination_tls,
                capability_proven = !needs_capability_probe(&record, require_https),
                error = ?result.err(),
                "completed on-demand proxy capability probe"
            );
            let endpoint_failed = record.observations.iter().any(|observation| {
                observation.context.phase == crate::core::ProxyObservationPhase::EndpointConnect
                    && observation
                        .completed_at
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                        >= u128::from(started)
                    && matches!(
                        observation.outcome,
                        crate::core::ProxyObservationOutcome::Failed { .. }
                    )
            });
            record.probe_attempts.insert(phase.to_string(), started);
            let retained = self.add_or_update(vec![record]).await;
            #[cfg(feature = "persistence")]
            self.persist_records(&retained).await;
            if endpoint_failed {
                self.record_global_failure(&authority).await;
                return None;
            }
            record = retained.into_iter().next()?;
        }
        if needs_capability_probe(&record, require_https)
            && !complementary_trial_allowed(&record, self.probe_mode())
        {
            return None;
        }
        is_eligible(
            &record,
            &country,
            require_https,
            destination,
            SystemTime::now(),
            self.probe_mode(),
        )
        .then_some(record)
    }

    /// Loads proxies from the provider after selection found no eligible cached
    /// candidate.
    ///
    /// Calls for the same country share a per-country asynchronous lock and a
    /// refresh-attempt cooldown. One call performs the network work while
    /// concurrent or subsequent calls reuse inventory state until the cooldown
    /// expires, including when the provider failed or returned no candidates.
    /// Provider records are deduplicated and stored without eager batch probing.
    /// Selection runs prechecks and full validations progressively within the shared deadline.
    /// The shared refresh owns a child task to avoid extending the caller's HTTP
    /// polling stack; dropping its last subscriber aborts that task.
    async fn load_if_needed(&self, country: &str) {
        let search = ProxySearchContext::current();
        if search.as_ref().is_some_and(|search| !search.can_discover()) {
            return;
        }
        let wait = async {
            let _local = if let Some(search) = &search {
                Some(search.provider_slots.acquire().await.ok()?)
            } else {
                None
            };
            let key = format!("provider:{country}");
            let shared = {
                let mut registry = self.shared_work.lock().await;
                registry.retain(|_, weak| weak.upgrade().is_some());
                if let Some(existing) = registry.get(&key).and_then(WeakShared::upgrade) {
                    existing
                } else {
                    let inventory = self.clone();
                    let country = country.to_string();
                    let future = async move {
                        // The owner breaks the nested HTTP polling stack and aborts
                        // the child when the last shared subscriber disappears.
                        let mut tasks = tokio::task::JoinSet::new();
                        let load_country = country.clone();
                        tasks.spawn(ProxySearchContext::unscoped(async move {
                            let Ok(_slot) = inventory.provider_slots.acquire().await else {
                                return;
                            };
                            inventory.load_country(&load_country).await;
                        }));
                        if let Some(Err(error)) = tasks.join_next().await {
                            tracing::warn!(
                                country,
                                panicked = error.is_panic(),
                                "dynamic proxy provider task failed"
                            );
                        }
                        None
                    }
                    .boxed()
                    .shared();
                    registry.insert(key, future.downgrade()?);
                    future
                }
            };
            shared.await
        };
        if let Some(search) = &search {
            let _ = search.technical(wait).await;
        } else {
            wait.await;
        }
    }

    async fn load_country(&self, country: &str) {
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
                let cooldown = if inner.country_early_refresh.contains(country) {
                    self.config.provider_min_refresh_interval
                } else {
                    self.config
                        .provider_refresh_cooldown
                        .max(self.config.provider_min_refresh_interval)
                };
                if elapsed < cooldown {
                    tracing::debug!(
                        country = %country,
                        elapsed_ms = elapsed.as_millis(),
                        cooldown_ms = cooldown.as_millis(),
                        "dynamic proxy provider refresh suppressed by inventory cooldown"
                    );
                    return;
                }
            }
            inner.country_early_refresh.remove(country);
            inner
                .country_last_refresh_attempts
                .insert(country.to_string(), now);
        }

        tracing::info!(
            country = %country,
            cooldown_seconds = self.config.provider_refresh_cooldown.as_secs(),
            "refreshing dynamic proxy provider during candidate exploration"
        );

        if let Err(error) = provider
            .load_proxies_progressively(
                crate::core::ProxyLoadRequest {
                    countries: vec![country.to_string()],
                },
                Arc::new(InventoryBatchSink {
                    inventory: self.clone(),
                    country: country.to_string(),
                }),
            )
            .await
        {
            tracing::warn!(country, %error, "progressive proxy load stopped; published batches retained");
        }
    }

    async fn merge_provider_batch(&self, country: &str, records: Vec<ProxyRecord>) {
        let loaded_count = records.len();
        let existing = self.inner.read().await.records.clone();
        let (records, probe_indices, duplicate_count, cached_count) =
            prepare_provider_records(records, &existing, SystemTime::now(), self.config.probe_ttl);
        let pending_probe_count = probe_indices.len();
        let probe_stats = selection_stats(
            records.iter(),
            country,
            false,
            None,
            SystemTime::now(),
            self.config.probe_ttl,
            self.probe_mode(),
        );
        let protocol_conflict_count = records
            .iter()
            .filter(|record| declared_protocol_count(record) > 1)
            .count();
        let undeclared_protocol_count = records
            .iter()
            .filter(|record| runtime_protocol_is_undeclared(record))
            .count();
        tracing::info!(
            country = %country,
            loaded_count,
            deduplicated_count = records.len(),
            duplicate_count,
            protocol_conflict_count,
            undeclared_protocol_count,
            cached_count,
            pending_probe_count,
            ok = probe_stats.ok,
            unknown = probe_stats.unknown,
            ko = probe_stats.ko,
            auth_required = probe_stats.authentication_required,
            auth_flag = probe_stats.authentication_required_flag,
            http_forward_available = probe_stats.http_forward_available,
            http_forward_unavailable = probe_stats.http_forward_unavailable,
            http_forward_unknown = probe_stats.http_forward_unknown,
            https_tunnel_available = probe_stats.https_tunnel_available,
            https_tunnel_unavailable = probe_stats.https_tunnel_unavailable,
            https_tunnel_unknown = probe_stats.https_tunnel_unknown,
            destination_tls_available = probe_stats.destination_tls_available,
            destination_tls_unavailable = probe_stats.destination_tls_unavailable,
            destination_tls_unknown = probe_stats.destination_tls_unknown,
            proxy_tls_valid = probe_stats.proxy_tls_valid,
            proxy_tls_invalid = probe_stats.proxy_tls_invalid,
            proxy_tls_unknown = probe_stats.proxy_tls_unknown,
            "loaded dynamic proxies prepared for on-demand probing"
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
            http_forward_available = http_stats.http_forward_available,
            http_forward_unavailable = http_stats.http_forward_unavailable,
            http_forward_unknown = http_stats.http_forward_unknown,
            https_tunnel_available = https_stats.https_tunnel_available,
            https_tunnel_unavailable = https_stats.https_tunnel_unavailable,
            https_tunnel_unknown = https_stats.https_tunnel_unknown,
            destination_tls_available = https_stats.destination_tls_available,
            destination_tls_unavailable = https_stats.destination_tls_unavailable,
            destination_tls_unknown = https_stats.destination_tls_unknown,
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
        self.discovery_revision
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }

    /// Allows the provider to be consulted again after an observed proxy
    /// connection failure.
    ///
    /// This does not load any data itself. It shortens the per-country
    /// refresh-attempt cooldown to the configured minimum so a later selection may refresh the provider
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
            inner.country_early_refresh.insert(country.to_string());
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
            self.probe_mode(),
        ))
    }

    // ── Failure tracking ─────────────────────────────────────────────

    /// Stores a contextual phase observation without synthesizing global failure.
    ///
    /// # Arguments
    /// - `authority`: Endpoint to which the observation belongs.
    /// - `observation`: Completed phase, never a synthetic cancellation failure.
    pub async fn record_observation(
        &self,
        authority: &str,
        observation: crate::core::ProxyObservation,
    ) {
        let updated = {
            let mut inner = self.inner.write().await;
            let Some(record) = inner.records.get_mut(authority) else {
                return;
            };
            record.record_observation(observation);
            record.clone()
        };
        #[cfg(feature = "persistence")]
        self.persist_records(std::slice::from_ref(&updated)).await;
        #[cfg(not(feature = "persistence"))]
        drop(updated);
    }

    /// Records a destination-scoped failure without globally degrading or evicting the proxy.
    ///
    /// # Arguments
    /// - `authority`: Endpoint actually used by the attempt.
    /// - `destination`: Exact destination of the failed exchange.
    /// - `reason`: Contextual failure category.
    ///
    /// # Returns
    /// Always `false`; retained for compatibility with the former promotion API.
    pub async fn record_destination_failure(
        &self,
        authority: &str,
        destination: &Destination,
        reason: crate::core::ProxyDestinationFailureReason,
    ) -> bool {
        self.record_destination_failure_with_cooldown(authority, destination, reason, None)
            .await
    }

    /// Records a destination failure without shortening an explicit origin rejection.
    /// Ambiguous failures use at most 30 seconds, including legacy stored exclusions
    /// when determining eligibility. Shorter configured limits remain effective.
    ///
    /// # Arguments
    /// - `authority`: Endpoint actually used.
    /// - `destination`: Exact observed destination.
    /// - `reason`: Contextual failure category.
    /// - `cooldown`: Optional override of the inventory default.
    pub async fn record_destination_failure_with_cooldown(
        &self,
        authority: &str,
        destination: &Destination,
        reason: crate::core::ProxyDestinationFailureReason,
        cooldown: Option<Duration>,
    ) -> bool {
        let cooldown = cooldown.unwrap_or(self.config.destination_failure_cooldown);
        let cooldown = if reason == crate::core::ProxyDestinationFailureReason::Other {
            cooldown.min(PROXY_TRANSIENT_FAILURE_COOLDOWN)
        } else {
            cooldown
        };
        let updated = {
            let mut inner = self.inner.write().await;
            let Some(record) = inner.records.get_mut(authority) else {
                return false;
            };
            let now = SystemTime::now();
            let host = destination.host_for_protocol();
            let scheme = destination_scheme(destination);
            if let Some(failure) = record.destination_failures.iter_mut().find(|failure| {
                failure.scheme == scheme && failure.host == host && failure.port == destination.port
            }) {
                failure.failure_count = failure.failure_count.saturating_add(1);
                failure.last_failed = now;
                if failure.reason != crate::core::ProxyDestinationFailureReason::BlockedByOrigin
                    || reason == crate::core::ProxyDestinationFailureReason::BlockedByOrigin
                {
                    failure.reason = reason;
                }
                failure.cooldown_until = failure.cooldown_until.max(Some(
                    now.checked_add(cooldown)
                        .unwrap_or(now + self.config.destination_failure_cooldown),
                ));
            } else {
                record
                    .destination_failures
                    .push(crate::core::ProxyDestinationFailure {
                        scheme,
                        host,
                        port: destination.port,
                        reason,
                        failure_count: 1,
                        last_failed: now,
                        cooldown_until: Some(
                            now.checked_add(cooldown)
                                .unwrap_or(now + self.config.destination_failure_cooldown),
                        ),
                    });
            }
            record.clone()
        };
        #[cfg(feature = "persistence")]
        self.persist_records(std::slice::from_ref(&updated)).await;
        #[cfg(not(feature = "persistence"))]
        drop(updated);
        false
    }

    /// Records a proven endpoint connection failure, with cooldown or age-based eviction.
    ///
    /// # Arguments
    /// * `authority` - Endpoint whose own connection failed, not a remote tunnel or TLS failure.
    pub async fn record_global_failure(&self, authority: &str) {
        if self.remove_if_stale(authority).await {
            return;
        }
        let updated = {
            let mut inner = self.inner.write().await;
            if let Some(record) = inner.records.get_mut(authority) {
                let now = SystemTime::now();
                record.status = ProxyRuntimeStatus::Ko;
                record.failure_count = record.failure_count.saturating_add(1);
                record.tcp_reachable = Some(false);
                record.cooldown_until = Some(now + self.config.ko_cooldown);
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
                record.last_validated_at = Some(SystemTime::now());
                record.health_evidence.last_positive_at = record
                    .health_evidence
                    .last_positive_at
                    .max(record.last_validated_at);
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
    /// the proxy as the preferred candidate for that exact destination. Clears
    /// older ambiguous failures for this association, preserving explicit origin
    /// rejections, newer failures and unrelated proxy or destination failure state.
    ///
    /// # Arguments
    /// - `authority`: Endpoint actually used by the accepted response.
    /// - `destination`: Exact origin that accepted the response.
    pub async fn record_validated_response(&self, authority: &str, destination: &Destination) {
        let accepted_at = SystemTime::now();
        let key = crate::core::ProxyDestinationPreference::destination_key(destination);
        let lock = self.destination_lock(&key).await;
        let _guard = lock.lock().await;
        #[cfg(feature = "persistence")]
        if !self
            .inner
            .read()
            .await
            .preferred_by_destination
            .contains_key(&key)
        {
            if let Some(repository) = &self.preferences {
                match repository.get(&key).await {
                    Ok(Some(preference)) => {
                        self.inner
                            .write()
                            .await
                            .preferred_by_destination
                            .insert(key.clone(), preference);
                    }
                    Err(error) => {
                        tracing::warn!(%error, destination = %key, "failed to load proxy history before accepted response")
                    }
                    _ => {}
                }
            }
        }
        let updated = {
            let mut inner = self.inner.write().await;
            let updated = inner.records.get_mut(authority).map(|record| {
                clear_superseded_destination_failures(record, destination, accepted_at);
                record.last_validated_at = record.last_validated_at.max(Some(accepted_at));
                record.health_evidence.last_positive_at = record
                    .health_evidence
                    .last_positive_at
                    .max(record.last_validated_at);
                record.clone()
            });
            if let Some(record) = &updated {
                inner
                    .preferred_by_destination
                    .entry(key.clone())
                    .or_insert_with(|| crate::core::ProxyDestinationPreference {
                        destination: key.clone(),
                        endpoints: Vec::new(),
                    })
                    .record_success(record.key(), accepted_at);
            }
            updated
        };
        if let Some(record) = updated {
            #[cfg(feature = "persistence")]
            {
                self.persist_accepted_record(&record, destination, accepted_at)
                    .await;
                let _commit = self.persistence_commits.lock().await;
                if let Some(repository) = &self.preferences {
                    let preference =
                        {
                            let inner = self.inner.read().await;
                            inner.preferred_by_destination.get(&key).cloned().map(
                                |mut preference| {
                                    preference.endpoints.retain(|item| {
                                        inner
                                            .records
                                            .values()
                                            .any(|record| record.key() == item.endpoint)
                                    });
                                    preference
                                },
                            )
                        };
                    if let Some(preference) = preference {
                        if let Err(error) = repository.save(&preference).await {
                            self.preference_persistence_failures
                                .fetch_add(1, Ordering::Relaxed);
                            tracing::error!(%error, destination = %key, endpoint = %authority,
                                "failed to persist accepted proxy preference; response and memory history preserved");
                        }
                    }
                }
            }
            #[cfg(not(feature = "persistence"))]
            drop(record);
            self.notify_destination_success();
        }
    }

    /// Evicts only on an observed endpoint connection failure after the retention threshold.
    async fn remove_if_stale(&self, authority: &str) -> bool {
        let _commit = self.persistence_commits.lock().await;
        let removed_record = {
            let mut inner = self.inner.write().await;
            let stale = inner
                .records
                .get(authority)
                .is_some_and(|record| proxy_validation_is_stale(record, SystemTime::now()));
            if !stale {
                return false;
            }
            let mut removed = inner.records.remove(authority);
            if let Some(record) = &mut removed {
                record.health_evidence.evicted_at = Some(SystemTime::now());
                record.cooldown_until = Some(SystemTime::now() + self.config.ko_cooldown);
                record.status = ProxyRuntimeStatus::Ko;
                record.tcp_reachable = Some(false);
                inner.evicted.insert(authority.to_string(), record.clone());
                if inner.evicted.len() > 1024 {
                    if let Some(oldest) = inner
                        .evicted
                        .iter()
                        .min_by_key(|(_, record)| record.health_evidence.evicted_at)
                        .map(|(key, _)| key.clone())
                    {
                        inner.evicted.remove(&oldest);
                    }
                }
                for preference in inner.preferred_by_destination.values_mut() {
                    preference
                        .endpoints
                        .retain(|item| item.endpoint != record.key());
                }
                inner
                    .affinity_bindings
                    .retain(|_, binding| binding.authority != authority);
            }
            if let Some(record) = &removed {
                if let Some(country) = &record.country {
                    if let Some(keys) = inner.country_records.get_mut(country) {
                        keys.retain(|key| key != authority);
                    }
                }
            }
            removed
        };

        #[cfg(feature = "persistence")]
        if let Some(record) = removed_record {
            if let Some(repository) = &self.persistence {
                if let Err(error) = repository.evict(&record).await {
                    tracing::error!(%error, endpoint = %authority, "failed to persist proxy endpoint eviction");
                }
            }
            if let Some(repository) = &self.preferences {
                if let Err(error) = repository.remove_endpoint(&record.key()).await {
                    tracing::warn!(%error, endpoint = %authority, "failed to clean evicted proxy preferences");
                }
            }
        }
        #[cfg(not(feature = "persistence"))]
        let _ = removed_record;

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
    include!("tests/proxy_lifecycle_tests.rs");
    use super::*;
    #[cfg(feature = "persistence")]
    use crate::core::proxy_repository::memory_proxy_store;
    #[cfg(feature = "persistence")]
    use crate::core::ProxyPreferenceRepository;
    #[cfg(feature = "persistence")]
    use crate::core::TypedProxyRepository;
    use crate::core::{
        ProxyAvailabilityHint, ProxyBatchSink, ProxyDestinationFailure, ProxyLoadRequest,
    };
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
            health_evidence: Default::default(),
            observations: Vec::new(),
            probe_attempts: Default::default(),
            tcp_reachable: None,
            protocol: Some(ProxyProtocol::Socks5),
            host: "merged.example".to_string(),
            port: 1080,
            country: Some("BE".to_string()),
            supports_https: Some(true),
            declarations: vec![ProxyDeclaration {
                source: "test-provider".to_string(),
                protocol: Some(ProxyProtocol::Socks5),
                supports_https: Some(true),
            }],
            status,
            http_forwarding: ProxyCapabilityStatus::Available,
            https_tunnel: ProxyCapabilityStatus::Available,
            destination_tls: ProxyCapabilityStatus::Unknown,
            proxy_tls_certificate: ProxyCapabilityStatus::Unknown,
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
        let mut provider_fresh_http = provider_fresh.clone();
        provider_fresh_http.protocol = Some(ProxyProtocol::Http);
        provider_fresh_http.declarations = vec![ProxyDeclaration {
            source: "secondary-provider".to_string(),
            protocol: Some(ProxyProtocol::Http),
            supports_https: None,
        }];
        let (prepared, probe_indices, duplicate_count, cached_count) = prepare_provider_records(
            vec![
                provider_fresh.clone(),
                provider_fresh_http,
                provider_untested,
                provider_expired,
                provider_new,
            ],
            &existing,
            now,
            config.probe_ttl,
        );

        assert_eq!(prepared.len(), 4);
        let mut expected_fresh = fresh;
        expected_fresh.supports_https = None;
        expected_fresh.declarations.push(ProxyDeclaration {
            source: "secondary-provider".to_string(),
            protocol: Some(ProxyProtocol::Http),
            supports_https: None,
        });
        assert_eq!(prepared[0], expected_fresh);
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
        retained.health_evidence.first_discovered_at = Some(SystemTime::now());
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
            tokio::spawn(async move {
                let search = ProxySearchContext::new(crate::core::ProxySearchLimits {
                    target: Duration::from_millis(10),
                    extension: Duration::ZERO,
                    ..Default::default()
                });
                search.scope(inventory.select("BE", true)).await
            })
        };
        let second = {
            let inventory = Arc::clone(&inventory);
            tokio::spawn(async move { inventory.select("BE", true).await })
        };

        let started = Instant::now();
        assert!(first.await?.is_err());
        assert!(second.await??.supports_https.unwrap_or(false));
        eprintln!(
            "lot7 shared provider: survivor_wait_us={}, provider_calls={}",
            started.elapsed().as_micros(),
            provider.calls.load(Ordering::SeqCst)
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

        let limits = crate::core::ProxySearchLimits::default();
        assert_eq!(limits.target, Duration::from_secs(120));
        assert_eq!(limits.extension, Duration::from_secs(15));
        let search = ProxySearchContext::new(limits);
        let hop = search.for_url("https://redirect.example/path");
        assert_eq!(hop.id(), search.id());
        for index in 0..201 {
            assert!(hop.reserve(false, &format!("tcp:{index}")));
        }
        assert!(!search.reserve(false, "tcp:0"));
        for index in 0..201 {
            assert!(search.reserve(true, &format!("validate:{index}")));
        }
        assert!(!hop.reserve(true, "validate:0"));
        let destination = Destination::host_port("origin.example", 443);
        for index in 0..201 {
            search.start_origin(&format!("endpoint:{index}"), &destination)?;
            search.end_attempt();
        }
        assert!(search.start_origin("endpoint:0", &destination).is_err());
        assert!(search.attempted("endpoint:200", Some(&destination)));
        assert!(!search.attempted("endpoint:201", Some(&destination)));
        hop.start_origin("endpoint:0", &destination)?;
        hop.end_attempt();
        assert!(hop.start_origin("endpoint:0", &destination).is_err());
        assert!(search.can_discover());
        let configured = crate::core::TimeoutConfig::default();
        let localized = search.connection_timeouts("endpoint:0", &configured);
        assert_eq!(localized.connect_ms, 3_000);
        assert_eq!(localized.handshake_ms, 5_000);
        assert_eq!(localized.idle_ms, configured.idle_ms);
        assert_eq!(
            search.connection_timeouts("ordinary", &configured),
            configured
        );
        let shorter = crate::core::TimeoutConfig {
            connect_ms: 100,
            handshake_ms: 200,
            idle_ms: 300,
        };
        assert_eq!(hop.connection_timeouts("endpoint:0", &shorter), shorter);
        let known = ProxySearchContext::default();
        known.mark_selection(
            "known:0",
            &destination,
            crate::core::ProxySelectionProvenance::AcceptedDestination,
        );
        known.start_origin("known:0", &destination)?;
        let reused = known.connection_timeouts("known:0", &configured);
        assert_eq!(reused.connect_ms, 30_000);
        assert_eq!(reused.handshake_ms, 30_000);
        assert_eq!(reused.idle_ms, configured.idle_ms);
        assert_eq!(known.connection_timeouts("known:0", &shorter), shorter);
        assert_eq!(known.connection_timeout(), Duration::from_secs(60));
        assert_eq!(search.connection_timeout(), Duration::from_secs(15));
        known.end_attempt();
        known.start_origin("fresh:0", &destination)?;
        assert_eq!(known.connection_timeout(), Duration::from_secs(15));
        for validation in [false, true] {
            let caller_task = tokio::task::try_id();
            assert!(search
                .scope(
                    inventory.shared_operation(
                        format!("isolated:{validation}"),
                        validation,
                        async move {
                            assert_ne!(
                                tokio::task::try_id(),
                                caller_task,
                                "probe work must not extend the caller's polling stack"
                            );
                            assert!(ProxySearchContext::current().is_none());
                            Some(record(ProxyRuntimeStatus::Ok))
                        }
                        .boxed(),
                    )
                )
                .await
                .is_some());
            let key = format!("cancelled:{validation}");
            let slots = if validation {
                inventory.validation_slots.clone()
            } else {
                inventory.tcp_slots.clone()
            };
            let available = slots.available_permits();
            let operation_slots = slots.clone();
            let (started, running) = tokio::sync::oneshot::channel();
            {
                let pending = search.scope(
                    inventory.shared_operation(
                        key.clone(),
                        validation,
                        async move {
                            let _slot = operation_slots.acquire().await.ok()?;
                            let _ = started.send(());
                            futures::future::pending().await
                        }
                        .boxed(),
                    ),
                );
                tokio::pin!(pending);
                tokio::select! {
                    _ = &mut pending => panic!("fixture work must remain pending"),
                    result = running => result.expect("owned probe task must start"),
                }
                assert!(!search.reserve(validation, &key));
            }
            tokio::time::timeout(Duration::from_secs(1), async {
                while slots.available_permits() != available {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("last-subscriber cancellation must release probe slots");
            assert!(search.reserve(validation, &key));
        }
        let tcp = search.tcp_slots.try_acquire_many(8)?;
        assert!(hop.tcp_slots.try_acquire().is_err());
        drop(tcp);
        let validation = search.validation_slots.try_acquire_many(2)?;
        assert!(hop.validation_slots.try_acquire().is_err());
        drop(validation);
        let country = search.provider_slots.try_acquire_many(2)?;
        assert!(hop.provider_slots.try_acquire().is_err());
        drop(country);
        let queued = search.tcp_slots.acquire_many(8).await?;
        let waiter = hop.tcp_slots.acquire();
        let wait_started = Instant::now();
        let (permit, ()) = tokio::join!(waiter, async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            drop(queued);
        });
        drop(permit?);
        eprintln!("lot7 slot wait: wait_us={}, tcp_limit=8, validation_limit=2, country_limit=2, prechecks=201, validations=201, endpoints=201, work_count_limit=none", wait_started.elapsed().as_micros());

        for engaged in [false, true] {
            let timed = ProxySearchContext::new(crate::core::ProxySearchLimits {
                target: Duration::from_millis(30),
                extension: Duration::from_millis(60),
                ..Default::default()
            });
            if engaged {
                timed.start_origin("endpoint", &destination)?;
            }
            let deadline_started = Instant::now();
            let result: Result<()> = tokio::time::timeout(Duration::from_secs(1), async {
                if engaged {
                    timed.origin(futures::future::pending()).await
                } else {
                    timed.technical(futures::future::pending()).await
                }
            })
            .await
            .expect("search deadlines must stop pending work");
            assert!(
                deadline_started.elapsed() >= Duration::from_millis(if engaged { 90 } else { 30 })
            );
            assert!(matches!(result, Err(ProxyError::Timeout(_))));
            assert!(!timed.can_start());
            assert!(timed.start_origin("new-endpoint", &destination).is_err());
            assert!(timed.technical(async {}).await.is_err());
        }
        let margin = ProxySearchContext::new(crate::core::ProxySearchLimits {
            target: Duration::from_millis(30),
            extension: Duration::from_millis(60),
            ..Default::default()
        });
        margin.start_origin("endpoint", &destination)?;
        margin
            .origin(async {
                tokio::time::sleep(Duration::from_millis(40)).await;
                assert!(!margin.can_start());
                assert!(margin.start_origin("new-endpoint", &destination).is_err());
                assert!(margin.technical(async {}).await.is_err());
            })
            .await?;
        let accepted = ProxySearchContext::new(crate::core::ProxySearchLimits {
            target: Duration::from_millis(30),
            extension: Duration::from_millis(60),
            ..Default::default()
        });
        accepted.start_origin("endpoint", &destination)?;
        accepted
            .origin(async {
                accepted.accept();
                tokio::time::sleep(Duration::from_millis(110)).await;
            })
            .await?;
        assert!(!accepted.can_start());
        use crate::core::{proxy_retry_allowed, ProxyRequestDelivery};
        for method in ["GET", "HEAD", "POST"] {
            assert!(proxy_retry_allowed(
                method,
                false,
                ProxyRequestDelivery::NotSent,
                false
            ));
            assert_eq!(
                proxy_retry_allowed(method, false, ProxyRequestDelivery::PossiblySent, false),
                method != "POST"
            );
            assert!(!proxy_retry_allowed(
                method,
                true,
                ProxyRequestDelivery::ResponseReceived,
                false
            ));
            assert!(proxy_retry_allowed(
                method,
                true,
                ProxyRequestDelivery::ResponseReceived,
                true
            ));
        }
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
        http_only.https_tunnel = ProxyCapabilityStatus::Unavailable;
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
                provider_min_refresh_interval: Duration::from_millis(10),
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

        validated.last_checked =
            Some(SystemTime::now() - InventoryConfig::default().probe_ttl - Duration::from_secs(1));
        inventory.add_or_update(vec![validated.clone()]).await;
        assert_eq!(
            inventory
                .select_any_for_destination(&["BE".to_string()], true, Some(&preferred_destination))
                .await
                .unwrap()
                .authority(),
            validated.authority()
        );

        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&preferred_destination))
                .await
                .unwrap()
                .authority(),
            validated.authority()
        );

        {
            let inventory = Arc::new(ProxyInventory::new(InventoryConfig::default(), None, None));
            inventory
                .add_or_update(vec![fastest.clone(), validated.clone()])
                .await;
            let countries = vec!["BE".to_string()];
            let leader = ProxySearchContext::default();
            let selected = leader
                .scope(inventory.select_any_for_destination_with_affinity(
                    &countries,
                    true,
                    Some(&preferred_destination),
                    Some("shared-cold-affinity"),
                ))
                .await
                .unwrap();
            let token = leader
                .scope(async {
                    crate::core::ProxySelectionToken::new(
                        inventory.clone(),
                        selected.authority(),
                        preferred_destination.clone(),
                    )
                })
                .await;
            assert_eq!(
                token.provenance(),
                crate::core::ProxySelectionProvenance::Discovery
            );
            let follower = ProxySearchContext::default();
            let pending = follower.scope(inventory.select_any_for_destination_with_affinity(
                &countries,
                true,
                Some(&preferred_destination),
                Some("shared-cold-affinity"),
            ));
            tokio::pin!(pending);
            assert!(
                tokio::time::timeout(Duration::from_millis(10), &mut pending)
                    .await
                    .is_err(),
                "cold follower must await the leader's business result"
            );
            let unrelated = inventory
                .select_any_for_destination(&countries, true, Some(&other_destination))
                .await
                .unwrap();
            assert_eq!(unrelated.authority(), fastest.authority());
            token.record_accepted().await;
            let selected = tokio::time::timeout(Duration::from_secs(1), &mut pending)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(selected.authority(), token.endpoint());
            assert_eq!(
                follower.selection_provenance(&selected.authority(), &preferred_destination),
                crate::core::ProxySelectionProvenance::AcceptedDestination
            );

            let cancelled = Destination::host_port("cancelled.example", 443)
                .with_protocol(crate::core::ApplicationProtocol::Https);
            let selected = leader
                .scope(inventory.select_any_for_destination(&countries, true, Some(&cancelled)))
                .await
                .unwrap();
            let token = leader
                .scope(async {
                    crate::core::ProxySelectionToken::new(
                        inventory.clone(),
                        selected.authority(),
                        cancelled.clone(),
                    )
                })
                .await;
            let abandoned = ProxySearchContext::default();
            assert!(tokio::time::timeout(
                Duration::from_millis(10),
                abandoned.scope(inventory.select_any_for_destination(
                    &countries,
                    true,
                    Some(&cancelled)
                ))
            )
            .await
            .is_err());
            drop(token);
            assert!(
                tokio::time::timeout(
                    Duration::from_secs(1),
                    inventory.select_any_for_destination(&countries, true, Some(&cancelled))
                )
                .await
                .unwrap()
                .is_ok(),
                "cancelling a leader and a waiter must release discovery ownership"
            );
        }
        {
            let provider = Arc::new(CountingProvider {
                records: Vec::new(),
                calls: AtomicUsize::new(0),
                delay: Duration::from_secs(60),
            });
            let waiting_inventory =
                ProxyInventory::new(InventoryConfig::default(), Some(provider.clone()), None);
            let countries = vec!["BE".into(), "FR".into()];
            let search = ProxySearchContext::default();
            let waiting = search.scope(waiting_inventory.select_any_for_destination(
                &countries,
                true,
                Some(&preferred_destination),
            ));
            tokio::pin!(waiting);
            assert!(
                tokio::time::timeout(Duration::from_millis(10), &mut waiting)
                    .await
                    .is_err()
            );
            assert!(provider.calls.load(Ordering::SeqCst) > 0);
            waiting_inventory
                .add_or_update(vec![validated.clone()])
                .await;
            waiting_inventory
                .record_validated_response(&validated.authority(), &preferred_destination)
                .await;
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), &mut waiting)
                    .await
                    .unwrap()
                    .unwrap()
                    .authority(),
                validated.authority(),
                "an accepted response must wake discovery already awaiting a slow provider"
            );
        }
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

        validated.destination_tls = ProxyCapabilityStatus::Available;
        inventory.add_or_update(vec![validated.clone()]).await;
        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&other_destination))
                .await
                .unwrap()
                .authority(),
            validated.authority()
        );
        assert_eq!(
            inventory.select("BE", false).await.unwrap().authority(),
            fastest.authority()
        );

        let mut unknown = record(ProxyRuntimeStatus::Ok);
        unknown.host = "unknown.example".to_string();
        unknown.latency_ms = Some(1);
        unknown.https_tunnel = ProxyCapabilityStatus::Unknown;
        inventory.add_or_update(vec![unknown]).await;
        assert_eq!(
            inventory.select("BE", true).await.unwrap().authority(),
            validated.authority()
        );
        let mut other_country = record(ProxyRuntimeStatus::Ok);
        other_country.host = "accepted-fr.example".into();
        other_country.country = Some("FR".into());
        inventory.add_or_update(vec![other_country.clone()]).await;
        inventory
            .record_validated_response(&other_country.authority(), &preferred_destination)
            .await;
        for countries in [
            vec!["BE".into(), "FR".into()],
            vec!["FR".into(), "BE".into()],
        ] {
            assert_eq!(
                inventory
                    .select_any_for_destination(&countries, true, Some(&preferred_destination))
                    .await
                    .unwrap()
                    .authority(),
                other_country.authority()
            );
        }
        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&preferred_destination))
                .await
                .unwrap()
                .authority(),
            validated.authority()
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
        for certificate in [
            ProxyCapabilityStatus::Unavailable,
            ProxyCapabilityStatus::Available,
        ] {
            inventory
                .inner
                .write()
                .await
                .records
                .get_mut(&validated_authority)
                .unwrap()
                .proxy_tls_certificate = certificate;
            let selected = inventory
                .select_for_destination("BE", true, Some(&destination))
                .await
                .unwrap();
            assert_eq!(
                selected.authority(),
                if certificate == ProxyCapabilityStatus::Unavailable {
                    fallback.authority()
                } else {
                    validated_authority.clone()
                }
            );
        }
        let other_destination = Destination::host_port("other.example", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        for target in [&destination, &other_destination] {
            inventory
                .record_destination_failure(
                    &validated_authority,
                    target,
                    crate::core::ProxyDestinationFailureReason::Other,
                )
                .await;
        }
        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&destination))
                .await
                .unwrap()
                .authority(),
            fallback.authority()
        );
        inventory
            .record_validated_response(&validated_authority, &destination)
            .await;
        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&destination))
                .await
                .unwrap()
                .authority(),
            validated_authority
        );
        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&other_destination))
                .await
                .unwrap()
                .authority(),
            fallback.authority()
        );
        inventory
            .record_destination_failure(
                &validated_authority,
                &destination,
                crate::core::ProxyDestinationFailureReason::Other,
            )
            .await;
        {
            let mut inner = inventory.inner.write().await;
            let record = inner.records.get_mut(&validated_authority).unwrap();
            record
                .destination_failures
                .iter_mut()
                .find(|failure| failure.host == "blocked.example")
                .unwrap()
                .last_failed = SystemTime::now() + Duration::from_secs(60);
        }
        inventory
            .record_validated_response(&validated_authority, &destination)
            .await;
        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&destination))
                .await
                .unwrap()
                .authority(),
            fallback.authority(),
            "acceptance must preserve newer failures"
        );
        inventory
            .record_destination_failure(
                &validated_authority,
                &destination,
                crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
            )
            .await;
        inventory
            .record_validated_response(&validated_authority, &destination)
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
        inventory
            .record_validated_response(&validated_authority, &destination)
            .await;
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
        let now = SystemTime::now();
        let destination = Destination::host_port("transient.example", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        let mut proxy = record(ProxyRuntimeStatus::Ok);
        proxy
            .destination_failures
            .push(crate::core::ProxyDestinationFailure {
                scheme: "https".into(),
                host: "transient.example".into(),
                port: 443,
                reason: crate::core::ProxyDestinationFailureReason::Other,
                failure_count: 1,
                last_failed: now - Duration::from_secs(31),
                cooldown_until: Some(now + PROXY_DESTINATION_FAILURE_COOLDOWN),
            });
        assert!(
            !has_active_destination_cooldown(&proxy, &destination, now),
            "legacy ambiguous exclusions must use the short policy without resetting the cache"
        );
        proxy.destination_failures[0].last_failed = now;
        assert!(has_active_destination_cooldown(&proxy, &destination, now));
        proxy.destination_failures[0].last_failed = now - Duration::from_secs(31);
        proxy.destination_failures[0].reason =
            crate::core::ProxyDestinationFailureReason::BlockedByOrigin;
        assert!(has_active_destination_cooldown(&proxy, &destination, now));
    }

    #[tokio::test]
    async fn multi_country_selection_checks_all_cached_countries_before_loading() {
        for require_https in [false, true] {
            let provider = Arc::new(CountingProvider {
                records: Vec::new(),
                calls: AtomicUsize::new(0),
                delay: Duration::ZERO,
            });
            let probe = Arc::new(ProxyProbe::new(crate::core::ProbeConfig::default()));
            let inventory = ProxyInventory::new(
                InventoryConfig::default(),
                Some(provider.clone()),
                Some(probe.clone()),
            );
            let mut cached = record(ProxyRuntimeStatus::Ok);
            cached.host = "fr-proxy.example".to_string();
            cached.country = Some("FR".to_string());
            probe.prepare_context(&mut cached);
            cached.http_forwarding = ProxyCapabilityStatus::Available;
            cached.https_tunnel = ProxyCapabilityStatus::Available;
            cached.destination_tls = ProxyCapabilityStatus::Available;
            cached.last_checked = Some(SystemTime::now());
            cached.latency_ms = Some(100);
            let mut unknown = record(ProxyRuntimeStatus::Unknown);
            unknown.host = "127.0.0.1".into();
            unknown.port = 9;
            unknown.country = Some("AD".into());
            unknown.latency_ms = Some(1);
            inventory.add_or_update(vec![unknown.clone()]).await;
            #[cfg(feature = "persistence")]
            let inventory = {
                let repository = Arc::new(TypedProxyRepository::new(memory_proxy_store().unwrap()));
                repository.save_many(&[cached.clone()]).await.unwrap();
                inventory.with_proxy_repository(repository)
            };
            #[cfg(not(feature = "persistence"))]
            inventory.add_or_update(vec![cached.clone()]).await;

            for countries in [
                vec!["ad".into(), " FR ".into(), "AD".into()],
                vec!["FR".into(), "AD".into()],
            ] {
                let selected = inventory
                    .select_any(&countries, require_https)
                    .await
                    .unwrap();
                assert_eq!(selected.authority(), cached.authority());
                assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
                let inner = inventory.inner.read().await;
                let unknown = inner.records.get(&unknown.authority()).unwrap();
                assert!(!unknown.probe_attempts.contains_key("tcp"));
                assert!(!unknown.probe_attempts.contains_key("http"));
                assert!(!unknown.probe_attempts.contains_key("https"));
            }
            let mut faster = cached.clone();
            faster.host = "ad-ready.example".into();
            faster.country = Some("AD".into());
            faster.latency_ms = Some(5);
            inventory.add_or_update(vec![faster.clone()]).await;
            for countries in [
                vec!["FR".into(), "AD".into()],
                vec!["AD".into(), "FR".into()],
            ] {
                assert_eq!(
                    inventory
                        .select_any(&countries, require_https)
                        .await
                        .unwrap()
                        .authority(),
                    faster.authority()
                );
                assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
            }
        }
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
        inventory
            .allow_provider_refresh_after_observed_failure(&countries)
            .await;
        assert!(inventory.select_any(&countries, true).await.is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), countries.len());
        {
            let mut inner = inventory.inner.write().await;
            for at in inner.country_last_refresh_attempts.values_mut() {
                *at = Instant::now() - Duration::from_secs(11);
            }
        }
        assert!(inventory.select_any(&countries, true).await.is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), countries.len() * 2);

        struct ProgressiveProvider(Arc<AtomicUsize>, Option<tokio::task::Id>);
        struct PendingLoad(Arc<AtomicUsize>);
        impl Drop for PendingLoad {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        #[async_trait]
        impl ProxyDataProvider for ProgressiveProvider {
            async fn load_proxies(&self, _: ProxyLoadRequest) -> Result<Vec<ProxyRecord>> {
                panic!("progressive selection must not wait for the list adapter");
            }
            async fn load_proxies_progressively(
                &self,
                request: ProxyLoadRequest,
                sink: Arc<dyn ProxyBatchSink>,
            ) -> Result<()> {
                assert_ne!(
                    tokio::task::try_id(),
                    self.1,
                    "provider refresh must not extend the caller's polling stack"
                );
                assert!(
                    ProxySearchContext::current().is_none(),
                    "provider requests must not inherit business acceptance state"
                );
                let _pending = PendingLoad(self.0.clone());
                let mut candidate = record(ProxyRuntimeStatus::Ok);
                candidate.country = Some(request.countries[0].clone());
                sink.publish(vec![candidate]).await?;
                futures::future::pending().await
            }
        }
        let cancelled = Arc::new(AtomicUsize::new(0));
        let inventory = ProxyInventory::new(
            InventoryConfig::default(),
            Some(Arc::new(ProgressiveProvider(
                cancelled.clone(),
                tokio::task::try_id(),
            ))),
            None,
        );
        let discovery_started = Instant::now();
        let selected = tokio::time::timeout(
            Duration::from_secs(1),
            inventory.select_any(&countries, true),
        )
        .await
        .expect("published candidates must precede provider completion")
        .unwrap();
        let first_candidate_elapsed = discovery_started.elapsed();
        assert!(countries.contains(selected.country.as_ref().unwrap()));
        tokio::time::timeout(Duration::from_secs(1), async {
            while cancelled.load(Ordering::SeqCst) == 0
                || inventory.provider_slots.available_permits() != 2
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("last-subscriber cancellation must stop owned provider tasks and release slots");
        eprintln!(
            "lot7 progressive discovery: first_candidate_us={}, cancelled_pending_loads={}",
            first_candidate_elapsed.as_micros(),
            cancelled.load(Ordering::SeqCst)
        );
        assert!(
            cancelled.load(Ordering::SeqCst) > 0,
            "pending producers must be dropped with the final subscriber"
        );

        let strict_inventory = ProxyInventory::new(
            InventoryConfig::default(),
            None,
            Some(Arc::new(ProxyProbe::new(crate::core::ProbeConfig {
                http_probe_url: None,
                https_probe_url: Some("https://probe.example/".into()),
                mode: ProbeMode::Strict,
                ..Default::default()
            }))),
        );
        #[cfg(feature = "persistence")]
        let strict_inventory = {
            struct YieldingRepository(Arc<dyn ProxyRepository>);
            #[async_trait]
            impl ProxyRepository for YieldingRepository {
                async fn get(&self, key: &crate::core::ProxyKey) -> AnyResult<Option<ProxyRecord>> {
                    self.0.get(key).await
                }
                async fn get_failure(
                    &self,
                    key: &crate::core::ProxyKey,
                ) -> AnyResult<Option<ProxyRecord>> {
                    self.0.get_failure(key).await
                }
                async fn evict(&self, record: &ProxyRecord) -> AnyResult<()> {
                    self.0.evict(record).await
                }
                async fn find_by_country(&self, country: &str) -> AnyResult<Vec<ProxyRecord>> {
                    self.0.find_by_country(country).await
                }
                async fn save_many(&self, records: &[ProxyRecord]) -> AnyResult<()> {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    self.0.save_many(records).await
                }
                async fn clear_superseded_destination_failures(
                    &self,
                    key: &crate::core::ProxyKey,
                    destination: &Destination,
                    accepted_at: SystemTime,
                ) -> AnyResult<()> {
                    self.0
                        .clear_superseded_destination_failures(key, destination, accepted_at)
                        .await
                }
                async fn delete(&self, key: &crate::core::ProxyKey) -> AnyResult<()> {
                    self.0.delete(key).await
                }
            }
            strict_inventory.with_proxy_repository(Arc::new(YieldingRepository(Arc::new(
                TypedProxyRepository::new(memory_proxy_store().unwrap()),
            ))))
        };
        let mut servers = Vec::new();
        for _ in 0..3 {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                loop {
                    let (mut stream, _) = listener.accept().await?;
                    let Ok(first) = stream.read_u8().await else {
                        continue;
                    };
                    let mut head = vec![first];
                    while !head.ends_with(b"\r\n\r\n") {
                        head.push(stream.read_u8().await?);
                        anyhow::ensure!(head.len() <= 8192, "fixture header limit");
                    }
                    stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;
                    stream.shutdown().await?;
                    return Ok::<_, anyhow::Error>(1);
                }
            });
            let mut candidate = record(ProxyRuntimeStatus::Unknown);
            candidate.host = address.ip().to_string();
            candidate.port = address.port();
            candidate.protocol = Some(ProxyProtocol::Http);
            candidate.declarations.clear();
            candidate.country = Some("FR".into());
            candidate.http_forwarding = ProxyCapabilityStatus::Unknown;
            candidate.https_tunnel = ProxyCapabilityStatus::Unknown;
            candidate.destination_tls = ProxyCapabilityStatus::Unknown;
            candidate.last_checked = None;
            candidate.tcp_reachable = None;
            candidate.probe_attempts.clear();
            strict_inventory.add_or_update(vec![candidate]).await;
            servers.push(server);
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            assert!(strict_inventory.select_any(&countries, true).await.is_err());
            for server in servers {
                assert_eq!(server.await.unwrap().unwrap(), 1);
            }
        })
        .await
        .expect("prechecks and validations must progress while persistence yields");
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
        let preferences = Arc::new(crate::core::TypedProxyPreferenceRepository::new(
            crate::core::proxy_preference_repository::memory_proxy_preference_store()?,
        ));
        let initial = ProxyInventory::new(InventoryConfig::default(), None, None)
            .with_proxy_repository(repository.clone())
            .with_preference_repository(preferences.clone());
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
        let redirect_destination = Destination::host_port("redirect.example", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        let redirect_token = crate::core::ProxySelectionToken::new(
            Arc::new(initial.clone()),
            fallback.authority(),
            redirect_destination.clone(),
        );
        redirect_token.record_redirect(302).await;

        initial
            .record_destination_failure(
                &blocked_authority,
                &destination,
                crate::core::ProxyDestinationFailureReason::Other,
            )
            .await;
        initial
            .record_validated_response(&blocked_authority, &destination)
            .await;
        let mut legacy = repository
            .get(&initial.inner.read().await.records[&blocked_authority].key())
            .await?
            .unwrap();
        assert!(
            legacy.destination_failures.is_empty(),
            "accepted responses must durably clear older ambiguous failures"
        );
        let accepted_at = preferences
            .get(&crate::core::ProxyDestinationPreference::destination_key(
                &destination,
            ))
            .await?
            .unwrap()
            .endpoints[0]
            .accepted_at;
        let old_failure = crate::core::ProxyDestinationFailure {
            scheme: "https".into(),
            host: "origin.example".into(),
            port: 443,
            reason: crate::core::ProxyDestinationFailureReason::Other,
            failure_count: 1,
            last_failed: accepted_at - Duration::from_secs(1),
            cooldown_until: Some(accepted_at + Duration::from_secs(86400)),
        };
        legacy.destination_failures.push(old_failure.clone());
        for (scheme, host, port) in [
            ("http", "origin.example", 443),
            ("https", "other.example", 443),
            ("https", "origin.example", 8443),
        ] {
            let mut unrelated = old_failure.clone();
            unrelated.scheme = scheme.into();
            unrelated.host = host.into();
            unrelated.port = port;
            legacy.destination_failures.push(unrelated);
        }
        repository.save_many(&[legacy.clone()]).await?;
        let provider = Arc::new(CountingProvider {
            records: Vec::new(),
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
        });
        let restore_started = Instant::now();
        let restarted =
            ProxyInventory::new(InventoryConfig::default(), Some(provider.clone()), None)
                .with_proxy_repository(repository.clone())
                .with_preference_repository(preferences.clone());
        assert_eq!(
            restarted
                .select_for_destination("BE", true, Some(&destination))
                .await?
                .authority(),
            blocked_authority
        );
        let restore_us = restore_started.elapsed().as_micros();
        let redirect_search = ProxySearchContext::default();
        let redirected = redirect_search
            .scope(restarted.select_for_destination("BE", true, Some(&redirect_destination)))
            .await?;
        assert_eq!(redirected.authority(), fallback.authority());
        assert_eq!(
            redirect_search.selection_provenance(&redirected.authority(), &redirect_destination),
            crate::core::ProxySelectionProvenance::RedirectDestination
        );
        let repaired = repository.get(&legacy.key()).await?.unwrap();
        assert_eq!(repaired.destination_failures.len(), 3);
        assert!(!has_active_destination_cooldown(
            &repaired,
            &destination,
            SystemTime::now()
        ));
        for (reason, last_failed) in [
            (
                crate::core::ProxyDestinationFailureReason::Other,
                accepted_at,
            ),
            (
                crate::core::ProxyDestinationFailureReason::Other,
                accepted_at + Duration::from_secs(1),
            ),
            (
                crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
                accepted_at - Duration::from_secs(1),
            ),
        ] {
            let mut protected = repaired.clone();
            let mut failure = old_failure.clone();
            failure.reason = reason;
            failure.last_failed = last_failed;
            protected.destination_failures.push(failure);
            store.put(&protected).await?;
            repository
                .clear_superseded_destination_failures(&legacy.key(), &destination, accepted_at)
                .await?;
            let protected_inventory = ProxyInventory::new(InventoryConfig::default(), None, None)
                .with_proxy_repository(repository.clone())
                .with_preference_repository(preferences.clone());
            assert_eq!(
                protected_inventory
                    .select_for_destination("BE", true, Some(&destination))
                    .await?
                    .authority(),
                fallback.authority(),
                "restored successes must preserve explicit, equal-date and newer exclusions"
            );
            assert_eq!(
                repository
                    .get(&legacy.key())
                    .await?
                    .unwrap()
                    .destination_failures
                    .len(),
                4
            );
        }
        store.put(&repaired).await?;
        let warm_started = Instant::now();
        for countries in [
            vec!["FR".into(), "BE".into()],
            vec!["BE".into(), "FR".into()],
        ] {
            assert_eq!(
                restarted
                    .select_any_for_destination(&countries, true, Some(&destination))
                    .await?
                    .authority(),
                blocked_authority
            );
        }
        assert!(restarted
            .select_for_destination("FR", true, Some(&destination))
            .await
            .is_err());
        assert_eq!(
            restarted
                .select_for_destination("BE", true, Some(&destination))
                .await?
                .authority(),
            blocked_authority
        );
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            1,
            "only the disallowed-country search should consult the provider"
        );
        eprintln!("lot7 restored preference: first_select_us={restore_us}, warm_and_country_checks_us={}, preferred_provider_calls=0", warm_started.elapsed().as_micros());
        initial.record_ok(&blocked_authority).await;

        initial
            .record_destination_failure(
                &blocked_authority,
                &destination,
                crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
            )
            .await;
        initial.record_ok(&blocked_authority).await;

        let restored = ProxyInventory::new(InventoryConfig::default(), None, None)
            .with_proxy_repository(repository)
            .with_preference_repository(preferences.clone());
        assert_eq!(
            restored
                .select_for_destination("BE", true, Some(&destination))
                .await?
                .authority(),
            fallback.authority()
        );
        let history = preferences
            .get(&crate::core::ProxyDestinationPreference::destination_key(
                &destination,
            ))
            .await?
            .unwrap();
        assert_eq!(history.endpoints[0].endpoint.host, "blocked-proxy.example");
        restored
            .record_validated_response(&fallback.authority(), &destination)
            .await;
        assert_eq!(
            preferences
                .get(&history.destination)
                .await?
                .unwrap()
                .endpoints
                .len(),
            2
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
    http_forward_available: usize,
    http_forward_unavailable: usize,
    http_forward_unknown: usize,
    https_tunnel_available: usize,
    https_tunnel_unavailable: usize,
    https_tunnel_unknown: usize,
    destination_tls_available: usize,
    destination_tls_unavailable: usize,
    destination_tls_unknown: usize,
    proxy_tls_valid: usize,
    proxy_tls_invalid: usize,
    proxy_tls_unknown: usize,
    eligible: usize,
}

fn selection_stats<'a>(
    records: impl IntoIterator<Item = &'a ProxyRecord>,
    country: &str,
    require_https: bool,
    destination: Option<&Destination>,
    now: SystemTime,
    probe_ttl: Duration,
    probe_mode: ProbeMode,
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

        match record.http_forwarding {
            ProxyCapabilityStatus::Available => stats.http_forward_available += 1,
            ProxyCapabilityStatus::Unavailable => stats.http_forward_unavailable += 1,
            ProxyCapabilityStatus::Unknown => stats.http_forward_unknown += 1,
        }
        match record.https_tunnel {
            ProxyCapabilityStatus::Available => stats.https_tunnel_available += 1,
            ProxyCapabilityStatus::Unavailable => stats.https_tunnel_unavailable += 1,
            ProxyCapabilityStatus::Unknown => stats.https_tunnel_unknown += 1,
        }
        match record.destination_tls {
            ProxyCapabilityStatus::Available => stats.destination_tls_available += 1,
            ProxyCapabilityStatus::Unavailable => stats.destination_tls_unavailable += 1,
            ProxyCapabilityStatus::Unknown => stats.destination_tls_unknown += 1,
        }
        match record.proxy_tls_certificate {
            ProxyCapabilityStatus::Available => stats.proxy_tls_valid += 1,
            ProxyCapabilityStatus::Unavailable => stats.proxy_tls_invalid += 1,
            ProxyCapabilityStatus::Unknown => stats.proxy_tls_unknown += 1,
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

        if require_https && !can_reach_https_destination(record, probe_mode) {
            stats.https_rejected += 1;
        }

        if is_eligible(record, country, require_https, destination, now, probe_mode) {
            stats.eligible += 1;
        }
    }

    stats
}

fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn recent_probe_attempt(record: &ProxyRecord, phase: &str, ttl: Duration) -> bool {
    record.probe_attempts.get(phase).is_some_and(|started| {
        epoch_millis().saturating_sub(*started) < ttl.as_millis().min(u64::MAX as u128) as u64
    })
}

fn complementary_trial_allowed(record: &ProxyRecord, mode: ProbeMode) -> bool {
    mode == ProbeMode::Relaxed
        && record.tcp_reachable == Some(true)
        && record.protocol.is_some()
        && record.authentication_required != Some(true)
        && (record.protocol != Some(ProxyProtocol::Https)
            || record.proxy_tls_certificate == ProxyCapabilityStatus::Available)
}

/// Returns whether the requested capability still needs a runtime measurement.
fn needs_capability_probe(record: &ProxyRecord, require_https: bool) -> bool {
    record.tcp_reachable == Some(false)
        || record.last_checked.is_none()
        || record.protocol.is_none()
        || matches!(record.status, ProxyRuntimeStatus::Unknown)
        || if require_https {
            record.https_tunnel == ProxyCapabilityStatus::Unknown
                || record.destination_tls == ProxyCapabilityStatus::Unknown
                || (record.protocol == Some(ProxyProtocol::Https)
                    && record.proxy_tls_certificate == ProxyCapabilityStatus::Unknown)
        } else {
            record.http_forwarding == ProxyCapabilityStatus::Unknown
        }
}

/// Includes untested records for lazy probing while preserving explicit
/// capability failures, authentication exclusions, and active cooldowns.
fn is_selection_candidate(
    record: &ProxyRecord,
    country: &str,
    require_https: bool,
    destination: Option<&Destination>,
    now: SystemTime,
    probe_mode: ProbeMode,
) -> bool {
    if record.proxy_tls_certificate == ProxyCapabilityStatus::Unavailable {
        return false;
    }
    let mut candidate = record.clone();
    if candidate.status == ProxyRuntimeStatus::Unknown
        || (candidate.status == ProxyRuntimeStatus::Ko
            && candidate.tcp_reachable == Some(false)
            && candidate.cooldown_until.is_some_and(|until| until <= now))
    {
        candidate.status = ProxyRuntimeStatus::Ok;
    }
    if candidate.http_forwarding == ProxyCapabilityStatus::Unknown {
        candidate.http_forwarding = ProxyCapabilityStatus::Available;
    }
    if candidate.https_tunnel == ProxyCapabilityStatus::Unknown {
        candidate.https_tunnel = ProxyCapabilityStatus::Available;
    }
    if candidate.destination_tls == ProxyCapabilityStatus::Unknown {
        candidate.destination_tls = ProxyCapabilityStatus::Available;
    }
    is_eligible(
        &candidate,
        country,
        require_https,
        destination,
        now,
        probe_mode,
    )
}

fn is_eligible(
    record: &ProxyRecord,
    country: &str,
    require_https: bool,
    destination: Option<&Destination>,
    now: SystemTime,
    probe_mode: ProbeMode,
) -> bool {
    if record.health_evidence.evicted_at.is_some()
        || record.proxy_tls_certificate == ProxyCapabilityStatus::Unavailable
    {
        return false;
    }
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

    if require_https {
        if !can_reach_https_destination(record, probe_mode) {
            return false;
        }
    } else if !can_reach_http_destination(record, probe_mode) {
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
    let loaded_count = records.len();
    let mut grouped = Vec::with_capacity(loaded_count);
    let mut grouped_indices = HashMap::new();
    for mut record in records {
        let authority = record.authority();
        if let Some(index) = grouped_indices.get(&authority).copied() {
            merge_provider_declaration(&mut grouped[index], record);
        } else {
            record.declarations = declarations_from_record(&record);
            grouped_indices.insert(authority, grouped.len());
            grouped.push(record);
        }
    }

    let mut prepared = Vec::with_capacity(grouped.len());
    let mut probe_indices = Vec::new();
    let duplicate_count = loaded_count.saturating_sub(grouped.len());
    let mut cached_count = 0;

    for mut record in grouped {
        let authority = record.authority();
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
    record.status = cached.status.clone();
    record.http_forwarding = cached.http_forwarding;
    record.https_tunnel = cached.https_tunnel;
    record.destination_tls = cached.destination_tls;
    record.proxy_tls_certificate = cached.proxy_tls_certificate;
    record.latency_ms = cached.latency_ms;
    record.failure_count = cached.failure_count;
    record.authentication_required = cached.authentication_required;
    record.destination_failures = cached.destination_failures.clone();
    record.last_checked = cached.last_checked;
    record.last_validated_at = cached.last_validated_at;
    record.health_evidence = cached.health_evidence.clone();
    record.observations = cached.observations.clone();
    record.cooldown_until = cached.cooldown_until;
}

fn merge_provider_declaration(target: &mut ProxyRecord, incoming: ProxyRecord) {
    for declaration in declarations_from_record(&incoming) {
        if !target.declarations.contains(&declaration) {
            target.declarations.push(declaration);
        }
    }

    target.supports_https = merge_https_hint(target.supports_https, incoming.supports_https);
    target.availability = preferred_availability(&target.availability, &incoming.availability);
    if target.country.is_none() {
        target.country = incoming.country;
    }
    target.protocol = preferred_declared_protocol(&target.declarations)
        .or(target.protocol.take())
        .or(incoming.protocol);
}

fn declarations_from_record(record: &ProxyRecord) -> Vec<ProxyDeclaration> {
    if record.declarations.is_empty() {
        vec![ProxyDeclaration {
            source: "legacy".to_string(),
            protocol: record.protocol.clone(),
            supports_https: record.supports_https,
        }]
    } else {
        record.declarations.clone()
    }
}

fn preferred_declared_protocol(declarations: &[ProxyDeclaration]) -> Option<ProxyProtocol> {
    PROXY_PROTOCOL_PRIORITY.iter().find_map(|preferred| {
        declarations
            .iter()
            .any(|declaration| declaration.protocol.as_ref() == Some(preferred))
            .then(|| preferred.clone())
    })
}

fn declared_protocol_count(record: &ProxyRecord) -> usize {
    let mut protocols = record
        .declarations
        .iter()
        .filter_map(|declaration| declaration.protocol.as_ref())
        .collect::<Vec<_>>();
    protocols.sort_by_key(|protocol| protocol.to_string());
    protocols.dedup();
    protocols.len()
}

fn runtime_protocol_is_undeclared(record: &ProxyRecord) -> bool {
    let Some(protocol) = &record.protocol else {
        return false;
    };
    !record.declarations.is_empty()
        && !record
            .declarations
            .iter()
            .any(|declaration| declaration.protocol.as_ref() == Some(protocol))
}

fn merge_https_hint(left: Option<bool>, right: Option<bool>) -> Option<bool> {
    match (left, right) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), _) | (_, Some(false)) => Some(false),
        (None, None) => None,
    }
}

fn preferred_availability(
    left: &crate::core::ProxyAvailabilityHint,
    right: &crate::core::ProxyAvailabilityHint,
) -> crate::core::ProxyAvailabilityHint {
    use crate::core::ProxyAvailabilityHint::{High, Low, Medium, Unknown};
    match (left, right) {
        (High, _) | (_, High) => High,
        (Medium, _) | (_, Medium) => Medium,
        (Low, _) | (_, Low) => Low,
        (Unknown, Unknown) => Unknown,
    }
}

fn proxy_validation_is_stale(record: &ProxyRecord, now: SystemTime) -> bool {
    let Some(last_validated_at) = record
        .health_evidence
        .last_positive_at
        .or(record.health_evidence.first_discovered_at)
    else {
        return false;
    };

    now.duration_since(last_validated_at)
        .map(|age| age > PROXY_CACHE_TTL)
        .unwrap_or(false)
}

/// Removes only ambiguous failures strictly older than acceptance by the same origin.
pub(super) fn clear_superseded_destination_failures(
    record: &mut ProxyRecord,
    destination: &Destination,
    accepted_at: SystemTime,
) -> bool {
    let previous_len = record.destination_failures.len();
    let scheme = destination_scheme(destination);
    let host = destination.host_for_protocol();
    record.destination_failures.retain(|failure| {
        !(failure.reason == crate::core::ProxyDestinationFailureReason::Other
            && failure.scheme == scheme
            && failure.host == host
            && failure.port == destination.port
            && failure.last_failed < accepted_at)
    });
    record.destination_failures.len() != previous_len
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
            && failure.cooldown_until.is_some_and(|cooldown| {
                let cooldown =
                    if failure.reason == crate::core::ProxyDestinationFailureReason::Other {
                        cooldown.min(
                            failure
                                .last_failed
                                .checked_add(PROXY_TRANSIENT_FAILURE_COOLDOWN)
                                .unwrap_or(cooldown),
                        )
                    } else {
                        cooldown
                    };
                cooldown > now
            })
    })
}

fn can_reach_https_destination(record: &ProxyRecord, probe_mode: ProbeMode) -> bool {
    if matches!(record.destination_tls, ProxyCapabilityStatus::Unavailable) {
        return false;
    }

    match probe_mode {
        ProbeMode::Strict => {
            matches!(record.https_tunnel, ProxyCapabilityStatus::Available)
                && matches!(record.destination_tls, ProxyCapabilityStatus::Available)
        }
        ProbeMode::Relaxed => match record.https_tunnel {
            ProxyCapabilityStatus::Available => {
                !matches!(record.destination_tls, ProxyCapabilityStatus::Unavailable)
            }
            ProxyCapabilityStatus::Unavailable => false,
            ProxyCapabilityStatus::Unknown => match record.protocol {
                Some(ProxyProtocol::Socks4 | ProxyProtocol::Socks4a | ProxyProtocol::Socks5) => {
                    true
                }
                Some(ProxyProtocol::Http | ProxyProtocol::Https) => {
                    record.supports_https == Some(true)
                }
                None => false,
            },
        },
    }
}

fn can_reach_http_destination(record: &ProxyRecord, probe_mode: ProbeMode) -> bool {
    match (probe_mode, record.http_forwarding) {
        (_, ProxyCapabilityStatus::Available) => true,
        (_, ProxyCapabilityStatus::Unavailable)
        | (ProbeMode::Strict, ProxyCapabilityStatus::Unknown) => false,
        (ProbeMode::Relaxed, ProxyCapabilityStatus::Unknown) => record.protocol.is_some(),
    }
}

fn destination_scheme(destination: &Destination) -> String {
    match destination.protocol {
        crate::core::ApplicationProtocol::Http => "http".to_string(),
        crate::core::ApplicationProtocol::Https => "https".to_string(),
        _ => "tcp".to_string(),
    }
}
