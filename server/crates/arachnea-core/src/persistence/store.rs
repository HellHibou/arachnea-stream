//! Generic asynchronous persistence contract for application records.

use std::time::SystemTime;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Namespace used to isolate data families in a persistence store.
///
/// Each namespace maps to a separate file in the file backend and can be
/// migrated independently. Examples: `cloudflare-session`, `http-cache`,
/// `scraper-cache`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PersistenceKey {
    /// Data family that owns the record.
    pub namespace: String,
    /// Domain-specific key within the namespace.
    pub key: String,
}

/// Opaque record stored by a [`PersistenceStore`].
///
/// The payload remains available for binary formats. JSON-oriented domains may
/// instead use flattened `fields` for an inspectable document representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedRecord {
    /// Version of the domain-specific payload format.
    pub format_version: u32,
    /// Opaque serialized payload for binary-oriented domains.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub payload: Vec<u8>,
    /// Domain fields written directly into JSON persistence documents.
    #[serde(default, skip_serializing_if = "Option::is_none", flatten)]
    pub fields: Option<Map<String, Value>>,
    /// Optional expiration hint used by the backend for pruning.
    pub expires_at: Option<SystemTime>,
    /// Timestamp of the last write.
    pub updated_at: SystemTime,
}

/// Asynchronous key-value persistence contract.
///
/// Implementations must be `Send + Sync` so they can be shared behind
/// `Arc<dyn PersistenceStore>` across concurrent clients.
///
/// The trait deliberately does not expose a generic `get<T: Serialize>`:
/// such a trait would not be usable behind `Arc<dyn PersistenceStore>`.
/// Each domain serializes and validates its own record before writing or
/// after reading.
#[async_trait]
pub trait PersistenceStore: Send + Sync {
    /// Returns the record stored under `key`, or `None` when absent.
    ///
    /// # Errors
    ///
    /// Returns a contextualized error when the backend cannot be read.
    async fn get(&self, key: &PersistenceKey) -> anyhow::Result<Option<PersistedRecord>>;

    /// Stores `record` under `key`, replacing any existing value.
    ///
    /// # Errors
    ///
    /// Returns a contextualized error when the backend cannot be written.
    async fn put(&self, key: PersistenceKey, record: PersistedRecord) -> anyhow::Result<()>;

    /// Removes the record stored under `key`.
    ///
    /// # Errors
    ///
    /// Returns a contextualized error when the backend cannot be modified.
    async fn delete(&self, key: &PersistenceKey) -> anyhow::Result<()>;
}
