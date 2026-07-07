use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

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
