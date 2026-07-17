use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::core::Result;

/// A single IP-to-country mapping.
///
/// This record is the canonical representation used for loading, storing,
/// and resolving IP geolocation data. The country code is trusted as-is
/// and is not re-verified.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IpCountryRecord {
    /// IP address (IPv4 or IPv6).
    pub ip: IpAddr,
    /// ISO 3166-1 alpha-2 country code (uppercase).
    pub country: String,
    /// Optional source identifier for diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Provider that loads IP-to-country mapping data from an external source.
#[async_trait]
pub trait IpCountryDataProvider: Send + Sync {
    /// Load all IP-to-country records.
    ///
    /// # Returns
    ///
    /// A list of IP-country records loaded from the source.
    async fn load_ip_countries(&self) -> Result<Vec<IpCountryRecord>>;

    /// Resolves a single IP address to a country code.
    ///
    /// The default implementation returns `None` and is suitable for providers
    /// that only support bulk refresh. Providers that can resolve individual
    /// IPs should override this method.
    ///
    /// # Parameters
    ///
    /// - `ip`: IP address to resolve.
    ///
    /// # Returns
    ///
    /// An optional country code when the IP could be resolved.
    async fn resolve_ip_country(&self, _ip: &std::net::IpAddr) -> Result<Option<String>> {
        Ok(None)
    }
}

/// Persistent store for IP-to-country records.
#[async_trait]
pub trait IpCountryStore: Send + Sync {
    /// Load all IP-country records from the store.
    ///
    /// Returns an empty vector when the backing file or resource does not exist.
    async fn load_ip_countries(&self) -> Result<Vec<IpCountryRecord>>;

    /// Save a complete set of IP-country records, replacing any existing data.
    async fn save_ip_countries(&self, entries: &[IpCountryRecord]) -> Result<()>;
}

/// Serialisation codec for IP-to-country records.
pub trait IpCountrySerdeCodec: Send + Sync {
    /// Human-readable format name for diagnostics (e.g. `"json"`, `"yaml"`).
    fn format_name(&self) -> &'static str;

    /// Serialise IP-country records into bytes.
    fn serialize_ip_countries(&self, entries: &[IpCountryRecord]) -> Result<Vec<u8>>;

    /// Deserialise IP-country records from bytes.
    fn deserialize_ip_countries(&self, bytes: &[u8]) -> Result<Vec<IpCountryRecord>>;
}

/// File-backed IP-country store that writes atomically through a temporary file
/// and rename.
///
/// # Atomicity contract
///
/// 1. Data is first written to `path.tmp`.
/// 2. The temporary file is synchronised (fsync).
/// 3. The temporary file is renamed over the target path.
///
/// If the process crashes before the rename, the original file (if any) is
/// preserved. A `.tmp` file left behind by a previous crash is silently
/// ignored on the next load — only the non-tmp path is read.
pub struct IpCountrySerdeStore {
    path: PathBuf,
    codec: Arc<dyn IpCountrySerdeCodec>,
}

impl IpCountrySerdeStore {
    /// Creates a new file-backed IP-country store.
    ///
    /// # Parameters
    ///
    /// - `path`: Filesystem path for the IP-country data file.
    /// - `codec`: Serialisation codec (e.g. `JsonIpCountryCodec`).
    pub fn new(path: impl Into<PathBuf>, codec: Arc<dyn IpCountrySerdeCodec>) -> Self {
        Self {
            path: path.into(),
            codec,
        }
    }

    /// Returns a reference to the store path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns a reference to the configured codec.
    pub fn codec(&self) -> &Arc<dyn IpCountrySerdeCodec> {
        &self.codec
    }

    /// Builds the temporary file path used during atomic writes.
    fn tmp_path(&self) -> PathBuf {
        let mut tmp = self.path.to_path_buf().into_os_string();
        tmp.push(".tmp");
        PathBuf::from(tmp)
    }
}

#[async_trait]
impl IpCountryStore for IpCountrySerdeStore {
    async fn load_ip_countries(&self) -> Result<Vec<IpCountryRecord>> {
        let data = match tokio::fs::read(&self.path).await {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        self.codec.deserialize_ip_countries(&data)
    }

    async fn save_ip_countries(&self, entries: &[IpCountryRecord]) -> Result<()> {
        let bytes = self.codec.serialize_ip_countries(entries)?;

        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let tmp = self.tmp_path();
        tokio::fs::write(&tmp, &bytes).await?;

        let file = tokio::fs::File::open(&tmp).await?;
        file.sync_all().await?;
        drop(file);

        tokio::fs::rename(&tmp, &self.path).await?;

        tracing::debug!(
            path = %self.path.display(),
            count = %entries.len(),
            format = %self.codec.format_name(),
            "ip-country store saved"
        );

        Ok(())
    }
}

// ── JSON codec ─────────────────────────────────────────────────────

/// Default JSON codec for IP-to-country records.
///
/// Uses `serde_json` with pretty-printed output for readability and manual
/// inspection.
pub struct JsonIpCountryCodec;

impl JsonIpCountryCodec {
    /// Creates a new JSON codec.
    pub fn new() -> Self {
        Self
    }
}

impl Default for JsonIpCountryCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl IpCountrySerdeCodec for JsonIpCountryCodec {
    fn format_name(&self) -> &'static str {
        "json"
    }

    fn serialize_ip_countries(&self, entries: &[IpCountryRecord]) -> Result<Vec<u8>> {
        serde_json::to_vec_pretty(entries)
            .map_err(|e| crate::core::ProxyError::Config(format!("json serialisation failed: {e}")))
    }

    fn deserialize_ip_countries(&self, bytes: &[u8]) -> Result<Vec<IpCountryRecord>> {
        serde_json::from_slice(bytes).map_err(|e| {
            crate::core::ProxyError::Config(format!("json deserialisation failed: {e}"))
        })
    }
}

// ── IP-country resolver ──────────────────────────────────────────────

/// Configuration for the IP-country resolver.
#[derive(Clone, Debug)]
pub struct IpCountryResolverConfig {
    /// Maximum time to wait for a single IP resolution via the provider.
    pub resolve_timeout: Duration,
    /// Whether to persist newly resolved IP-country mappings to the store.
    pub persist_on_resolve: bool,
}

impl Default for IpCountryResolverConfig {
    fn default() -> Self {
        Self {
            resolve_timeout: Duration::from_secs(5),
            persist_on_resolve: true,
        }
    }
}

/// Resolves IP addresses to country codes using a local store and an optional
/// provider for on-demand resolution.
///
/// The resolver maintains an in-memory map loaded from the store at
/// construction. When a requested IP is not found in the map and a provider is
/// configured, it calls the provider's `resolve_ip_country()` with a bounded
/// timeout. If the provider returns a country, the mapping is added to the
/// in-memory map and optionally persisted.
///
/// This is the synchronous bounded resolution path required for strict country
/// selection: the caller waits for the resolution once, with a timeout and a
/// lock that prevents concurrent duplicate resolutions.
pub struct IpCountryResolver {
    inner: RwLock<IpCountryResolverInner>,
    provider: Option<Arc<dyn IpCountryDataProvider>>,
    store: Option<Arc<dyn IpCountryStore>>,
    config: IpCountryResolverConfig,
}

struct IpCountryResolverInner {
    /// IP-to-country map indexed by string representation for fast lookup.
    map: HashMap<String, String>,
    /// Set of IPs currently being resolved, to prevent concurrent duplicates.
    resolving: HashMap<String, Instant>,
    /// Whether the map was loaded from the store.
    loaded: bool,
}

impl IpCountryResolver {
    /// Creates a new IP-country resolver.
    ///
    /// # Parameters
    ///
    /// - `config`: Resolver configuration (timeout, persistence).
    /// - `provider`: Optional provider for on-demand resolution.
    /// - `store`: Optional store for loading and persisting mappings.
    pub fn new(
        config: IpCountryResolverConfig,
        provider: Option<Arc<dyn IpCountryDataProvider>>,
        store: Option<Arc<dyn IpCountryStore>>,
    ) -> Self {
        Self {
            inner: RwLock::new(IpCountryResolverInner {
                map: HashMap::new(),
                resolving: HashMap::new(),
                loaded: false,
            }),
            provider,
            store,
            config,
        }
    }

    /// Loads the IP-country map from the configured store.
    ///
    /// This is a no-op when no store is configured or when the map has already
    /// been loaded.
    ///
    /// # Errors
    ///
    /// Returns an error when the store read or deserialisation fails.
    pub async fn load_from_store(&self) -> Result<()> {
        let store = match &self.store {
            Some(s) => s,
            None => return Ok(()),
        };

        let mut inner = self.inner.write().await;
        if inner.loaded {
            return Ok(());
        }

        let records = store.load_ip_countries().await?;
        for record in &records {
            inner
                .map
                .insert(record.ip.to_string(), record.country.clone());
        }
        inner.loaded = true;

        tracing::info!(
            count = records.len(),
            "loaded IP-country mappings from store"
        );

        Ok(())
    }

    /// Resolves an IP address to a country code.
    ///
    /// 1. Checks the in-memory map.
    /// 2. If not found and a provider is configured, calls the provider with a
    ///    bounded timeout.
    /// 3. If the provider returns a country, stores it in the map and
    ///    optionally persists to the store.
    ///
    /// # Parameters
    ///
    /// - `ip`: IP address to resolve.
    ///
    /// # Returns
    ///
    /// An optional country code when the IP could be resolved.
    ///
    /// # Errors
    ///
    /// Returns an error when the provider call or store persistence fails.
    pub async fn resolve(&self, ip: &IpAddr) -> Result<Option<String>> {
        // Fast path: check the in-memory map
        {
            let inner = self.inner.read().await;
            if let Some(country) = inner.map.get(&ip.to_string()) {
                return Ok(Some(country.clone()));
            }
        }

        let provider = match &self.provider {
            Some(p) => p,
            None => return Ok(None),
        };

        // Check if this IP is already being resolved by another task
        {
            let inner = self.inner.read().await;
            if inner.resolving.contains_key(&ip.to_string()) {
                // Wait briefly for the other task to finish, then re-check
                drop(inner);
                tokio::time::sleep(Duration::from_millis(100)).await;
                let inner = self.inner.read().await;
                if let Some(country) = inner.map.get(&ip.to_string()) {
                    return Ok(Some(country.clone()));
                }
                return Ok(None);
            }
        }

        // Mark as resolving
        {
            let mut inner = self.inner.write().await;
            inner.resolving.insert(ip.to_string(), Instant::now());
        }

        let result =
            tokio::time::timeout(self.config.resolve_timeout, provider.resolve_ip_country(ip))
                .await;

        // Clear the resolving marker
        {
            let mut inner = self.inner.write().await;
            inner.resolving.remove(&ip.to_string());
        }

        match result {
            Ok(Ok(Some(country))) => {
                // Store in the in-memory map
                {
                    let mut inner = self.inner.write().await;
                    inner.map.insert(ip.to_string(), country.clone());
                }

                // Optionally persist to the store
                if self.config.persist_on_resolve {
                    if let Some(store) = &self.store {
                        let inner = self.inner.read().await;
                        let records: Vec<IpCountryRecord> = inner
                            .map
                            .iter()
                            .map(|(ip_str, country)| {
                                let ip: IpAddr = ip_str.parse().unwrap_or_else(|_| {
                                    panic!("invalid IP in resolver map: {ip_str}")
                                });
                                IpCountryRecord {
                                    ip,
                                    country: country.clone(),
                                    source: Some("resolver".to_string()),
                                }
                            })
                            .collect();
                        drop(inner);
                        if let Err(error) = store.save_ip_countries(&records).await {
                            tracing::warn!(%error, "failed to persist IP-country mapping after resolve");
                        }
                    }
                }

                tracing::debug!(ip = %ip, country = %country, "resolved IP country via provider");
                Ok(Some(country))
            }
            Ok(Ok(None)) => {
                tracing::debug!(ip = %ip, "provider returned no country for IP");
                Ok(None)
            }
            Ok(Err(error)) => {
                tracing::warn!(ip = %ip, %error, "provider failed to resolve IP country");
                Ok(None)
            }
            Err(_) => {
                tracing::warn!(ip = %ip, "IP country resolution timed out");
                Ok(None)
            }
        }
    }

    /// Returns the number of cached IP-country mappings.
    pub async fn cached_count(&self) -> usize {
        self.inner.read().await.map.len()
    }

    /// Returns `true` when the resolver has been loaded from the store.
    pub async fn is_loaded(&self) -> bool {
        self.inner.read().await.loaded
    }
}
