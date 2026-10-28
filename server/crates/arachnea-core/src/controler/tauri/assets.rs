//! Tauri embedded providers and scoped frontend asset mounts.

use crate::controler::web_assets::{
    is_embedded_web_asset_source, normalize_mount_path, scope_web_asset_source, strip_mount_path,
    EmbeddedWebAssets, SharedWebAssets, WebAssetSource,
};
use ::tauri::{utils::assets::AssetKey, Assets};
use std::{borrow::Cow, sync::Arc};

/// Embedded web assets backed by a Tauri asset bundle.
///
/// This struct implements the EmbeddedWebAssets trait for Tauri's built-in
/// asset system.
pub struct TauriEmbeddedWebAssets {
    /// The Tauri asset bundle owned by the application crate.
    assets: Arc<dyn Assets<::tauri::Wry>>,
}

impl TauriEmbeddedWebAssets {
    /// Creates an embedded web asset provider from Tauri generated assets.
    ///
    /// # Arguments
    /// * `assets` - Generated Tauri asset bundle owned by the application crate.
    ///
    /// # Returns
    /// A new TauriEmbeddedWebAssets instance.
    pub fn new(assets: Arc<dyn Assets<::tauri::Wry>>) -> Self {
        Self { assets }
    }
}

impl EmbeddedWebAssets for TauriEmbeddedWebAssets {
    /// Loads an embedded asset by normalized relative path.
    ///
    /// # Arguments
    /// * `path` - The normalized relative path to the asset.
    ///
    /// # Returns
    /// `Some(Vec<u8>)` containing the asset bytes if found.
    /// `None` if the asset does not exist.
    fn get(&self, path: &str) -> Option<Vec<u8>> {
        let key: AssetKey = path.into();
        self.assets.get(&key).map(Cow::into_owned)
    }
}

/// Internal representation of Tauri web assets configuration.
#[derive(Clone)]
pub(super) struct TauriWebAssets {
    /// The normalized mount path for these assets.
    pub(super) mount_path: String,
    /// The source from which to load the assets.
    source: WebAssetSource,
}

impl TauriWebAssets {
    /// Creates Tauri web assets from a directory on disk.
    ///
    /// # Arguments
    /// * `directory_path` - Directory containing the frontend assets.
    /// * `path` - Relative path under the Tauri root where assets should be mounted.
    ///
    /// # Returns
    /// A new TauriWebAssets instance configured for directory loading.
    pub(super) fn from_directory(directory_path: &str, path: &str) -> Self {
        let mount_path = normalize_mount_path(path);
        Self {
            mount_path: mount_path.clone(),
            source: scope_web_asset_source(WebAssetSource::directory(directory_path), &mount_path),
        }
    }

    /// Creates Tauri web assets from embedded resources.
    ///
    /// # Arguments
    /// * `assets` - Embedded frontend asset provider.
    /// * `path` - Relative path under the Tauri root where assets should be mounted.
    ///
    /// # Returns
    /// A new TauriWebAssets instance configured for embedded loading.
    pub(super) fn from_embedded(assets: SharedWebAssets, path: &str) -> Self {
        let mount_path = normalize_mount_path(path);
        Self {
            mount_path: mount_path.clone(),
            source: scope_web_asset_source(WebAssetSource::embedded(assets), &mount_path),
        }
    }

    /// Returns whether this mount serves the given request path.
    ///
    /// The root mount (empty path) serves every request; a scoped mount only
    /// serves requests whose path is below its mount segment, so a bundle
    /// mounted at `/admin/` never shadows sibling routes.
    ///
    /// # Arguments
    /// * `request_path` - The normalized request path starting with `/`.
    ///
    /// # Returns
    /// `true` when this mount is a candidate for the request.
    pub(super) fn matches(&self, request_path: &str) -> bool {
        if self.mount_path.is_empty() {
            return true;
        }
        request_path == format!("/{}", self.mount_path)
            || request_path.starts_with(&format!("/{}/", self.mount_path))
    }

    /// Generates the window URL for these web assets.
    ///
    /// # Arguments
    /// * `web_scheme` - The custom URI scheme to use.
    ///
    /// # Returns
    /// The URL string for the Tauri window configuration.
    pub(super) fn window_url(&self, web_scheme: &str) -> String {
        if self.mount_path.is_empty() {
            format!("{web_scheme}://localhost/")
        } else {
            format!("{web_scheme}://localhost/{}/", self.mount_path)
        }
    }

    /// Loads a web asset from a request path.
    ///
    /// # Arguments
    /// * `request_path` - The requested asset path.
    ///
    /// # Returns
    /// `Ok(WebAsset)` if the asset is found.
    /// `Err(String)` if the asset cannot be found.
    pub(super) fn load_request(
        &self,
        request_path: &str,
    ) -> Result<crate::controler::web_assets::WebAsset, String> {
        let relative_path = strip_mount_path(request_path, &self.mount_path).ok_or_else(|| {
            format!(
                "Web asset not found: {}",
                request_path.trim_start_matches('/')
            )
        })?;
        self.source.load(&relative_path)
    }

    /// Reports whether this mount serves conditional validators.
    ///
    /// # Returns
    /// `true` for embedded bundles, `false` for directory sources (mutable).
    pub(super) fn is_embedded(&self) -> bool {
        is_embedded_web_asset_source(&self.source)
    }
}
