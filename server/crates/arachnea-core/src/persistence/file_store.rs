//! File-backed persistence store with atomic writes and deferred commits.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{
    JsonPersistenceFileCodec, PersistedRecord, PersistenceBackend, PersistenceFileCodec,
    PersistenceStore, PersistenceTransaction, record_matches_filters,
};

/// Serialized document stored on disk for one namespace.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistenceFileDocument {
    /// Records keyed by domain key.
    records: HashMap<String, PersistedRecord>,
}

/// In-memory cache of one namespace document plus its dirty flag.
///
/// The flag is set whenever the cached document differs from the on-disk
/// document; [`FileState::commit`] flushes the namespace only when set.
struct NamespaceCache {
    /// Cached document for the namespace.
    document: PersistenceFileDocument,
    /// Whether the cached document differs from the on-disk document.
    dirty: bool,
}

/// Shared state of the file-backed backend: memory cache, dirty flags and
/// file-writing primitives.
struct FileState<C> {
    /// Directory containing one file per namespace.
    directory: PathBuf,
    /// Extension assigned to namespace files, without a leading dot.
    extension: String,
    /// Codec used for namespace documents.
    codec: Arc<C>,
    /// Cached namespace documents keyed by namespace.
    namespaces: Mutex<HashMap<String, NamespaceCache>>,
}

/// Legacy file-backed [`PersistenceStore`] implementation.
///
/// One file per namespace, stored under the application data directory by
/// default (`data/store-NAMESPACE.json`). The store keeps an in-memory cache
/// per namespace: reads are served from memory, while writes only update the
/// cache and mark the namespace dirty. [`PersistenceTransaction::commit`]
/// flushes a dirty namespace to disk atomically (temporary file + fsync +
/// rename); committing a clean namespace performs no I/O. Expired records are
/// pruned when a namespace is loaded or read; pruning marks the namespace
/// dirty so the next commit persists it.
///
/// The default codec is JSON. Call [`LegacyFilePersistenceStore::new_with_codec`] to
/// select another codec.
pub struct LegacyFilePersistenceStore<C = JsonPersistenceFileCodec> {
    state: Arc<FileState<C>>,
}

impl LegacyFilePersistenceStore<JsonPersistenceFileCodec> {
    /// Creates a file store rooted at `directory`.
    ///
    /// # Arguments
    ///
    /// * `directory` - Directory containing one file per namespace.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            state: Arc::new(FileState {
                directory: directory.into(),
                extension: JsonPersistenceFileCodec.file_extension().to_string(),
                codec: Arc::new(JsonPersistenceFileCodec),
                namespaces: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Creates a file store rooted at the default application data directory.
    ///
    /// The directory is `get_application_data_path("data")`.
    pub fn default_data_dir() -> Self {
        Self::new(crate::application::get_application_data_path("data"))
    }
}

impl<C: PersistenceFileCodec> LegacyFilePersistenceStore<C> {
    /// Creates a file store using `codec` and its file extension per namespace.
    pub fn new_with_codec(directory: impl Into<PathBuf>, codec: C) -> Self {
        let extension = codec.file_extension().trim_start_matches('.').to_string();
        assert!(
            !extension.is_empty(),
            "persistence file extension must not be empty"
        );
        Self {
            state: Arc::new(FileState {
                directory: directory.into(),
                extension,
                codec: Arc::new(codec),
                namespaces: Mutex::new(HashMap::new()),
            }),
        }
    }
}

impl<C: PersistenceFileCodec> FileState<C> {
    /// Loads the namespace into the cache when not already loaded.
    ///
    /// Expired records found on disk are pruned during load and mark the
    /// namespace dirty so the next commit persists the pruned document.
    async fn load_if_needed(&self, namespace: &str) -> anyhow::Result<()> {
        let mut namespaces = self.namespaces.lock().await;
        if namespaces.contains_key(namespace) {
            return Ok(());
        }
        let directory = self.directory.clone();
        let owned_namespace = namespace.to_string();
        let extension = self.extension.clone();
        let codec = self.codec.clone();
        let (document, pruned) = tokio::task::spawn_blocking(move || {
            load_document(directory, owned_namespace, extension, codec)
        })
        .await
        .map_err(|err| anyhow::anyhow!("persistence read task failed: {err}"))??;
        namespaces.insert(
            namespace.to_string(),
            NamespaceCache {
                document,
                dirty: pruned,
            },
        );
        Ok(())
    }

    /// Persists a namespace document atomically (tmp + fsync + rename).
    ///
    /// # Errors
    ///
    /// Returns a contextualized error when the document cannot be written.
    fn persist_document(
        directory: PathBuf,
        extension: String,
        codec: Arc<C>,
        namespace: String,
        document: PersistenceFileDocument,
    ) -> anyhow::Result<()> {
        let path = namespace_path(&directory, &namespace, &extension);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                anyhow::anyhow!(
                    "failed to create persistence directory {}: {err}",
                    parent.display()
                )
            })?;
        }
        let payload = codec.serialize(&document)?;
        let temp_path = path.with_extension(format!("{}.{}.tmp", extension, std::process::id()));
        let mut file = fs::File::create(&temp_path).map_err(|err| {
            anyhow::anyhow!(
                "failed to create temporary persistence document {}: {err}",
                temp_path.display()
            )
        })?;
        file.write_all(&payload).map_err(|err| {
            anyhow::anyhow!(
                "failed to write temporary persistence document {}: {err}",
                temp_path.display()
            )
        })?;
        file.sync_all().map_err(|err| {
            anyhow::anyhow!(
                "failed to synchronise temporary persistence document {}: {err}",
                temp_path.display()
            )
        })?;
        drop(file);
        fs::rename(&temp_path, &path).map_err(|err| {
            anyhow::anyhow!(
                "failed to rename persistence document {} to {}: {err}",
                temp_path.display(),
                path.display()
            )
        })?;
        tracing::debug!(path = %path.display(), "persistence namespace committed");
        Ok(())
    }
}

fn namespace_path(directory: &std::path::Path, namespace: &str, extension: &str) -> PathBuf {
    directory.join(format!("store-{}.{}", safe_namespace(namespace), extension))
}

/// Encodes a namespace into a safe file-name component.
///
/// Only alphanumeric characters, `-`, and `_` are kept; other characters are
/// percent-encoded.
fn safe_namespace(namespace: &str) -> String {
    let mut safe = String::with_capacity(namespace.len());
    for byte in namespace.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' {
            safe.push(byte as char);
        } else {
            safe.push_str(&format!("%{byte:02X}"));
        }
    }
    safe
}

/// Loads a namespace document from disk, pruning expired records.
///
/// # Returns
///
/// The loaded document and whether expired records were pruned.
fn load_document<C: PersistenceFileCodec>(
    directory: PathBuf,
    namespace: String,
    extension: String,
    codec: Arc<C>,
) -> anyhow::Result<(PersistenceFileDocument, bool)> {
    let path = namespace_path(&directory, &namespace, &extension);
    tracing::debug!("Loading file {}", path.display());
    let mut document = match fs::read(&path) {
        Ok(payload) => codec
            .deserialize(&payload)
            .map_err(|err| anyhow::anyhow!("{}: {err}", path.display()))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            PersistenceFileDocument::default()
        }
        Err(err) => {
            return Err(anyhow::anyhow!(
                "failed to read persistence document {}: {err}",
                path.display()
            ));
        }
    };
    let now = std::time::SystemTime::now();
    let record_count = document.records.len();
    document
        .records
        .retain(|_, record| record.expires_at.is_none_or(|expires_at| expires_at > now));
    let pruned = document.records.len() != record_count;
    Ok((document, pruned))
}

#[async_trait]
impl<C: PersistenceFileCodec> PersistenceStore for LegacyFilePersistenceStore<C> {
    async fn transaction(&self, namespace: &str) -> anyhow::Result<PersistenceTransaction> {
        Ok(PersistenceTransaction::new(
            namespace.to_string(),
            self.state.clone(),
        ))
    }
}

#[async_trait]
impl<C: PersistenceFileCodec> PersistenceBackend for FileState<C> {
    async fn get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<PersistedRecord>> {
        self.load_if_needed(namespace).await?;
        let mut namespaces = self.namespaces.lock().await;
        let cache = namespaces
            .get_mut(namespace)
            .expect("persistence namespace was inserted");
        if cache.document.records.get(key).is_some_and(|record| {
            record
                .expires_at
                .is_some_and(|expires_at| expires_at <= std::time::SystemTime::now())
        }) {
            cache.document.records.remove(key);
            cache.dirty = true;
            return Ok(None);
        }
        Ok(cache.document.records.get(key).cloned())
    }

    async fn put(
        &self,
        namespace: &str,
        key: String,
        record: PersistedRecord,
    ) -> anyhow::Result<()> {
        self.load_if_needed(namespace).await?;
        let mut namespaces = self.namespaces.lock().await;
        let cache = namespaces
            .get_mut(namespace)
            .expect("persistence namespace was inserted");
        cache.document.records.insert(key, record);
        cache.dirty = true;
        Ok(())
    }

    async fn delete(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
        self.load_if_needed(namespace).await?;
        let mut namespaces = self.namespaces.lock().await;
        let cache = namespaces
            .get_mut(namespace)
            .expect("persistence namespace was inserted");
        if cache.document.records.remove(key).is_some() {
            cache.dirty = true;
        }
        Ok(())
    }

    async fn find_by_fields(
        &self,
        namespace: &str,
        filters: &Map<String, Value>,
    ) -> anyhow::Result<Vec<(String, PersistedRecord)>> {
        self.load_if_needed(namespace).await?;
        let namespaces = self.namespaces.lock().await;
        let cache = namespaces
            .get(namespace)
            .expect("persistence namespace was inserted");
        Ok(cache
            .document
            .records
            .iter()
            .filter(|(_, record)| record_matches_filters(record.fields.as_ref(), filters))
            .map(|(key, record)| (key.clone(), record.clone()))
            .collect())
    }

    async fn commit(&self, namespace: &str) -> anyhow::Result<()> {
        let document = {
            let mut namespaces = self.namespaces.lock().await;
            let Some(cache) = namespaces.get_mut(namespace) else {
                return Ok(());
            };
            if !cache.dirty {
                return Ok(());
            }
            cache.document.clone()
        };
        let directory = self.directory.clone();
        let extension = self.extension.clone();
        let codec = self.codec.clone();
        let owned_namespace = namespace.to_string();
        tokio::task::spawn_blocking(move || {
            Self::persist_document(directory, extension, codec, owned_namespace, document)
        })
        .await
        .map_err(|err| anyhow::anyhow!("persistence commit task failed: {err}"))??;
        let mut namespaces = self.namespaces.lock().await;
        if let Some(cache) = namespaces.get_mut(namespace) {
            cache.dirty = false;
        }
        Ok(())
    }
}

impl<C> std::fmt::Debug for LegacyFilePersistenceStore<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LegacyFilePersistenceStore")
            .field("directory", &self.state.directory)
            .field("extension", &self.state.extension)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::time::{Duration, SystemTime};

    fn unique_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock is after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "arachnea-core-{name}-{}-{stamp}",
            std::process::id()
        ))
    }

    fn record(fields: Value, expires_at: Option<SystemTime>) -> PersistedRecord {
        PersistedRecord {
            format_version: 1,
            fields: Some(fields.as_object().cloned().expect("object fields")),
            expires_at,
            updated_at: SystemTime::now(),
        }
    }

    #[tokio::test]
    async fn put_commit_get_roundtrip() {
        let dir = unique_dir("roundtrip");
        let store = LegacyFilePersistenceStore::new(&dir);
        let transaction = store.transaction("test-ns").await.expect("transaction");
        transaction
            .put("entry-1".to_string(), record(json!({"value": "hello"}), None))
            .await
            .expect("put succeeds");
        transaction.commit().await.expect("commit succeeds");

        // A fresh store instance reads the committed document from disk.
        let reloaded = LegacyFilePersistenceStore::new(&dir);
        let reloaded_transaction = reloaded.transaction("test-ns").await.expect("transaction");
        let got = reloaded_transaction
            .get("entry-1")
            .await
            .expect("get succeeds");
        assert!(got.is_some());
        assert_eq!(
            got.unwrap().fields.unwrap().get("value").cloned(),
            Some(json!("hello"))
        );
        assert!(namespace_path(&dir, "test-ns", "json").exists());
        std::fs::remove_dir_all(&dir).expect("test dir removed");
    }

    #[tokio::test]
    async fn put_defers_write_until_commit() {
        let dir = unique_dir("deferred");
        let store = LegacyFilePersistenceStore::new(&dir);
        let transaction = store.transaction("test-ns").await.expect("transaction");
        transaction
            .put("entry-1".to_string(), record(json!({"value": 1}), None))
            .await
            .expect("put succeeds");
        assert!(!namespace_path(&dir, "test-ns", "json").exists());
        transaction.commit().await.expect("commit succeeds");
        assert!(namespace_path(&dir, "test-ns", "json").exists());
        std::fs::remove_dir_all(&dir).expect("test dir removed");
    }

    #[tokio::test]
    async fn commit_clean_namespace_writes_nothing() {
        let dir = unique_dir("clean-commit");
        let store = LegacyFilePersistenceStore::new(&dir);
        let transaction = store.transaction("missing-ns").await.expect("transaction");
        transaction.commit().await.expect("commit succeeds");
        assert!(!namespace_path(&dir, "missing-ns", "json").exists());
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("test dir removed");
        }
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let dir = unique_dir("missing");
        let store = LegacyFilePersistenceStore::new(&dir);
        let transaction = store.transaction("test-ns").await.expect("transaction");
        assert!(transaction.get("missing").await.expect("get succeeds").is_none());
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("test dir removed");
        }
    }

    #[tokio::test]
    async fn delete_removes_record_after_commit() {
        let dir = unique_dir("delete");
        let store = LegacyFilePersistenceStore::new(&dir);
        let transaction = store.transaction("test-ns").await.expect("transaction");
        transaction
            .put("entry-1".to_string(), record(json!({"value": 1}), None))
            .await
            .expect("put succeeds");
        transaction.commit().await.expect("commit succeeds");
        transaction.delete("entry-1").await.expect("delete succeeds");
        transaction.commit().await.expect("commit succeeds");
        assert!(transaction.get("entry-1").await.expect("get succeeds").is_none());

        let reloaded = LegacyFilePersistenceStore::new(&dir);
        let reloaded_transaction = reloaded.transaction("test-ns").await.expect("transaction");
        assert!(reloaded_transaction
            .get("entry-1")
            .await
            .expect("get succeeds")
            .is_none());
        std::fs::remove_dir_all(&dir).expect("test dir removed");
    }

    #[tokio::test]
    async fn expired_record_is_pruned_on_read() {
        let dir = unique_dir("expired");
        let store = LegacyFilePersistenceStore::new(&dir);
        let transaction = store.transaction("test-ns").await.expect("transaction");
        let past = SystemTime::now() - Duration::from_secs(60);
        transaction
            .put("entry-1".to_string(), record(json!({"value": 1}), Some(past)))
            .await
            .expect("put succeeds");
        transaction.commit().await.expect("commit succeeds");
        assert!(transaction.get("entry-1").await.expect("get succeeds").is_none());
        std::fs::remove_dir_all(&dir).expect("test dir removed");
    }

    #[tokio::test]
    async fn find_by_fields_filters_records() {
        let dir = unique_dir("find");
        let store = LegacyFilePersistenceStore::new(&dir);
        let transaction = store.transaction("test-ns").await.expect("transaction");
        transaction
            .put("fr-1".to_string(), record(json!({"country": "FR"}), None))
            .await
            .expect("put fr-1");
        transaction
            .put("de-1".to_string(), record(json!({"country": "DE"}), None))
            .await
            .expect("put de-1");
        transaction
            .put(
                "nested".to_string(),
                record(json!({"meta": {"region": "eu"}}), None),
            )
            .await
            .expect("put nested");

        let mut filters = Map::new();
        filters.insert("country".to_string(), json!("FR"));
        let matches = transaction
            .find_by_fields(&filters)
            .await
            .expect("find_by_fields succeeds");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "fr-1");

        let mut nested_filters = Map::new();
        nested_filters.insert("meta.region".to_string(), json!("eu"));
        let nested_matches = transaction
            .find_by_fields(&nested_filters)
            .await
            .expect("find_by_fields succeeds");
        assert_eq!(nested_matches.len(), 1);
        assert_eq!(nested_matches[0].0, "nested");
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("test dir removed");
        }
    }

    #[tokio::test]
    async fn commits_are_isolated_per_namespace() {
        let dir = unique_dir("isolated");
        let store = LegacyFilePersistenceStore::new(&dir);
        let proxies = store.transaction("proxy-inventory").await.expect("transaction");
        let sessions = store
            .transaction("cloudflare-session")
            .await
            .expect("transaction");
        proxies
            .put("entry-1".to_string(), record(json!({"kind": "proxy"}), None))
            .await
            .expect("put proxy");
        sessions
            .put("entry-1".to_string(), record(json!({"kind": "session"}), None))
            .await
            .expect("put session");
        proxies.commit().await.expect("commit proxies");
        assert!(namespace_path(&dir, "proxy-inventory", "json").exists());
        assert!(!namespace_path(&dir, "cloudflare-session", "json").exists());
        sessions.commit().await.expect("commit sessions");
        assert!(namespace_path(&dir, "cloudflare-session", "json").exists());
        std::fs::remove_dir_all(&dir).expect("test dir removed");
    }

    #[test]
    fn safe_namespace_encodes_special_chars() {
        assert_eq!(safe_namespace("cloudflare-session"), "cloudflare-session");
        assert_eq!(safe_namespace("http/cache"), "http%2Fcache");
        assert_eq!(safe_namespace("a b"), "a%20b");
    }
}
