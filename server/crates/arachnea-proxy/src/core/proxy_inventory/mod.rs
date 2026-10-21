//! Dynamic proxy inventory with shared discovery, health and destination history.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{watch, Mutex, RwLock, Semaphore};

#[cfg(feature = "persistence")]
use crate::core::ProxyRepository;
use crate::core::{IpCountryResolver, ProbeMode, ProxyDataProvider, ProxyProbe, ProxyRecord};
use probing::SharedProbe;

mod config;
mod discovery;
mod eligibility;
mod observations;
#[cfg(feature = "persistence")]
mod persistence;
mod preferences;
pub(crate) mod preparation;
mod probing;
mod records;
mod selection;

pub use config::{
    CoexistencePolicy, InventoryConfig, PROXY_DESTINATION_FAILURE_COOLDOWN,
    PROXY_TRANSIENT_FAILURE_COOLDOWN,
};
pub(super) use observations::clear_superseded_destination_failures;

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

struct ProxyAffinityBinding {
    authority: String,
    expires_at: Instant,
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
    preparations: Arc<preparation::PreparationRuntime>,
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
            preparations: Arc::new(preparation::PreparationRuntime::default()),
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
}

#[cfg(test)]
#[path = "../../../tests/core/proxy_inventory/mod.rs"]
mod tests;
