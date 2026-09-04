//! Credentials storage contracts and shared helpers.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// The current version of the credentials document format.
///
/// This constant is used to ensure compatibility when reading and writing
/// credentials documents.
const CURRENT_DOCUMENT_VERSION: u32 = 1;

/// Credentials persisted for one backend service.
///
/// This struct represents the login credentials for a single service,
/// including both the login identifier and password.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredCredentials {
    /// Service login identifier.
    ///
    /// This field contains the username or identifier used to authenticate
    /// with the service.
    pub login: String,
    /// Service password.
    ///
    /// This field contains the password or secret used to authenticate
    /// with the service.
    pub password: String,
}

impl StoredCredentials {
    /// Normalizes the credentials by trimming whitespace and validating.
    ///
    /// # Returns
    /// `Ok(Self)` with trimmed login and password if both are non-empty.
    /// `Err(anyhow::Error)` if either login or password is empty after trimming.
    pub(crate) fn normalized(self) -> Result<Self> {
        let login = self.login.trim().to_string();
        if login.is_empty() {
            bail!("Missing credentials login.");
        }

        let password = self.password.trim().to_string();
        if password.is_empty() {
            bail!("Missing credentials password.");
        }

        Ok(Self { login, password })
    }
}

/// Storage abstraction used to persist service credentials.
///
/// This trait defines the interface for storing and retrieving credentials
/// for various backend services.
#[allow(dead_code)]
pub trait CredentialsStore: Send + Sync {
    /// Loads credentials for one configured service.
    ///
    /// # Arguments
    /// * `service_id` - Stable service identifier such as `m6play-fr`.
    ///
    /// # Returns
    /// `Ok(Some(StoredCredentials))` if credentials exist for the service.
    /// `Ok(None)` if no credentials are stored for the service.
    /// `Err(anyhow::Error)` when the backing store cannot be read or parsed.
    fn get_credentials(&self, service_id: &str) -> Result<Option<StoredCredentials>>;

    /// Stores credentials for one configured service.
    ///
    /// # Arguments
    /// * `service_id` - Stable service identifier such as `m6play-fr`.
    /// * `credentials` - Credentials payload to persist.
    ///
    /// # Returns
    /// `Ok(())` on successful storage.
    /// `Err(anyhow::Error)` when the payload is invalid or when the backing store
    /// cannot be updated.
    fn set_credentials(&self, service_id: &str, credentials: StoredCredentials) -> Result<()>;

    /// Removes credentials for one configured service.
    ///
    /// # Arguments
    /// * `service_id` - Stable service identifier such as `m6play-fr`.
    ///
    /// # Returns
    /// `Ok(())` on successful removal.
    /// `Err(anyhow::Error)` when the backing store cannot be updated.
    fn clear_credentials(&self, service_id: &str) -> Result<()>;
}

/// Internal representation of a credentials document.
///
/// This struct represents the complete credentials storage format,
/// including version information and a map of service credentials.
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct CredentialsDocument {
    /// Document format version.
    ///
    /// This field is used to ensure compatibility when reading and writing
    /// credentials documents.
    #[serde(default = "current_document_version")]
    pub version: u32,
    /// Map of service identifiers to their credentials.
    ///
    /// This field contains all stored credentials, keyed by service identifier.
    #[serde(default)]
    pub services: HashMap<String, StoredCredentials>,
}

impl CredentialsDocument {
    /// Creates a new empty credentials document.
    ///
    /// # Returns
    /// A new CredentialsDocument with the current version and empty services map.
    pub fn new() -> Self {
        Self {
            version: current_document_version(),
            services: HashMap::new(),
        }
    }
}

/// Returns the current document version.
///
/// # Returns
/// The current credentials document format version.
fn current_document_version() -> u32 {
    CURRENT_DOCUMENT_VERSION
}

/// Normalizes a service identifier by trimming whitespace and validating.
///
/// # Arguments
/// * `service_id` - The service identifier to normalize.
///
/// # Returns
/// `Ok(String)` with the trimmed service identifier if non-empty.
/// `Err(anyhow::Error)` if the service identifier is empty after trimming.
pub(crate) fn normalize_service_id(service_id: &str) -> Result<String> {
    let normalized_service_id = service_id.trim().to_string();
    if normalized_service_id.is_empty() {
        bail!("Missing credentials service identifier.");
    }

    Ok(normalized_service_id)
}

/// Reads a file if it exists.
///
/// # Arguments
/// * `path` - The path to the file to read.
///
/// # Returns
/// `Ok(Some(bytes))` if the file exists and was read successfully.
/// `Ok(None)` if the file does not exist.
/// `Err(anyhow::Error)` if the file exists but cannot be read.
pub(crate) fn read_file_if_exists(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .with_context(|| format!("Failed to read credentials store `{}`.", path.display())),
    }
}

/// Removes a file if it exists.
///
/// # Arguments
/// * `path` - The path to the file to remove.
///
/// # Returns
/// `Ok(())` if the file was removed or doesn't exist.
/// `Err(anyhow::Error)` if the file exists but cannot be removed.
#[allow(dead_code)]
pub(crate) fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("Failed to delete credentials store `{}`.", path.display())),
    }
}
