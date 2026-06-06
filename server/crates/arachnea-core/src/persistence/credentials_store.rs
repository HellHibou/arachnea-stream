use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

const CURRENT_DOCUMENT_VERSION: u32 = 1;

/// Credentials persisted for one backend service.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredCredentials {
    /// Service login identifier.
    pub login: String,
    /// Service password.
    pub password: String,
}

impl StoredCredentials {
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
#[allow(dead_code)]
pub trait CredentialsStore: Send + Sync {
    /// Loads credentials for one configured service.
    ///
    /// # Arguments
    /// * `service_id` - Stable service identifier such as `m6play-fr`.
    ///
    /// # Errors
    /// Returns an error when the backing store cannot be read or parsed.
    fn get_credentials(&self, service_id: &str) -> Result<Option<StoredCredentials>>;

    /// Stores credentials for one configured service.
    ///
    /// # Arguments
    /// * `service_id` - Stable service identifier such as `m6play-fr`.
    /// * `credentials` - Credentials payload to persist.
    ///
    /// # Errors
    /// Returns an error when the payload is invalid or when the backing store
    /// cannot be updated.
    fn set_credentials(&self, service_id: &str, credentials: StoredCredentials) -> Result<()>;

    /// Removes credentials for one configured service.
    ///
    /// # Arguments
    /// * `service_id` - Stable service identifier such as `m6play-fr`.
    ///
    /// # Errors
    /// Returns an error when the backing store cannot be updated.
    fn clear_credentials(&self, service_id: &str) -> Result<()>;
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct CredentialsDocument {
    #[serde(default = "current_document_version")]
    pub version: u32,
    #[serde(default)]
    pub services: HashMap<String, StoredCredentials>,
}

impl CredentialsDocument {
    pub fn new() -> Self {
        Self {
            version: current_document_version(),
            services: HashMap::new(),
        }
    }
}

fn current_document_version() -> u32 {
    CURRENT_DOCUMENT_VERSION
}

pub(crate) fn normalize_service_id(service_id: &str) -> Result<String> {
    let normalized_service_id = service_id.trim().to_string();
    if normalized_service_id.is_empty() {
        bail!("Missing credentials service identifier.");
    }

    Ok(normalized_service_id)
}

pub(crate) fn read_file_if_exists(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .with_context(|| format!("Failed to read credentials store `{}`.", path.display())),
    }
}

#[allow(dead_code)]
pub(crate) fn write_file_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent().filter(|value| !value.as_os_str().is_empty()) {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create credentials store directory `{}`.",
                parent.display()
            )
        })?;
    }

    let temporary_path = temporary_path_for(path)?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(&temporary_path).with_context(|| {
        format!(
            "Failed to open temporary credentials store `{}`.",
            temporary_path.display()
        )
    })?;
    file.write_all(bytes).with_context(|| {
        format!(
            "Failed to write temporary credentials store `{}`.",
            temporary_path.display()
        )
    })?;
    file.sync_all().with_context(|| {
        format!(
            "Failed to flush temporary credentials store `{}`.",
            temporary_path.display()
        )
    })?;
    drop(file);

    fs::rename(&temporary_path, path)
        .with_context(|| format!("Failed to replace credentials store `{}`.", path.display()))?;
    set_owner_only_permissions(path)?;

    Ok(())
}

#[allow(dead_code)]
pub(crate) fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("Failed to delete credentials store `{}`.", path.display())),
    }
}

#[allow(dead_code)]
fn temporary_path_for(path: &Path) -> Result<PathBuf> {
    let file_name = path.file_name().with_context(|| {
        format!(
            "Cannot derive a temporary credentials store path from `{}`.",
            path.display()
        )
    })?;

    let mut temporary_name = OsString::from(".");
    temporary_name.push(file_name);
    temporary_name.push(".tmp");

    Ok(path.with_file_name(temporary_name))
}

#[cfg(unix)]
#[allow(dead_code)]
fn set_owner_only_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).with_context(|| {
        format!(
            "Failed to restrict credentials store permissions for `{}`.",
            path.display()
        )
    })
}

#[cfg(not(unix))]
#[allow(dead_code)]
fn set_owner_only_permissions(_path: &Path) -> Result<()> {
    Ok(())
}
