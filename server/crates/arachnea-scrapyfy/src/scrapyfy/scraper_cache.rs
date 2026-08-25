//! Server-side result cache for scraper query executions, backed by `foyer`.
//!
//! The cache is a foyer hybrid cache (bounded in-memory layer + bounded
//! on-disk store) keyed per source execution. Entries hold the parsed rows
//! together with the validator fragment that produced them, so a conditional
//! request validated against the stored fragment proves the cached rows are
//! still current. There is no functional TTL: freshness is driven by ETag
//! revalidation; [`SERVER_CACHE_ENTRY_TTL`] only acts as a safety bound.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use arachnea_core::application::get_application_path;
use arachnea_core::crypt::hash62_64;
use foyer::{BlockEngineConfig, DeviceBuilder, FsDeviceBuilder, HybridCache, HybridCacheBuilder};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

use super::{ScraperDataNode, ScraperOutputType};

/// Safety bound on the age of a server-cache entry. Freshness is normally
/// driven by ETag revalidation; entries older than this are dropped.
pub const SERVER_CACHE_ENTRY_TTL: Duration = Duration::from_secs(365 * 24 * 60 * 60);

/// Default maximum on-disk size of the server cache (100 MiB).
pub const DEFAULT_CACHE_MAX_DISK_BYTES: u64 = 100 * 1024 * 1024;

/// Default maximum in-memory size of the server cache (32 MiB).
pub const DEFAULT_CACHE_MAX_MEMORY_BYTES: u64 = 32 * 1024 * 1024;

/// Cache behavior of one query execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheType {
    /// No client ETag flow, no server cache.
    NoCache,
    /// Client-driven conditional ETag validation only (historical default).
    ClientCache,
    /// Server-side cache; no global ETag is exposed to the client.
    ServerCache,
    /// Server-side cache AND global ETag exposed to the client.
    FullCache,
}

impl Default for CacheType {
    /// Returns the historical behavior — client ETag flow, no server cache.
    fn default() -> Self {
        Self::ClientCache
    }
}

impl CacheType {
    /// Returns whether this execution reads/writes the server-side cache.
    pub fn uses_server_cache(&self) -> bool {
        matches!(self, Self::ServerCache | Self::FullCache)
    }

    /// Returns whether the rebuilt global ETag is exposed to the client.
    pub fn sends_client_etag(&self) -> bool {
        matches!(self, Self::ClientCache | Self::FullCache)
    }
}

/// How a single aggregation pass interacts with the server cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerCacheInteraction {
    /// No server-cache involvement (historical behavior).
    None,
    /// Read the cached entry when present and validate it against the remote
    /// through its stored fragment; refresh or invalidate the entry afterwards.
    Validate,
    /// Force an unconditional fetch and write the fresh outcome to the cache.
    ForceRefresh,
}


/// Configuration of the server-side result cache.
#[derive(Debug, Clone)]
pub struct ScraperCacheConfig {
    /// Maximum on-disk size of the cache store.
    pub max_disk_bytes: u64,
    /// Maximum in-memory size of the cache.
    pub max_memory_bytes: u64,
    /// Directory holding the on-disk cache store.
    pub cache_dir: PathBuf,
}

impl Default for ScraperCacheConfig {
    /// Returns the default configuration — 100 MiB disk / 32 MiB memory,
    /// rooted at `<application data directory>/cache`.
    fn default() -> Self {
        Self {
            max_disk_bytes: DEFAULT_CACHE_MAX_DISK_BYTES,
            max_memory_bytes: DEFAULT_CACHE_MAX_MEMORY_BYTES,
            cache_dir: PathBuf::from(get_application_path("data")).join("cache"),
        }
    }
}

/// Faithful serde representation of one [`ScraperDataNode`].
///
/// `ScraperDataNode`'s own serde impl projects nodes onto the typed JSON shape
/// consumed by API clients (a typed scalar becomes a bare JSON string or
/// number), which cannot be read back into a node. This DTO keeps the full
/// internal structure so cached rows survive a serialization roundtrip
/// unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerializedScraperDataNode {
    output_type: Option<ScraperOutputType>,
    values: Vec<String>,
    children: HashMap<String, SerializedScraperDataNode>,
    items: Vec<SerializedScraperDataNode>,
}

impl From<&ScraperDataNode> for SerializedScraperDataNode {
    fn from(node: &ScraperDataNode) -> Self {
        Self {
            output_type: node.output_type,
            values: node.values.clone(),
            children: node
                .children
                .iter()
                .map(|(name, child)| (name.clone(), Self::from(child)))
                .collect(),
            items: node.items.iter().map(Self::from).collect(),
        }
    }
}

impl From<SerializedScraperDataNode> for ScraperDataNode {
    fn from(raw: SerializedScraperDataNode) -> Self {
        Self {
            output_type: raw.output_type,
            values: raw.values,
            children: raw
                .children
                .into_iter()
                .map(|(name, child)| (name, ScraperDataNode::from(child)))
                .collect(),
            items: raw.items.into_iter().map(ScraperDataNode::from).collect(),
        }
    }
}

/// One cached source execution: parsed rows plus the validator fragment that
/// produced them and the insertion timestamp backing [`SERVER_CACHE_ENTRY_TTL`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedSourceEntry {
    /// Parsed rows of the source execution.
    pub rows: Vec<HashMap<String, ScraperDataNode>>,
    /// Validator fragment that produced these rows.
    pub etag_fragment: String,
    stored_at_unix: u64,
}

impl CachedSourceEntry {
    /// Returns whether this entry outlived [`SERVER_CACHE_ENTRY_TTL`].
    fn is_expired(&self, now_unix: u64) -> bool {
        now_unix.saturating_sub(self.stored_at_unix) >= SERVER_CACHE_ENTRY_TTL.as_secs()
    }

    /// Rebuilds an entry from its wire representation.
    fn from_wire(wire: CachedSourceEntryWire) -> Self {
        Self {
            rows: wire
                .rows
                .into_iter()
                .map(|row| {
                    row.into_iter()
                        .map(|(name, node)| (name, ScraperDataNode::from(node)))
                        .collect()
                })
                .collect(),
            etag_fragment: wire.etag_fragment,
            stored_at_unix: wire.stored_at_unix,
        }
    }
}

/// Wire representation of [`CachedSourceEntry`] used for foyer storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedSourceEntryWire {
    rows: Vec<HashMap<String, SerializedScraperDataNode>>,
    etag_fragment: String,
    stored_at_unix: u64,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since_epoch| since_epoch.as_secs())
        .unwrap_or_default()
}

/// Builds the deterministic server-cache key of one source execution.
///
/// The key covers every input shaping the output of the execution: group,
/// source, query, merged parameters, media-type filter, root fields filters
/// and the source YAML hash. Source selection (`scrapper_list`) is
/// intentionally not part of the key: it only selects which sources run, and
/// each source already appears in its own key.
///
/// # Arguments
///
/// * `group_name` - Query group name.
/// * `source_name` - Source (collection) name within the group.
/// * `query_name` - Name of the executed query.
/// * `params` - Merged runtime parameter values.
/// * `query_media_type_filter` - Media-type filter list or None.
/// * `fields_filters` - Root fields filter map or None.
/// * `yaml_hash` - Base62 hash of the source YAML document.
#[allow(clippy::too_many_arguments)]
pub fn server_cache_key(
    group_name: &str,
    source_name: &str,
    query_name: &str,
    params: &HashMap<String, String>,
    query_media_type_filter: Option<&Vec<String>>,
    fields_filters: Option<&HashMap<String, Vec<String>>>,
    yaml_hash: &str,
) -> String {
    const SEPARATOR: &str = "\u{1}";
    const PAIR_SEPARATOR: &str = "\u{2}";

    let mut param_pairs: Vec<String> = params
        .iter()
        .map(|(key, value)| format!("{key}{PAIR_SEPARATOR}{value}"))
        .collect();
    param_pairs.sort();

    let mut media_types: Vec<&str> = query_media_type_filter
        .map(|values| values.iter().map(String::as_str).collect())
        .unwrap_or_default();
    media_types.sort();
    media_types.dedup();

    let mut field_filters: Vec<String> = fields_filters
        .map(|filters| {
            filters
                .iter()
                .map(|(field, values)| {
                    let mut sorted: Vec<&str> = values.iter().map(String::as_str).collect();
                    sorted.sort();
                    sorted.dedup();
                    format!("{field}{PAIR_SEPARATOR}{}", sorted.join(SEPARATOR))
                })
                .collect()
        })
        .unwrap_or_default();
    field_filters.sort();

    let composite = [
        group_name,
        source_name,
        query_name,
        &param_pairs.join(SEPARATOR),
        &media_types.join(SEPARATOR),
        &field_filters.join(SEPARATOR),
        yaml_hash,
    ]
    .join("\u{0}");

    hash62_64(composite.as_bytes())
}

async fn build_hybrid_cache(config: &ScraperCacheConfig) -> Result<ServerHybridCache> {
    std::fs::create_dir_all(&config.cache_dir)?;
    let device = FsDeviceBuilder::new(&config.cache_dir)
        .with_capacity(config.max_disk_bytes as usize)
        .build()?;
    let hybrid: ServerHybridCache = HybridCacheBuilder::new()
        .memory(config.max_memory_bytes as usize)
        // Entries are weighted by their serialized byte size so both bounds
        // reflect actual data volume instead of counting entries.
        .with_weighter(|_key: &String, value: &Vec<u8>| value.len().max(1))
        .storage()
        .with_engine_config(BlockEngineConfig::new(device))
        .build()
        .await?;
    Ok(hybrid)
}

/// Concrete hybrid cache type — keys are canonical cache keys, values are
/// JSON-serialized [`CachedSourceEntry`] bytes.
type ServerHybridCache = HybridCache<String, Vec<u8>>;

/// Shared handle to the lazily-built foyer hybrid cache.
pub type ScraperServerCacheHandle = Arc<ServerHybridCache>;

/// Lazily-initialized server-side cache wrapping the foyer hybrid cache.
///
/// All operations are best-effort: engine creation or I/O failures degrade to
/// a cache miss (with a warning) and never fail the query execution itself.
pub struct ScraperServerCache {
    config: ScraperCacheConfig,
    hybrid: OnceCell<ScraperServerCacheHandle>,
}

impl ScraperServerCache {
    /// Creates an uninitialized cache with the given configuration. The foyer
    /// engine is built on first use.
    pub fn new(config: ScraperCacheConfig) -> Self {
        Self {
            config,
            hybrid: OnceCell::new(),
        }
    }

    /// Returns the cache configuration.
    pub fn config(&self) -> &ScraperCacheConfig {
        &self.config
    }

    /// Returns the shared hybrid cache handle, building it on first use.
    ///
    /// # Errors
    ///
    /// Returns an error when the on-disk cache store cannot be created.
    pub async fn handle(&self) -> Result<ScraperServerCacheHandle> {
        self.hybrid
            .get_or_try_init(|| async {
                let hybrid = build_hybrid_cache(&self.config).await?;
                Ok(Arc::new(hybrid))
            })
            .await
            .cloned()
    }

    async fn write_entry(&self, key: &str, entry: &CachedSourceEntryWire) {
        let Ok(handle) = self.handle().await else {
            tracing::warn!(
                cache_dir = %self.config.cache_dir.display(),
                "server cache unavailable; skipping cache write"
            );
            return;
        };
        let Ok(bytes) = serde_json::to_vec(entry) else {
            tracing::warn!("server cache entry serialization failed; skipping cache write");
            return;
        };
        // Foyer's synchronous insert is admission-controlled and infallible:
        // oversized entries are silently dropped by the cache itself.
        handle.insert(key.to_string(), bytes);
    }

    /// Stores the parsed rows of one source execution with its validator fragment.
    pub async fn store_entry(
        &self,
        key: &str,
        rows: &[HashMap<String, ScraperDataNode>],
        etag_fragment: &str,
    ) {
        // Build the wire representation directly from the borrowed rows to
        // avoid deep-copying the whole payload twice.
        let entry = CachedSourceEntryWire {
            rows: rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|(name, node)| (name.clone(), SerializedScraperDataNode::from(node)))
                        .collect()
                })
                .collect(),
            etag_fragment: etag_fragment.to_string(),
            stored_at_unix: now_unix(),
        };
        tracing::debug!(
            key = %key,
            row_count = rows.len(),
            "source execution stored into server cache"
        );
        self.write_entry(key, &entry).await;
    }

    /// Loads the cached entry of one source execution, or `None` when absent,
    /// expired, corrupted, or when the cache engine is unavailable.
    pub async fn load_entry(&self, key: &str) -> Option<CachedSourceEntry> {
        let handle = self.handle().await.ok()?;
        let bytes = match handle.get(key).await {
            Ok(Some(entry)) => entry.value().clone(),
            Ok(None) => return None,
            Err(error) => {
                tracing::warn!(error = %error, "server cache read failed; treating as miss");
                return None;
            }
        };
        match serde_json::from_slice::<CachedSourceEntryWire>(&bytes) {
            Ok(wire) => {
                let entry = CachedSourceEntry::from_wire(wire);
                if entry.is_expired(now_unix()) {
                    self.invalidate(key).await;
                    None
                } else {
                    Some(entry)
                }
            }
            Err(_) => {
                // Unreadable entry: drop it so a fresh fetch replaces it.
                self.invalidate(key).await;
                None
            }
        }
    }

    /// Removes the cached entry of one source execution.
    pub async fn invalidate(&self, key: &str) {
        let Ok(handle) = self.handle().await else {
            return;
        };
        handle.remove(key);
    }
}

