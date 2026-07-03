use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use crate::core::{ProxyRecord, Result};

/// Persistent store for proxy records.
///
/// Implementations manage serialisation, file I/O and atomicity so callers
/// can focus on inventory logic.
#[async_trait]
pub trait ProxyStore: Send + Sync {
    /// Load all proxy records from the store.
    ///
    /// Returns an empty vector when the backing file or resource does not exist.
    async fn load_proxies(&self) -> Result<Vec<ProxyRecord>>;

    /// Save a complete set of proxy records, replacing any existing data.
    async fn save_proxies(&self, proxies: &[ProxyRecord]) -> Result<()>;
}

/// Serialisation codec for proxy records.
///
/// Keeping codecs separate from the store allows the same I/O logic to be
/// reused with JSON, YAML, or other formats.
pub trait ProxySerdeCodec: Send + Sync {
    /// Human-readable format name for diagnostics (e.g. `"json"`, `"yaml"`).
    fn format_name(&self) -> &'static str;

    /// Serialise proxy records into bytes.
    fn serialize_proxies(&self, proxies: &[ProxyRecord]) -> Result<Vec<u8>>;

    /// Deserialise proxy records from bytes.
    fn deserialize_proxies(&self, bytes: &[u8]) -> Result<Vec<ProxyRecord>>;
}

/// File-backed proxy store that writes atomically through a temporary file
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
pub struct ProxySerdeStore {
    path: PathBuf,
    codec: Arc<dyn ProxySerdeCodec>,
}

impl ProxySerdeStore {
    /// Creates a new file-backed proxy store.
    ///
    /// # Parameters
    ///
    /// - `path`: Filesystem path for the proxy data file.
    /// - `codec`: Serialisation codec (e.g. `JsonProxyCodec`).
    pub fn new(path: impl Into<PathBuf>, codec: Arc<dyn ProxySerdeCodec>) -> Self {
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
    pub fn codec(&self) -> &Arc<dyn ProxySerdeCodec> {
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
impl ProxyStore for ProxySerdeStore {
    async fn load_proxies(&self) -> Result<Vec<ProxyRecord>> {
        let data = match tokio::fs::read(&self.path).await {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        self.codec.deserialize_proxies(&data)
    }

    async fn save_proxies(&self, proxies: &[ProxyRecord]) -> Result<()> {
        let bytes = self.codec.serialize_proxies(proxies)?;

        // Ensure parent directory exists
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let tmp = self.tmp_path();

        // Write to temporary file
        tokio::fs::write(&tmp, &bytes).await?;

        // Synchronise to disk
        let file = tokio::fs::File::open(&tmp).await?;
        file.sync_all().await?;
        drop(file);

        // Atomic rename
        tokio::fs::rename(&tmp, &self.path).await?;

        tracing::debug!(
            path = %self.path.display(),
            count = %proxies.len(),
            format = %self.codec.format_name(),
            "proxy store saved"
        );

        Ok(())
    }
}

// ── JSON codec ─────────────────────────────────────────────────────

/// Default JSON codec for proxy records.
///
/// Uses `serde_json` with pretty-printed output for readability and manual
/// inspection.
pub struct JsonProxyCodec;

impl JsonProxyCodec {
    /// Creates a new JSON codec.
    pub fn new() -> Self {
        Self
    }
}

impl Default for JsonProxyCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl ProxySerdeCodec for JsonProxyCodec {
    fn format_name(&self) -> &'static str {
        "json"
    }

    fn serialize_proxies(&self, proxies: &[ProxyRecord]) -> Result<Vec<u8>> {
        serde_json::to_vec_pretty(proxies)
            .map_err(|e| crate::core::ProxyError::Config(format!("json serialisation failed: {e}")))
    }

    fn deserialize_proxies(&self, bytes: &[u8]) -> Result<Vec<ProxyRecord>> {
        serde_json::from_slice(bytes).map_err(|e| {
            crate::core::ProxyError::Config(format!("json deserialisation failed: {e}"))
        })
    }
}

// ─── Convenience helpers for ProxyInventory ─────────────────────────

impl crate::core::ProxyInventory {
    /// Loads proxy records from a store and replaces the current inventory.
    ///
    /// # Parameters
    ///
    /// - `store`: Proxy store to load from.
    pub async fn load_from_store(&self, store: &dyn ProxyStore) -> Result<()> {
        let records = store.load_proxies().await?;
        let count = records.len();
        self.add_or_update(records).await;
        tracing::info!(count, "loaded proxy records from store");
        Ok(())
    }

    /// Persists all current proxy records to a store.
    ///
    /// # Parameters
    ///
    /// - `store`: Proxy store to save to.
    pub async fn save_to_store(&self, store: &dyn ProxyStore) -> Result<()> {
        let records = self.all_records().await;
        store.save_proxies(&records).await
    }
}
