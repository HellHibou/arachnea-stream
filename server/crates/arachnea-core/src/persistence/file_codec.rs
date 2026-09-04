//! Reusable serialization codecs for file-backed persistence.

use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use anyhow::Context;
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

    /// Serializes and atomically saves a document to a file.
    ///
    /// The parent directory is created when necessary. The payload is flushed to
    /// a temporary file in the target directory before atomically replacing the
    /// target file.
    ///
    /// # Arguments
    ///
    /// * `path` - Target file path.
    /// * `document` - Document to serialize and persist.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization, directory creation, writing,
    /// synchronization, or replacement fails.
    fn save<T: Serialize>(&self, path: impl AsRef<Path>, document: &T) -> anyhow::Result<()> {
        let path = path.as_ref();
        let payload = self.serialize(document).with_context(|| {
            format!(
                "Failed to serialize persistence document {}.",
                path.display()
            )
        })?;
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create persistence directory {}.",
                    parent.display()
                )
            })?;
        }

        let temporary_path = temporary_path_for(path)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary_path).with_context(|| {
            format!(
                "Failed to create temporary persistence document {}.",
                temporary_path.display()
            )
        })?;
        file.write_all(&payload).with_context(|| {
            format!(
                "Failed to write temporary persistence document {}.",
                temporary_path.display()
            )
        })?;
        file.sync_all().with_context(|| {
            format!(
                "Failed to flush temporary persistence document {}.",
                temporary_path.display()
            )
        })?;
        drop(file);

        fs::rename(&temporary_path, path)
            .with_context(|| format!("Failed to replace persistence document {}.", path.display()))
    }
}

/// Derives a randomized temporary path in the target file's directory.
fn temporary_path_for(path: &Path) -> anyhow::Result<std::path::PathBuf> {
    let file_name = path.file_name().with_context(|| {
        format!(
            "Cannot derive a temporary persistence path from {}.",
            path.display()
        )
    })?;
    let mut temporary_name = OsString::from(".");
    temporary_name.push(file_name);
    temporary_name.push(format!(".{:016x}.tmp", rand::random::<u64>()));
    Ok(path.with_file_name(temporary_name))
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
