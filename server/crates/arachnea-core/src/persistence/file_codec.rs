//! Reusable serialization codecs for file-backed persistence.

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Serializes and deserializes typed documents stored in a file.
pub trait PersistenceFileCodec: Send + Sync + 'static {
    /// Returns the file extension used by this codec, without a leading dot.
    fn file_extension(&self) -> &str;

    /// Serializes a document for storage.
    fn serialize<T: Serialize>(&self, document: &T) -> anyhow::Result<Vec<u8>>;

    /// Deserializes a document loaded from storage.
    fn deserialize<T: DeserializeOwned>(&self, payload: &[u8]) -> anyhow::Result<T>;
}

/// JSON codec used by file-backed stores by default.
#[derive(Debug, Default)]
pub struct JsonPersistenceFileCodec;

impl PersistenceFileCodec for JsonPersistenceFileCodec {
    fn file_extension(&self) -> &str {
        "json"
    }

    fn serialize<T: Serialize>(&self, document: &T) -> anyhow::Result<Vec<u8>> {
        serde_json::to_vec_pretty(document)
            .map_err(|err| anyhow::anyhow!("failed to serialize persistence document: {err}"))
    }

    fn deserialize<T: DeserializeOwned>(&self, payload: &[u8]) -> anyhow::Result<T> {
        serde_json::from_slice(payload)
            .map_err(|err| anyhow::anyhow!("failed to parse persistence document: {err}"))
    }
}
