//! Persistent application configuration for the Arachnéa Stream executable.

use anyhow::{bail, Context, Result};
use arachnea_core::application;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

/// Current on-disk configuration format version.
const CONFIGURATION_VERSION: u32 = 1;
/// Relative configuration path in the writable application data directory.
pub const CONFIGURATION_PATH: &str = "data/config.json";

/// Settings persisted independently from command-line overrides.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ApplicationConfiguration {
    /// On-disk schema version.
    pub format_version: u32,
    /// Optional REST listener port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_port: Option<u16>,
    /// Optional public root path prefix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoint_root: Option<String>,
    /// Optional network mode: local, private, or public.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_mode: Option<String>,
    /// Argon2id encoded administrator password hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,
}

impl ApplicationConfiguration {
    /// Loads and validates the application configuration.
    ///
    /// A missing file represents the default configuration.
    pub fn load() -> Result<Self> {
        let path = configuration_path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to read application configuration {}.",
                        path.display()
                    )
                })
            }
        };
        let configuration: Self = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "Failed to parse application configuration {}.",
                path.display()
            )
        })?;
        configuration.validate()?;
        Ok(configuration)
    }

    /// Validates and writes the configuration atomically.
    pub fn save(&self) -> Result<()> {
        self.validate()?;
        let path = configuration_path();
        let parent = path
            .parent()
            .context("Application configuration path has no parent.")?;
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create configuration directory {}.",
                parent.display()
            )
        })?;
        let bytes = serde_json::to_vec_pretty(&self.clone().normalized())
            .context("Failed to serialize application configuration.")?;
        let temporary_path = path.with_file_name(".config.json.tmp");
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary_path).with_context(|| {
            format!(
                "Failed to create temporary configuration {}.",
                temporary_path.display()
            )
        })?;
        file.write_all(&bytes).with_context(|| {
            format!(
                "Failed to write temporary configuration {}.",
                temporary_path.display()
            )
        })?;
        file.sync_all().with_context(|| {
            format!(
                "Failed to flush temporary configuration {}.",
                temporary_path.display()
            )
        })?;
        drop(file);
        fs::rename(&temporary_path, &path).with_context(|| {
            format!(
                "Failed to replace application configuration {}.",
                path.display()
            )
        })?;
        set_owner_only_permissions(&path)?;
        Ok(())
    }

    /// Validates all configured values before applying or persisting them.
    pub fn validate(&self) -> Result<()> {
        if self.format_version != 0 && self.format_version != CONFIGURATION_VERSION {
            bail!(
                "Unsupported application configuration version {}.",
                self.format_version
            );
        }
        if self.server_port == Some(0) {
            bail!("Application configuration server port cannot be zero.");
        }
        if let Some(root) = self.entrypoint_root.as_deref() {
            validate_entrypoint_root(root)?;
        }
        if let Some(network_mode) = self.network_mode.as_deref() {
            if !matches!(network_mode, "local" | "private" | "public") {
                bail!(
                    "Invalid application configuration network mode {}.",
                    network_mode
                );
            }
        }
        if self.password_hash.as_deref().is_some_and(str::is_empty) {
            bail!("Application configuration password hash cannot be empty.");
        }
        Ok(())
    }

    /// Returns the configuration version normalized for writing.
    pub fn normalized(mut self) -> Self {
        self.format_version = CONFIGURATION_VERSION;
        self
    }
}

/// Returns the absolute configuration path.
pub fn configuration_path() -> PathBuf {
    PathBuf::from(application::get_application_data_path(CONFIGURATION_PATH))
}

fn validate_entrypoint_root(root: &str) -> Result<()> {
    if root.contains('?') || root.contains('#') {
        bail!("Application configuration entrypoint root cannot contain a query or fragment.");
    }
    if root.split('/').any(|segment| matches!(segment, "." | "..")) {
        bail!("Application configuration entrypoint root cannot contain dot segments.");
    }
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).with_context(|| {
        format!(
            "Failed to restrict configuration permissions for {}.",
            path.display()
        )
    })
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &Path) -> Result<()> {
    Ok(())
}
