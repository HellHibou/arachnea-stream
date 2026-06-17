use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::credentials_store::{
    normalize_service_id, read_file_if_exists, remove_file_if_exists, write_file_atomically,
    CredentialsDocument, CredentialsStore, StoredCredentials,
};

/// JSON-backed credentials store kept in clear text on disk.
pub struct FileCredentialsStore {
    path: PathBuf,
}

impl FileCredentialsStore {
    /// Creates a clear-text credentials store backed by one JSON file.
    ///
    /// # Arguments
    /// * `path` - Absolute or relative path of the JSON file to read and write.
    ///
    /// # Returns
    /// A file-backed credentials store.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    fn read_document(&self) -> Result<CredentialsDocument> {
        let Some(bytes) = read_file_if_exists(&self.path)? else {
            return Ok(CredentialsDocument::new());
        };

        let mut document: CredentialsDocument =
            serde_json::from_slice(&bytes).with_context(|| {
                format!(
                    "Failed to parse JSON credentials store `{}`.",
                    self.path.display()
                )
            })?;
        if document.version == 0 {
            document.version = 1;
        }

        Ok(document)
    }

    fn write_document(&self, document: &CredentialsDocument) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(document).with_context(|| {
            format!(
                "Failed to serialize JSON credentials store `{}`.",
                self.path.display()
            )
        })?;

        write_file_atomically(&self.path, &bytes)
    }
}

impl CredentialsStore for FileCredentialsStore {
    fn get_credentials(&self, service_id: &str) -> Result<Option<StoredCredentials>> {
        let normalized_service_id = normalize_service_id(service_id)?;
        let document = self.read_document()?;

        document
            .services
            .get(&normalized_service_id)
            .cloned()
            .map(StoredCredentials::normalized)
            .transpose()
    }

    fn set_credentials(&self, service_id: &str, credentials: StoredCredentials) -> Result<()> {
        let normalized_service_id = normalize_service_id(service_id)?;
        let normalized_credentials = credentials.normalized()?;
        let mut document = self.read_document()?;
        document.version = 1;
        document
            .services
            .insert(normalized_service_id, normalized_credentials);

        self.write_document(&document)
    }

    fn clear_credentials(&self, service_id: &str) -> Result<()> {
        let normalized_service_id = normalize_service_id(service_id)?;
        let mut document = self.read_document()?;
        document.services.remove(&normalized_service_id);

        if document.services.is_empty() {
            remove_file_if_exists(&self.path)
        } else {
            self.write_document(&document)
        }
    }
}
