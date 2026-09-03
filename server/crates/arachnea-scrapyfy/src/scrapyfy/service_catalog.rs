//! Read-only service catalog loading from recursive manifests and YAML files.

use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::{
    resolve_manifest_sources, ScraperQueryCollectionRaw, ScraperServiceCredentials,
    ScraperSourceDescriptor,
};

/// Metadata for one declared service, including services disabled by default.
#[derive(Clone, Debug)]
pub struct ScraperServiceCatalogEntry {
    /// Source activation descriptor.
    pub source: ScraperSourceDescriptor,
    /// Human-readable title, when declared.
    pub title: Option<String>,
    /// Logo URL/template, when declared.
    pub logo: Option<String>,
    /// Localized descriptions.
    pub description: std::collections::HashMap<String, String>,
    /// Optional account requirements.
    pub credentials: Option<ScraperServiceCredentials>,
}

/// Failure reason for one catalog source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceCatalogFailureReason {
    /// The YAML file declared by the manifest does not exist.
    Missing,
    /// The YAML file exists but could not be parsed.
    Invalid,
    /// Another source already declared the same YAML identifier.
    Duplicate,
}

/// One source that could not be added to the service catalog.
#[derive(Clone, Debug)]
pub struct ServiceCatalogFailure {
    /// YAML file path involved in the failure.
    pub path: PathBuf,
    /// Parsed identifier, when the YAML was readable far enough to provide one.
    pub id: Option<String>,
    /// Qualified failure reason.
    pub reason: ServiceCatalogFailureReason,
    /// Human-readable error message.
    pub message: String,
}

/// Outcome of a tolerant catalog load that keeps per-source failures.
#[derive(Clone, Debug, Default)]
pub struct ServiceCatalogLoad {
    /// Successfully parsed entries, including sources disabled by default.
    pub entries: Vec<ScraperServiceCatalogEntry>,
    /// Sources that could not be loaded, with their failure reason.
    pub failures: Vec<ServiceCatalogFailure>,
}

/// Loads every YAML source declared by a recursive services manifest.
///
/// # Arguments
/// * `config_path` - Path to the root services manifest JSON file.
/// * `service_store_id` - Service store (group) identifier owning the sources.
pub fn load_service_catalog(
    config_path: impl AsRef<Path>,
    service_store_id: &str,
) -> Result<Vec<ScraperServiceCatalogEntry>> {
    let config_path = config_path.as_ref();
    let config_path: PathBuf = if config_path.is_relative() {
        arachnea_core::application::get_application_resource_path(&config_path.to_string_lossy())
            .into()
    } else {
        config_path.to_path_buf()
    };
    let base = config_path.parent().unwrap_or(Path::new(""));
    let mut ids = HashSet::new();
    resolve_manifest_sources(&config_path)?
        .into_iter()
        .map(|source| {
            let path = base.join(&source.path);
            let file = std::fs::File::open(&path)
                .with_context(|| format!("Failed to open service YAML {}.", path.display()))?;
            let raw: ScraperQueryCollectionRaw = serde_yaml::from_reader(file)
                .with_context(|| format!("Failed to parse service YAML {}.", path.display()))?;
            if !ids.insert(raw.id.clone()) {
                bail!("Duplicate service id {}.", raw.id);
            }
            Ok(ScraperServiceCatalogEntry {
                source: ScraperSourceDescriptor {
                    service_store_id: service_store_id.to_string(),
                    id: raw.id,
                    path,
                    default_enabled: source.enabled,
                    parameters: source.parameters,
                },
                title: raw.title,
                logo: raw.logo,
                description: raw.description,
                credentials: raw.credentials,
            })
        })
        .collect()
}

/// Loads every YAML source declared by a recursive services manifest, keeping
/// per-source failures instead of aborting on the first broken entry.
///
/// Manifest resolution errors (unreadable or invalid manifest) remain fatal.
///
/// # Arguments
/// * `config_path` - Path to the root services manifest JSON file.
/// * `service_store_id` - Service store (group) identifier owning the sources.
///
/// # Returns
/// A [`ServiceCatalogLoad`] with the successfully parsed entries and the
/// qualified failures for the remaining sources.
///
/// # Errors
/// Returns an error when the manifest itself cannot be resolved.
pub fn load_service_catalog_detailed(
    config_path: impl AsRef<Path>,
    service_store_id: &str,
) -> Result<ServiceCatalogLoad> {
    let config_path = config_path.as_ref();
    let config_path: PathBuf = if config_path.is_relative() {
        arachnea_core::application::get_application_resource_path(&config_path.to_string_lossy())
            .into()
    } else {
        config_path.to_path_buf()
    };
    let base = config_path.parent().unwrap_or(Path::new(""));
    let sources = resolve_manifest_sources(&config_path)?;
    let mut load = ServiceCatalogLoad::default();
    let mut ids = HashSet::new();

    for source in sources {
        let path = base.join(&source.path);
        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                load.failures.push(ServiceCatalogFailure {
                    path: path.clone(),
                    id: None,
                    reason: ServiceCatalogFailureReason::Missing,
                    message: format!("Service YAML {} does not exist.", path.display()),
                });
                continue;
            }
            Err(error) => {
                load.failures.push(ServiceCatalogFailure {
                    path: path.clone(),
                    id: None,
                    reason: ServiceCatalogFailureReason::Invalid,
                    message: format!("Failed to open service YAML {}: {error}.", path.display()),
                });
                continue;
            }
        };
        let raw: ScraperQueryCollectionRaw = match serde_yaml::from_reader(file) {
            Ok(raw) => raw,
            Err(error) => {
                load.failures.push(ServiceCatalogFailure {
                    path: path.clone(),
                    id: None,
                    reason: ServiceCatalogFailureReason::Invalid,
                    message: format!("Failed to parse service YAML {}: {error}.", path.display()),
                });
                continue;
            }
        };
        if !ids.insert(raw.id.clone()) {
            load.failures.push(ServiceCatalogFailure {
                path: path.clone(),
                id: Some(raw.id.clone()),
                reason: ServiceCatalogFailureReason::Duplicate,
                message: format!("Duplicate service id {}.", raw.id),
            });
            continue;
        }
        load.entries.push(ScraperServiceCatalogEntry {
            source: ScraperSourceDescriptor {
                service_store_id: service_store_id.to_string(),
                id: raw.id,
                path,
                default_enabled: source.enabled,
                parameters: source.parameters,
            },
            title: raw.title,
            logo: raw.logo,
            description: raw.description,
            credentials: raw.credentials,
        });
    }

    Ok(load)
}
