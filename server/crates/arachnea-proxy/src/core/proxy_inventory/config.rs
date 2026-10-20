//! Inventory policy, defaults and cooldown constants.

use std::time::Duration;

use crate::{
    PROXY_INVENTORY_DEFAULT_EXPLORATION_BEFORE_REFRESH,
    PROXY_INVENTORY_DEFAULT_FAILURES_BEFORE_REFRESH, PROXY_INVENTORY_DEFAULT_KO_COOLDOWN,
    PROXY_INVENTORY_DEFAULT_MAX_DESTINATION_FAILURES_BEFORE_KO,
    PROXY_INVENTORY_DEFAULT_PROBE_BATCH_SIZE, PROXY_INVENTORY_DEFAULT_PROBE_TTL,
    PROXY_INVENTORY_DEFAULT_PROVIDER_MIN_REFRESH_INTERVAL,
    PROXY_INVENTORY_DEFAULT_PROVIDER_REFRESH_COOLDOWN, PROXY_INVENTORY_DEFAULT_RESERVE_SIZE,
    PROXY_INVENTORY_DEFAULT_RESERVE_TIMEOUT,
};

pub(crate) use crate::PROXY_AFFINITY_TTL;

/// Default duration for excluding a proxy from one exact destination after a
/// destination-scoped failure such as an HTTP 403 origin rejection.
///
/// Maximum suspension after an ambiguous transport failure for one destination.
///
/// Both cooldown constants are defined in the crate root and re-exported here
/// for path compatibility.
pub use crate::{PROXY_DESTINATION_FAILURE_COOLDOWN, PROXY_TRANSIENT_FAILURE_COOLDOWN};

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
    /// Fresh technical fallback endpoints to prepare in addition to the accepted proxy.
    /// Zero disables preparation after acceptance; at most two are prepared.
    pub reserve_size: usize,
    /// Independent deadline for preparation after business acceptance.
    pub reserve_timeout: Duration,
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
            probe_ttl: PROXY_INVENTORY_DEFAULT_PROBE_TTL,
            ko_cooldown: PROXY_INVENTORY_DEFAULT_KO_COOLDOWN,
            destination_failure_cooldown: PROXY_DESTINATION_FAILURE_COOLDOWN,
            max_destination_failures_before_ko:
                PROXY_INVENTORY_DEFAULT_MAX_DESTINATION_FAILURES_BEFORE_KO,
            probe_batch_size: PROXY_INVENTORY_DEFAULT_PROBE_BATCH_SIZE,
            reserve_size: PROXY_INVENTORY_DEFAULT_RESERVE_SIZE,
            reserve_timeout: PROXY_INVENTORY_DEFAULT_RESERVE_TIMEOUT,
            provider_refresh_cooldown: PROXY_INVENTORY_DEFAULT_PROVIDER_REFRESH_COOLDOWN,
            provider_min_refresh_interval: PROXY_INVENTORY_DEFAULT_PROVIDER_MIN_REFRESH_INTERVAL,
            exploration_before_refresh: PROXY_INVENTORY_DEFAULT_EXPLORATION_BEFORE_REFRESH,
            failures_before_refresh: PROXY_INVENTORY_DEFAULT_FAILURES_BEFORE_REFRESH,
        }
    }
}
