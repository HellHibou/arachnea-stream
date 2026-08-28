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

/// Loads every YAML source declared by a recursive services manifest.
pub fn load_service_catalog(
    config_path: impl AsRef<Path>,
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
