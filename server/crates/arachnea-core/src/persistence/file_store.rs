//! File-backed persistence store with atomic writes.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    JsonPersistenceFileCodec, PersistedRecord, PersistenceFileCodec, PersistenceKey,
    PersistenceStore,
};

/// Serialized document stored on disk for one namespace.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistenceFileDocument {
    /// Records keyed by domain key.
    records: HashMap<String, PersistedRecord>,
}

/// File-backed [`PersistenceStore`] implementation.
///
/// One file per namespace, stored under the application data directory by
/// default (`data/store-NAMESPACE.json`). Writes are atomic: the document is
/// written to a temporary file in the same directory, then renamed over the
/// target. Expired records are pruned on read.
///
/// The default codec is JSON. Call [`FilePersistenceStore::new_with_codec`] to
/// select another codec.
pub struct FilePersistenceStore<C = JsonPersistenceFileCodec> {
    /// Directory containing one file per namespace.
    directory: PathBuf,
    /// In-memory cache of loaded namespace documents.
    documents: Mutex<HashMap<String, PersistenceFileDocument>>,
    /// Codec used for namespace documents.
    codec: Arc<C>,
    /// Extension assigned to namespace files, without a leading dot.
    extension: String,
}

impl FilePersistenceStore<JsonPersistenceFileCodec> {
    /// Creates a file store rooted at `directory`.
    ///
    /// # Arguments
    ///
    /// * `directory` - Directory containing one file per namespace.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            documents: Mutex::new(HashMap::new()),
            codec: Arc::new(JsonPersistenceFileCodec),
            extension: JsonPersistenceFileCodec.file_extension().to_string(),
        }
    }

    /// Creates a file store rooted at the default application data directory.
    ///
    /// The directory is `get_application_path("data")`.
    pub fn default_data_dir() -> Self {
        Self::new(crate::application::get_application_path("data"))
    }
}

impl<C: PersistenceFileCodec> FilePersistenceStore<C> {
    /// Creates a file store using `codec` and its file extension per namespace.
    pub fn new_with_codec(directory: impl Into<PathBuf>, codec: C) -> Self {
        let extension = codec.file_extension().trim_start_matches('.').to_string();
        assert!(
            !extension.is_empty(),
            "persistence file extension must not be empty"
        );
        Self {
            directory: directory.into(),
            documents: Mutex::new(HashMap::new()),
            codec: Arc::new(codec),
            extension,
        }
    }

    /// Loads the document for a namespace, pruning expired records.
    fn load_document(
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

    /// Persists a namespace document atomically.
    fn persist_document(
        directory: PathBuf,
        namespace: String,
        extension: String,
        codec: Arc<C>,
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
        fs::write(&temp_path, &payload).map_err(|err| {
            anyhow::anyhow!(
                "failed to write temporary persistence document {}: {err}",
                temp_path.display()
            )
        })?;
        fs::rename(&temp_path, &path).map_err(|err| {
            anyhow::anyhow!(
                "failed to rename persistence document {} to {}: {err}",
                temp_path.display(),
                path.display()
            )
        })?;
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

#[async_trait]
impl<C: PersistenceFileCodec> PersistenceStore for FilePersistenceStore<C> {
    async fn get(&self, key: &PersistenceKey) -> anyhow::Result<Option<PersistedRecord>> {
        let mut documents = self.documents.lock().await;
        if !documents.contains_key(&key.namespace) {
            let directory = self.directory.clone();
            let namespace = key.namespace.clone();
            let extension = self.extension.clone();
            let codec = self.codec.clone();
            let (document, pruned) = tokio::task::spawn_blocking(move || {
                Self::load_document(directory, namespace, extension, codec)
            })
            .await
            .map_err(|err| anyhow::anyhow!("persistence read task failed: {err}"))??;
            if pruned {
                let directory = self.directory.clone();
                let namespace = key.namespace.clone();
                let extension = self.extension.clone();
                let codec = self.codec.clone();
                let document_to_persist = document.clone();
                tokio::task::spawn_blocking(move || {
                    Self::persist_document(
                        directory,
                        namespace,
                        extension,
                        codec,
                        document_to_persist,
                    )
                })
                .await
                .map_err(|err| anyhow::anyhow!("persistence prune task failed: {err}"))??;
            }
            documents.insert(key.namespace.clone(), document);
        }
        let document = documents
            .get_mut(&key.namespace)
            .expect("persistence document was inserted");
        let record = document.records.get(&key.key).cloned();
        if record.as_ref().is_some_and(|record| {
            record
                .expires_at
                .is_some_and(|expires_at| expires_at <= std::time::SystemTime::now())
        }) {
            document.records.remove(&key.key);
            let directory = self.directory.clone();
            let namespace = key.namespace.clone();
            let extension = self.extension.clone();
            let codec = self.codec.clone();
            let document_to_persist = document.clone();
            tokio::task::spawn_blocking(move || {
                Self::persist_document(directory, namespace, extension, codec, document_to_persist)
            })
            .await
            .map_err(|err| anyhow::anyhow!("persistence prune task failed: {err}"))??;
            return Ok(None);
        }
        Ok(record)
    }

    async fn put(&self, key: PersistenceKey, record: PersistedRecord) -> anyhow::Result<()> {
        let mut documents = self.documents.lock().await;
        if !documents.contains_key(&key.namespace) {
            let directory = self.directory.clone();
            let namespace = key.namespace.clone();
            let extension = self.extension.clone();
            let codec = self.codec.clone();
            let (document, _) = tokio::task::spawn_blocking(move || {
                Self::load_document(directory, namespace, extension, codec)
            })
            .await
            .map_err(|err| anyhow::anyhow!("persistence read task failed: {err}"))??;
            documents.insert(key.namespace.clone(), document);
        }
        let document = documents
            .get_mut(&key.namespace)
            .expect("persistence document was inserted");
        document.records.insert(key.key, record);
        let directory = self.directory.clone();
        let namespace = key.namespace;
        let extension = self.extension.clone();
        let codec = self.codec.clone();
        let document = document.clone();
        tokio::task::spawn_blocking(move || {
            Self::persist_document(directory, namespace, extension, codec, document)
        })
        .await
        .map_err(|err| anyhow::anyhow!("persistence write task failed: {err}"))?
    }

    async fn delete(&self, key: &PersistenceKey) -> anyhow::Result<()> {
        let mut documents = self.documents.lock().await;
        if !documents.contains_key(&key.namespace) {
            let directory = self.directory.clone();
            let namespace = key.namespace.clone();
            let extension = self.extension.clone();
            let codec = self.codec.clone();
            let (document, _) = tokio::task::spawn_blocking(move || {
                Self::load_document(directory, namespace, extension, codec)
            })
            .await
            .map_err(|err| anyhow::anyhow!("persistence read task failed: {err}"))??;
            documents.insert(key.namespace.clone(), document);
        }
        let document = documents
            .get_mut(&key.namespace)
            .expect("persistence document was inserted");
        document.records.remove(&key.key);
        let directory = self.directory.clone();
        let namespace = key.namespace.clone();
        let extension = self.extension.clone();
        let codec = self.codec.clone();
        let document = document.clone();
        tokio::task::spawn_blocking(move || {
            Self::persist_document(directory, namespace, extension, codec, document)
        })
        .await
        .map_err(|err| anyhow::anyhow!("persistence write task failed: {err}"))?
    }
}

impl<C> std::fmt::Debug for FilePersistenceStore<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilePersistenceStore")
            .field("directory", &self.directory)
            .field("extension", &self.extension)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn record(payload: &[u8], expires_at: Option<SystemTime>) -> PersistedRecord {
        PersistedRecord {
            format_version: 1,
            payload: payload.to_vec(),
            fields: None,
            expires_at,
            updated_at: SystemTime::now(),
        }
    }

    #[tokio::test]
    async fn put_get_roundtrip() {
        let dir = unique_dir("roundtrip");
        let store = FilePersistenceStore::new(&dir);
        let key = PersistenceKey {
            namespace: "test-ns".to_string(),
            key: "entry-1".to_string(),
        };
        store
            .put(key.clone(), record(b"hello", None))
            .await
            .expect("put succeeds");
        let got = store.get(&key).await.expect("get succeeds");
        assert!(got.is_some());
        assert_eq!(got.unwrap().payload, b"hello");
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("test dir removed");
        }
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let dir = unique_dir("missing");
        let store = FilePersistenceStore::new(&dir);
        let key = PersistenceKey {
            namespace: "test-ns".to_string(),
            key: "missing".to_string(),
        };
        assert!(store.get(&key).await.expect("get succeeds").is_none());
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("test dir removed");
        }
    }

    #[tokio::test]
    async fn delete_removes_record() {
        let dir = unique_dir("delete");
        let store = FilePersistenceStore::new(&dir);
        let key = PersistenceKey {
            namespace: "test-ns".to_string(),
            key: "entry-1".to_string(),
        };
        store
            .put(key.clone(), record(b"hello", None))
            .await
            .expect("put succeeds");
        store.delete(&key).await.expect("delete succeeds");
        assert!(store.get(&key).await.expect("get succeeds").is_none());
        std::fs::remove_dir_all(&dir).expect("test dir removed");
    }

    #[tokio::test]
    async fn expired_record_is_pruned() {
        let dir = unique_dir("expired");
        let store = FilePersistenceStore::new(&dir);
        let key = PersistenceKey {
            namespace: "test-ns".to_string(),
            key: "entry-1".to_string(),
        };
        let past = SystemTime::now() - Duration::from_secs(60);
        store
            .put(key.clone(), record(b"stale", Some(past)))
            .await
            .expect("put succeeds");
        assert!(store.get(&key).await.expect("get succeeds").is_none());
        std::fs::remove_dir_all(&dir).expect("test dir removed");
    }

    #[test]
    fn safe_namespace_encodes_special_chars() {
        assert_eq!(safe_namespace("cloudflare-session"), "cloudflare-session");
        assert_eq!(safe_namespace("http/cache"), "http%2Fcache");
        assert_eq!(safe_namespace("a b"), "a%20b");
    }
}
