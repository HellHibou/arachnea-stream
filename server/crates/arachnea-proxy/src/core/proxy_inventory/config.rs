//! Inventory policy, defaults and cooldown constants.

use std::time::Duration;

pub(super) const PROXY_AFFINITY_TTL: Duration = Duration::from_secs(15 * 60);

/// Default duration for excluding a proxy from one exact destination after a
/// destination-scoped failure such as an HTTP 403 origin rejection.
pub const PROXY_DESTINATION_FAILURE_COOLDOWN: Duration = Duration::from_secs(24 * 60 * 60);

/// Maximum suspension after an ambiguous transport failure for one destination.
pub const PROXY_TRANSIENT_FAILURE_COOLDOWN: Duration = Duration::from_secs(30);

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
