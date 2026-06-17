use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::credentials_store::{
    normalize_service_id, read_file_if_exists, remove_file_if_exists, write_file_atomically,
    CredentialsDocument, CredentialsStore, StoredCredentials,
};

const CURRENT_ENCRYPTED_DOCUMENT_VERSION: u32 = 1;
const AES_GCM_NONCE_SIZE: usize = 12;

#[derive(Serialize, Deserialize)]
struct EncryptedCredentialsDocument {
    #[serde(default = "current_encrypted_document_version")]
    version: u32,
    nonce: String,
    ciphertext: String,
}

fn current_encrypted_document_version() -> u32 {
    CURRENT_ENCRYPTED_DOCUMENT_VERSION
}

/// AES-GCM backed credentials store encrypted as one JSON file.
pub struct EncryptedFileCredentialsStore {
    path: PathBuf,
    encryption_key: [u8; 32],
}

impl EncryptedFileCredentialsStore {
    /// Creates an encrypted file-backed credentials store.
    ///
    /// # Arguments
    /// * `path` - Absolute or relative path of the encrypted JSON file.
    /// * `encryption_key` - Static AES-256-GCM key used to encrypt the payload.
    ///
    /// # Returns
    /// An encrypted file-backed credentials store.
    pub fn new(path: impl AsRef<Path>, encryption_key: [u8; 32]) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            encryption_key,
        }
    }

    fn cipher(&self) -> Result<Aes256Gcm> {
        Aes256Gcm::new_from_slice(&self.encryption_key)
            .map_err(|_| anyhow!("Invalid AES-GCM credentials store key length."))
    }

    fn read_document(&self) -> Result<CredentialsDocument> {
        let Some(bytes) = read_file_if_exists(&self.path)? else {
            return Ok(CredentialsDocument::new());
        };

        let encrypted_document: EncryptedCredentialsDocument = serde_json::from_slice(&bytes)
            .with_context(|| {
                format!(
                    "Failed to parse encrypted credentials envelope `{}`.",
                    self.path.display()
                )
            })?;

        if encrypted_document.version == 0 {
            bail!(
                "Unsupported encrypted credentials document version in `{}`.",
                self.path.display()
            );
        }

        let nonce = BASE64_STANDARD
            .decode(encrypted_document.nonce.trim())
            .with_context(|| {
                format!(
                    "Failed to decode AES-GCM nonce from `{}`.",
                    self.path.display()
                )
            })?;
        if nonce.len() != AES_GCM_NONCE_SIZE {
            bail!(
                "Invalid AES-GCM nonce length in encrypted credentials store `{}`.",
                self.path.display()
            );
        }

        let ciphertext = BASE64_STANDARD
            .decode(encrypted_document.ciphertext.trim())
            .with_context(|| {
                format!(
                    "Failed to decode AES-GCM payload from `{}`.",
                    self.path.display()
                )
            })?;

        let plaintext = self
            .cipher()?
            .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
            .map_err(|_| {
                anyhow!(
                    "Failed to decrypt encrypted credentials store `{}`.",
                    self.path.display()
                )
            })?;

        let mut document: CredentialsDocument =
            serde_json::from_slice(&plaintext).with_context(|| {
                format!(
                    "Failed to parse decrypted credentials payload from `{}`.",
                    self.path.display()
                )
            })?;
        if document.version == 0 {
            document.version = 1;
        }

        Ok(document)
    }

    fn write_document(&self, document: &CredentialsDocument) -> Result<()> {
        let plaintext = serde_json::to_vec_pretty(document).with_context(|| {
            format!(
                "Failed to serialize credentials payload for `{}`.",
                self.path.display()
            )
        })?;

        let mut nonce = [0u8; AES_GCM_NONCE_SIZE];
        rand::fill(&mut nonce);

        let ciphertext = self
            .cipher()?
            .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
            .map_err(|_| {
                anyhow!(
                    "Failed to encrypt credentials payload for `{}`.",
                    self.path.display()
                )
            })?;

        let encrypted_document = EncryptedCredentialsDocument {
            version: current_encrypted_document_version(),
            nonce: BASE64_STANDARD.encode(nonce),
            ciphertext: BASE64_STANDARD.encode(ciphertext),
        };
        let bytes = serde_json::to_vec_pretty(&encrypted_document).with_context(|| {
            format!(
                "Failed to serialize encrypted credentials envelope `{}`.",
                self.path.display()
            )
        })?;

        write_file_atomically(&self.path, &bytes)
    }
}

impl CredentialsStore for EncryptedFileCredentialsStore {
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
