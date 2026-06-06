use crate::core::{
    Answer, CacheConfig, DnsClass, DnsError, DnssecState, EcsPolicy, QueryRequest, RecordType,
};
use hickory_proto::rr::RecordType as HickoryRecordType;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

/// Cache status for a returned answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheState {
    Disabled,
    Miss,
    Hit,
    Stale,
}

/// Runtime counters maintained by a core instance.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CoreStats {
    /// Total number of resolution calls.
    pub requests: u64,
    /// Answers served from fresh or stale cache.
    pub cache_hits: u64,
    /// Cache misses.
    pub cache_misses: u64,
    /// Upstream errors observed.
    pub upstream_errors: u64,
    /// Negative answers served from cache.
    pub negative_cache_hits: u64,
    /// Negative answers synthesized from cached DNSSEC denial proofs.
    pub aggressive_negative_cache_hits: u64,
}

/// Per-core in-memory TTL cache.
#[derive(Debug, Clone)]
pub struct Cache {
    entries: Arc<RwLock<HashMap<CacheKey, CacheEntry>>>,
    negative_entries: Arc<RwLock<HashMap<CacheKey, NegativeCacheEntry>>>,
    dnssec_negative_proofs: Arc<RwLock<Vec<DnssecNegativeProof>>>,
    config: CacheConfig,
}

impl Cache {
    /// Creates a cache from configuration.
    ///
    /// # Parameters
    ///
    /// - `config`: Cache configuration to use.
    ///
    /// # Returns
    ///
    /// Empty cache instance.
    pub(crate) fn new(config: CacheConfig) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            negative_entries: Arc::new(RwLock::new(HashMap::new())),
            dnssec_negative_proofs: Arc::new(RwLock::new(Vec::new())),
            config,
        }
    }

    /// Reads a fresh or stale entry for the key.
    ///
    /// # Parameters
    ///
    /// - `key`: Cache key to look up.
    ///
    /// # Returns
    ///
    /// Cached answer and cache state when present.
    pub(crate) async fn get(&self, key: &CacheKey) -> Option<(Answer, CacheState)> {
        if !self.config.enabled {
            return None;
        }

        let entries = self.entries.read().await;
        let entry = entries.get(key)?;
        if Instant::now() <= entry.expires_at {
            let mut answer = entry.answer.clone();
            answer.metadata.cache = CacheState::Hit;
            return Some((answer, CacheState::Hit));
        }

        if self.config.serve_stale && Instant::now() <= entry.stale_until {
            let mut answer = entry.answer.clone();
            answer.metadata.cache = CacheState::Stale;
            for record in &mut answer.records {
                record.ttl = self.config.stale_response_ttl_seconds;
            }
            return Some((answer, CacheState::Stale));
        }

        None
    }

    /// Reads a cached negative response when negative caching is enabled.
    ///
    /// # Parameters
    ///
    /// - `key`: Cache key to look up.
    ///
    /// # Returns
    ///
    /// Cached negative DNS error when present and fresh.
    pub(crate) async fn get_negative(&self, key: &CacheKey) -> Option<DnsError> {
        if !self.config.enabled || !self.config.negative_cache {
            return None;
        }

        let entries = self.negative_entries.read().await;
        let entry = entries.get(key)?;
        (Instant::now() <= entry.expires_at).then(|| entry.error.to_error())
    }

    /// Synthesizes a negative answer from cached NSEC denial proofs when safe.
    ///
    /// # Parameters
    ///
    /// - `query`: Query to test against cached denial proofs.
    ///
    /// # Returns
    ///
    /// Synthesized negative DNS error when a proof covers `query`.
    pub(crate) async fn get_aggressive_negative(&self, query: &QueryRequest) -> Option<DnsError> {
        if !self.config.enabled
            || !self.config.negative_cache
            || !self.config.aggressive_dnssec_negative_cache
        {
            return None;
        }

        let proofs = self.dnssec_negative_proofs.read().await;
        proofs
            .iter()
            .filter(|proof| Instant::now() <= proof.expires_at)
            .find_map(|proof| proof.error_for(query))
    }

    /// Inserts a positive answer when caching is enabled.
    ///
    /// # Parameters
    ///
    /// - `key`: Cache key for the answer.
    /// - `answer`: Positive DNS answer to store.
    pub(crate) async fn put(&self, key: CacheKey, answer: Answer) {
        if !self.config.enabled || answer.records.is_empty() {
            return;
        }

        let ttl = answer.metadata.ttl.unwrap_or(self.config.max_ttl_seconds);
        let ttl = ttl.clamp(self.config.min_ttl_seconds, self.config.max_ttl_seconds);
        let now = Instant::now();
        let entry = CacheEntry {
            answer,
            expires_at: now + Duration::from_secs(ttl as u64),
            stale_until: now + Duration::from_secs((ttl + self.config.max_stale_seconds) as u64),
        };

        let mut entries = self.entries.write().await;
        if entries.len() >= self.config.max_entries {
            if let Some(first) = entries.keys().next().cloned() {
                entries.remove(&first);
            }
        }
        entries.insert(key, entry);
    }

    /// Inserts a bounded negative cache entry for NXDOMAIN or NODATA.
    ///
    /// # Parameters
    ///
    /// - `key`: Cache key for the negative response.
    /// - `error`: DNS error to store when cacheable.
    pub(crate) async fn put_negative(&self, key: CacheKey, error: DnsError) {
        if !self.config.enabled || !self.config.negative_cache {
            return;
        }

        if !matches!(error, DnsError::Nxdomain(_) | DnsError::NoData { .. }) {
            return;
        }

        let ttl = self.config.max_ttl_seconds.min(300).max(1);
        let Some(error) = NegativeCachedError::from_error(error) else {
            return;
        };

        let entry = NegativeCacheEntry {
            error,
            expires_at: Instant::now() + Duration::from_secs(ttl as u64),
        };

        let mut entries = self.negative_entries.write().await;
        if entries.len() >= self.config.max_entries {
            if let Some(first) = entries.keys().next().cloned() {
                entries.remove(&first);
            }
        }
        entries.insert(key, entry);
    }

    /// Stores DNSSEC NSEC proofs that can later synthesize NXDOMAIN or NODATA.
    /// Inserts DNSSEC NSEC proofs for aggressive negative caching.
    ///
    /// # Parameters
    ///
    /// - `proofs`: NSEC proofs extracted from recursive responses.
    #[cfg_attr(not(feature = "recursive"), allow(dead_code))]
    pub(crate) async fn put_nsec_proofs(&self, proofs: Vec<NsecProof>) {
        if !self.config.enabled
            || !self.config.negative_cache
            || !self.config.aggressive_dnssec_negative_cache
            || proofs.is_empty()
        {
            return;
        }

        let ttl = self.config.max_ttl_seconds.min(300).max(1);
        let expires_at = Instant::now() + Duration::from_secs(ttl as u64);
        let mut cached = self.dnssec_negative_proofs.write().await;
        cached.retain(|proof| Instant::now() <= proof.expires_at);
        for proof in proofs {
            if cached.len() >= self.config.max_entries {
                cached.remove(0);
            }
            cached.push(DnssecNegativeProof { proof, expires_at });
        }
    }

    /// Clears all cache entries.
    pub async fn clear(&self) {
        self.entries.write().await.clear();
        self.negative_entries.write().await.clear();
        self.dnssec_negative_proofs.write().await.clear();
    }
}

#[derive(Debug, Clone)]
/// Positive cache entry with its expiration timestamp.
struct CacheEntry {
    answer: Answer,
    expires_at: Instant,
    stale_until: Instant,
}

#[derive(Debug, Clone)]
/// Negative cache entry with its expiration timestamp.
struct NegativeCacheEntry {
    error: NegativeCachedError,
    expires_at: Instant,
}

#[derive(Debug, Clone)]
/// DNSSEC negative proof used for aggressive negative caching.
struct DnssecNegativeProof {
    proof: NsecProof,
    expires_at: Instant,
}

/// A cached NSEC denial proof. NSEC3 proofs are intentionally collected by the
/// recursor but not synthesized here until a full validator is wired in.
#[derive(Debug, Clone)]
pub(crate) struct NsecProof {
    pub(crate) owner: String,
    pub(crate) next_owner: String,
    pub(crate) types: Vec<HickoryRecordType>,
}

impl DnssecNegativeProof {
    /// Builds a DNS error when this proof covers a query.
    ///
    /// # Parameters
    ///
    /// - `query`: Query to test against the negative proof.
    ///
    /// # Returns
    ///
    /// Matching DNS error when the proof covers `query`.
    fn error_for(&self, query: &QueryRequest) -> Option<DnsError> {
        if self.proof.owner == query.name
            && !self.proof.types.contains(&query.record_type.to_hickory())
        {
            return Some(DnsError::NoData {
                name: query.name.clone(),
                record_type: query.record_type,
            });
        }

        covers_name(&query.name, &self.proof.owner, &self.proof.next_owner)
            .then(|| DnsError::Nxdomain(query.name.clone()))
    }
}

/// Returns whether an NSEC owner range covers a name.
///
/// # Parameters
///
/// - `name`: Candidate normalized DNS name.
/// - `owner`: NSEC owner name.
/// - `next_owner`: NSEC next owner name.
///
/// # Returns
///
/// `true` when `name` falls inside the NSEC range.
fn covers_name(name: &str, owner: &str, next_owner: &str) -> bool {
    if owner < next_owner {
        owner < name && name < next_owner
    } else {
        owner < name || name < next_owner
    }
}

#[derive(Debug, Clone)]
/// Negative DNS error stored in the cache.
enum NegativeCachedError {
    Nxdomain(String),
    NoData {
        name: String,
        record_type: RecordType,
    },
}

impl NegativeCachedError {
    /// Converts a DNS error into a cacheable negative error.
    ///
    /// # Parameters
    ///
    /// - `error`: DNS error returned by resolution.
    ///
    /// # Returns
    ///
    /// Cacheable negative error, if the error kind is cacheable.
    fn from_error(error: DnsError) -> Option<Self> {
        match error {
            DnsError::Nxdomain(name) => Some(Self::Nxdomain(name)),
            DnsError::NoData { name, record_type } => Some(Self::NoData { name, record_type }),
            _ => None,
        }
    }

    /// Converts the cached negative error back to a DNS error.
    ///
    /// # Returns
    ///
    /// DNS error equivalent to the cached value.
    fn to_error(&self) -> DnsError {
        match self {
            Self::Nxdomain(name) => DnsError::Nxdomain(name.clone()),
            Self::NoData { name, record_type } => DnsError::NoData {
                name: name.clone(),
                record_type: *record_type,
            },
        }
    }
}

/// Cache key includes protocol and privacy/security policy dimensions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CacheKey {
    pub(crate) name: String,
    pub(crate) record_type: RecordType,
    pub(crate) class: DnsClass,
    pub(crate) ecs: EcsPolicy,
    pub(crate) dnssec: DnssecState,
}

impl CacheKey {
    /// Builds a key for a query and effective policy settings.
    ///
    /// # Parameters
    ///
    /// - `query`: Query represented by the cache key.
    /// - `ecs`: Effective ECS policy dimension.
    /// - `dnssec`: Effective DNSSEC policy dimension.
    ///
    /// # Returns
    ///
    /// Cache key including query and policy dimensions.
    pub(crate) fn new(query: &QueryRequest, ecs: EcsPolicy, dnssec: DnssecState) -> Self {
        Self {
            name: query.name.clone(),
            record_type: query.record_type,
            class: query.class,
            ecs,
            dnssec,
        }
    }
}
