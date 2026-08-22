//! Generic asynchronous persistence contract for application records.

use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Namespace and domain key pair identifying one record inside a backend.
///
/// Public callers manipulate plain string keys through
/// [`PersistenceTransaction`]; the namespace is carried by the transaction.
/// Backends use this structure internally to address records.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PersistenceKey {
    /// Data family that owns the record.
    pub namespace: String,
    /// Domain-specific key within the namespace.
    pub key: String,
}

/// Record stored by a [`PersistenceStore`].
///
/// Records are stored exclusively as named fields ([`PersistedRecord::fields`],
/// sub-objects allowed) so documents stay inspectable and field-based queries
/// remain possible. `format_version` supports future migrations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedRecord {
    /// Version of the domain-specific field layout.
    pub format_version: u32,
    /// Named domain fields written directly into persistence documents.
    #[serde(default, skip_serializing_if = "Option::is_none", flatten)]
    pub fields: Option<Map<String, Value>>,
    /// Optional expiration hint used by the backend for pruning.
    pub expires_at: Option<SystemTime>,
    /// Timestamp of the last write.
    pub updated_at: SystemTime,
}

/// Factory of namespace-bound transactions.
///
/// Implementations must be `Send + Sync` so they can be shared behind
/// `Arc<dyn PersistenceStore>` across concurrent clients. Each domain opens a
/// transaction on its own namespace, performs reads and writes with plain
/// string keys, then commits to flush pending changes.
#[async_trait]
pub trait PersistenceStore: Send + Sync {
    /// Creates a transaction bound to `namespace`.
    ///
    /// # Errors
    ///
    /// Returns a contextualized error when the backend cannot prepare the
    /// transaction.
    async fn transaction(&self, namespace: &str) -> anyhow::Result<PersistenceTransaction>;
}

/// Internal backend contract implemented by storage engines.
///
/// This trait is crate-private on purpose: external code manipulates records
/// through [`PersistenceStore`] and [`PersistenceTransaction`] only. Future
/// database backends implement this trait inside `arachnea-core`.
#[async_trait]
pub(crate) trait PersistenceBackend: Send + Sync {
    /// Returns the record stored under `key` in `namespace`, or `None` when
    /// absent.
    async fn get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<PersistedRecord>>;

    /// Stores `record` under `key` in `namespace`, replacing any existing
    /// value.
    async fn put(
        &self,
        namespace: &str,
        key: String,
        record: PersistedRecord,
    ) -> anyhow::Result<()>;

    /// Removes the record stored under `key` in `namespace`.
    async fn delete(&self, namespace: &str, key: &str) -> anyhow::Result<()>;

    /// Returns all `(key, record)` pairs of `namespace` whose fields match
    /// every `filters` entry. Filter keys support nested paths with dot
    /// notation (e.g. `"a.b"`).
    async fn find_by_fields(
        &self,
        namespace: &str,
        filters: &Map<String, Value>,
    ) -> anyhow::Result<Vec<(String, PersistedRecord)>>;

    /// Flushes pending changes of `namespace` when the backend defers writes.
    /// No-op for purely in-memory backends.
    async fn commit(&self, namespace: &str) -> anyhow::Result<()>;
}

/// Namespace-bound handle grouping all operations of one data family.
///
/// All operations apply to the namespace carried by this instance; keys are
/// plain domain keys. Transactions are lightweight (a namespace plus a shared
/// backend handle), hold no write state of their own, and may be recreated per
/// operation or kept alive across a read-modify-commit cycle. Several
/// transactions may coexist on the same namespace; the last write wins per
/// key. `commit` flushes pending changes of the namespace when the backend
/// defers writes; committing one namespace never touches other namespaces.
pub struct PersistenceTransaction {
    namespace: String,
    backend: Arc<dyn PersistenceBackend>,
}

impl PersistenceTransaction {
    /// Creates a transaction bound to `namespace` over `backend`.
    pub(crate) fn new(namespace: String, backend: Arc<dyn PersistenceBackend>) -> Self {
        Self { namespace, backend }
    }

    /// Returns the namespace this transaction operates on.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Returns the record stored under `key`, or `None` when absent.
    ///
    /// # Errors
    ///
    /// Returns a contextualized error when the backend cannot be read.
    pub async fn get(&self, key: &str) -> anyhow::Result<Option<PersistedRecord>> {
        self.backend.get(&self.namespace, key).await
    }

    /// Stores `record` under `key`, replacing any existing value.
    ///
    /// # Errors
    ///
    /// Returns a contextualized error when the backend cannot be written.
    pub async fn put(&self, key: String, record: PersistedRecord) -> anyhow::Result<()> {
        self.backend.put(&self.namespace, key, record).await
    }

    /// Removes the record stored under `key`.
    ///
    /// # Errors
    ///
    /// Returns a contextualized error when the backend cannot be modified.
    pub async fn delete(&self, key: &str) -> anyhow::Result<()> {
        self.backend.delete(&self.namespace, key).await
    }

    /// Returns all `(key, record)` pairs whose fields match every `filters`
    /// entry. Filter keys support nested paths with dot notation.
    ///
    /// # Errors
    ///
    /// Returns a contextualized error when the backend cannot be read.
    pub async fn find_by_fields(
        &self,
        filters: &Map<String, Value>,
    ) -> anyhow::Result<Vec<(String, PersistedRecord)>> {
        self.backend.find_by_fields(&self.namespace, filters).await
    }

    /// Flushes pending changes of the namespace when the backend defers
    /// writes.
    ///
    /// # Errors
    ///
    /// Returns a contextualized error when pending changes cannot be flushed.
    pub async fn commit(&self) -> anyhow::Result<()> {
        self.backend.commit(&self.namespace).await
    }
}

/// Returns `true` when every `filters` entry matches the record `fields`.
///
/// A filter key may designate a nested path using dot notation (e.g. `"a.b"`
/// matches field `a`, sub-field `b`). Matching is strict equality between JSON
/// values. A record without fields only matches empty filters.
pub(crate) fn record_matches_filters(
    fields: Option<&Map<String, Value>>,
    filters: &Map<String, Value>,
) -> bool {
    let Some(fields) = fields else {
        return filters.is_empty();
    };
    filters
        .iter()
        .all(|(path, expected)| field_value_at_path(fields, path) == Some(expected))
}

/// Resolves a dot-separated path inside a field map.
fn field_value_at_path<'a>(fields: &'a Map<String, Value>, path: &str) -> Option<&'a Value> {
    let mut segments = path.split('.');
    let first = segments.next()?;
    let mut current = fields.get(first)?;
    for segment in segments {
        current = current.get(segment)?;
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn empty_filters_match_any_record() {
        assert!(record_matches_filters(None, &Map::new()));
        assert!(record_matches_filters(Some(&fields(&[])), &Map::new()));
    }

    #[test]
    fn filters_match_top_level_fields() {
        let record = fields(&[("country", json!("FR")), ("port", json!(8080))]);
        let mut filters = Map::new();
        filters.insert("country".to_string(), json!("FR"));
        assert!(record_matches_filters(Some(&record), &filters));
        filters.insert("country".to_string(), json!("DE"));
        assert!(!record_matches_filters(Some(&record), &filters));
    }

    #[test]
    fn filters_support_nested_paths() {
        let record = fields(&[("meta", json!({ "region": "eu" }))]);
        let mut filters = Map::new();
        filters.insert("meta.region".to_string(), json!("eu"));
        assert!(record_matches_filters(Some(&record), &filters));
        filters.insert("meta.region".to_string(), json!("us"));
        assert!(!record_matches_filters(Some(&record), &filters));
        filters.insert("meta.missing".to_string(), json!("eu"));
        assert!(!record_matches_filters(Some(&record), &filters));
    }

    #[test]
    fn record_without_fields_rejects_non_empty_filters() {
        let mut filters = Map::new();
        filters.insert("country".to_string(), json!("FR"));
        assert!(!record_matches_filters(None, &filters));
    }
}