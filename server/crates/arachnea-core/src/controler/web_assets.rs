//! Shared frontend asset loading for controller backends.

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Binary frontend asset and its response metadata.
///
/// This struct represents a loaded frontend asset with its binary content
/// and the appropriate MIME type for HTTP responses.
#[derive(Clone)]
pub(crate) struct WebAsset {
    /// The binary content of the asset.
    pub bytes: Vec<u8>,
    /// The MIME type for the asset.
    pub mime_type: &'static str,
}

/// Embedded frontend assets provided by an application crate.
///
/// This trait defines the interface for accessing frontend assets that are
/// embedded in the application binary.
pub trait EmbeddedWebAssets: Send + Sync {
    /// Loads an embedded asset by normalized relative path.
    ///
    /// # Arguments
    /// * `path` - The normalized relative path to the asset.
    ///
    /// # Returns
    /// `Some(Vec<u8>)` containing the asset bytes if found.
    /// `None` if the asset does not exist.
    fn get(&self, path: &str) -> Option<Vec<u8>>;
}

/// Shared embedded frontend asset provider.
///
/// This type alias represents a reference-counted trait object for embedded
/// web assets, allowing the asset provider to be shared across multiple
/// parts of the application.
pub type SharedWebAssets = Arc<dyn EmbeddedWebAssets>;

/// Web asset provider scoped to a subdirectory of an outer asset pool.
///
/// Used to mount one bundle embedded inside a shared asset pool: the release
/// build embeds every frontend under the configured `frontendDist` root, so
/// the admin bundle lives under `admin/` inside the same pool. Paths are
/// translated into the outer pool and the SPA fallback stays inside the
/// scoped bundle (its own `index.html`) instead of leaking to the outer root
/// document.
pub(crate) struct ScopedWebAssets {
    /// Outer asset pool containing the scoped bundle.
    outer: SharedWebAssets,
    /// Normalized subdirectory of the outer pool holding the bundle.
    asset_root: String,
}

impl ScopedWebAssets {
    /// Creates a scoped asset provider over an outer pool.
    ///
    /// # Arguments
    /// * `outer` - Asset pool containing the scoped bundle.
    /// * `asset_root` - Subdirectory of the outer pool holding the bundle.
    ///
    /// # Returns
    /// A scoped embedded asset provider.
    pub(crate) fn new(outer: SharedWebAssets, asset_root: &str) -> Self {
        Self {
            outer,
            asset_root: normalize_mount_path(asset_root),
        }
    }
}

impl EmbeddedWebAssets for ScopedWebAssets {
    /// Loads an asset from the scoped bundle inside the outer pool.
    ///
    /// # Arguments
    /// * `path` - The normalized relative path inside the scoped bundle.
    ///
    /// # Returns
    /// `Some(Vec<u8>)` containing the asset bytes if found.
    /// `None` if the asset does not exist inside the scoped bundle.
    fn get(&self, path: &str) -> Option<Vec<u8>> {
        let path = path.trim_start_matches('/');
        if path.is_empty() || self.asset_root.is_empty() {
            return None;
        }
        let sanitized = sanitize_relative_path(path).ok()?;
        let mut key = self.asset_root.clone();
        for segment in sanitized.components() {
            let segment = segment.as_os_str().to_str()?;
            key.push('/');
            key.push_str(segment);
        }
        self.outer.get(&key)
    }
}

/// Frontend asset source shared by the REST and Tauri controllers.
///
/// This enum represents the different sources from which frontend assets
/// can be loaded.
#[derive(Clone)]
pub(crate) enum WebAssetSource {
    /// Assets loaded from a directory on disk.
    Directory(PathBuf),
    /// Assets loaded from embedded resources.
    Embedded(SharedWebAssets),
}

impl WebAssetSource {
    /// Creates a frontend asset source backed by a directory on disk.
    ///
    /// # Arguments
    /// * `directory_path` - The path to the directory containing the assets.
    ///
    /// # Returns
    /// A WebAssetSource that loads assets from the specified directory.
    pub(crate) fn directory(directory_path: &str) -> Self {
        Self::Directory(PathBuf::from(directory_path))
    }

    /// Creates a frontend asset source backed by embedded application assets.
    ///
    /// # Arguments
    /// * `assets` - The embedded asset provider.
    ///
    /// # Returns
    /// A WebAssetSource that loads assets from embedded resources.
    pub(crate) fn embedded(assets: SharedWebAssets) -> Self {
        Self::Embedded(assets)
    }

    /// Loads a frontend asset from the source, applying SPA-style fallbacks.
    ///
    /// This method tries multiple candidate paths for Single Page Application
    /// routing (e.g., trying `index.html` when a specific path isn't found).
    ///
    /// # Arguments
    /// * `request_path` - The requested asset path.
    ///
    /// # Returns
    /// `Ok(WebAsset)` if the asset is found.
    /// `Err(String)` if the asset cannot be found.
    pub(crate) fn load(&self, request_path: &str) -> Result<WebAsset, String> {
        let request_path = decode_request_path(request_path)?;
        let request_path = request_path.trim_start_matches('/');

        for candidate in asset_candidates(request_path) {
            if let Some(bytes) = self.load_candidate(&candidate)? {
                return Ok(WebAsset {
                    mime_type: mime_type_for_path(&candidate),
                    bytes,
                });
            }
        }

        Err(format!("Web asset not found: {}", request_path))
    }

    /// Loads a specific asset candidate from the source.
    ///
    /// # Arguments
    /// * `candidate` - The candidate path to try loading.
    ///
    /// # Returns
    /// `Ok(Some(bytes))` if the asset exists.
    /// `Ok(None)` if the asset doesn't exist.
    /// `Err(String)` if there was an error loading the asset.
    fn load_candidate(&self, candidate: &str) -> Result<Option<Vec<u8>>, String> {
        match self {
            Self::Directory(directory_path) => load_directory_asset(directory_path, candidate),
            Self::Embedded(assets) => Ok(assets.get(candidate)),
        }
    }
}

/// Scopes an embedded asset source to a subdirectory mount inside the pool.
///
/// Embedded bundles mounted below the root live inside the shared asset pool
/// under the mount path (e.g. `admin/`). Scoping the provider keeps asset
/// lookup and the SPA fallback inside the bundle instead of falling back to
/// the root bundle document. Directory sources and root mounts are returned
/// unchanged (directory sources already resolve their own subtree).
///
/// # Arguments
/// * `source` - The web asset source to scope.
/// * `mount_path` - The mount path being served (already normalized).
///
/// # Returns
/// The scoped source when applicable, otherwise the original source.
pub(crate) fn scope_web_asset_source(source: WebAssetSource, mount_path: &str) -> WebAssetSource {
    let mount_path = normalize_mount_path(mount_path);
    match source {
        WebAssetSource::Embedded(assets) if !mount_path.is_empty() => {
            WebAssetSource::embedded(Arc::new(ScopedWebAssets::new(assets, &mount_path)))
        }
        other => other,
    }
}

/// Normalizes a mount path by trimming leading and trailing slashes.
///
/// # Arguments
/// * `path` - The mount path to normalize.
///
/// # Returns
/// The normalized path without leading or trailing slashes.
///
/// # Examples
///
/// ```text
/// assert_eq!(normalize_mount_path("/api/"), "api");
/// assert_eq!(normalize_mount_path("static"), "static");
/// ```text
pub(crate) fn normalize_mount_path(path: &str) -> String {
    path.trim_matches('/').to_string()
}

/// Strips a controller mount path from a requested asset path.
///
/// # Arguments
/// * `request_path` - The full requested path.
/// * `mount_path` - The mount path to strip.
///
/// # Returns
/// `Some(String)` containing the path after the mount path, or `None` if
/// the request path doesn't start with the mount path.
///
/// # Examples
///
/// ```text
/// assert_eq!(strip_mount_path("/api/assets/style.css", "api"), Some("assets/style.css".to_string()));
/// assert_eq!(strip_mount_path("/assets/style.css", "api"), None);
/// ```text
pub(crate) fn strip_mount_path(request_path: &str, mount_path: &str) -> Option<String> {
    let request_path = request_path.trim_start_matches('/');
    let mount_path = normalize_mount_path(mount_path);

    if mount_path.is_empty() {
        Some(request_path.to_string())
    } else if request_path == mount_path {
        Some(String::new())
    } else {
        request_path
            .strip_prefix(&(mount_path + "/"))
            .map(ToString::to_string)
    }
}

/// Decodes a URL-encoded request path.
///
/// # Arguments
/// * `request_path` - The URL-encoded request path.
///
/// # Returns
/// `Ok(String)` containing the decoded path.
/// `Err(String)` if the path cannot be decoded.
fn decode_request_path(request_path: &str) -> Result<String, String> {
    let encoded_path = request_path
        .split(&['?', '#'][..])
        .next()
        .unwrap_or_default();

    urlencoding::decode(encoded_path)
        .map(Cow::into_owned)
        .map_err(|error| format!("Failed to decode web asset path `{encoded_path}`: {error}"))
}

/// Generates candidate paths for SPA-style asset loading.
///
/// This function generates multiple candidate paths to try when loading an asset,
/// following Single Page Application conventions.
///
/// # Arguments
/// * `request_path` - The original request path.
///
/// # Returns
/// A vector of candidate paths to try in order.
///
/// # Examples
///
/// ```text
/// let candidates = asset_candidates("about");
/// assert_eq!(candidates, vec!["about", "about.html", "about/index.html", "index.html"]);
/// ```
fn asset_candidates(request_path: &str) -> Vec<String> {
    let request_path = request_path.trim_matches('/');

    if request_path.is_empty() {
        return vec!["index.html".to_string()];
    }

    let mut candidates = vec![request_path.to_string()];
    if Path::new(request_path).extension().is_none() {
        candidates.push(format!("{request_path}.html"));
        candidates.push(format!("{request_path}/index.html"));
    }
    candidates.push("index.html".to_string());
    candidates
}

/// Loads an asset from a directory on disk.
///
/// # Arguments
/// * `directory_path` - The base directory to load from.
/// * `candidate` - The candidate path to try.
///
/// # Returns
/// `Ok(Some(bytes))` if the asset exists and was read successfully.
/// `Ok(None)` if the asset doesn't exist.
/// `Err(String)` if there was an error reading the asset.
fn load_directory_asset(directory_path: &Path, candidate: &str) -> Result<Option<Vec<u8>>, String> {
    let candidate_path = sanitize_relative_path(candidate)?;
    let full_path = directory_path.join(candidate_path);

    match std::fs::read(&full_path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "Failed to read web asset `{}`: {}",
            full_path.display(),
            error
        )),
    }
}

/// Sanitizes a relative path to prevent directory traversal attacks.
///
/// # Arguments
/// * `path` - The path to sanitize.
///
/// # Returns
/// `Ok(PathBuf)` containing the sanitized path.
/// `Err(String)` if the path contains invalid components.
///
/// # Examples
///
/// ```text
/// let sanitized = sanitize_relative_path("assets/../config");
/// assert!(sanitized.is_err()); // Contains ParentDir
/// ```
fn sanitize_relative_path(path: &str) -> Result<PathBuf, String> {
    let mut sanitized = PathBuf::new();

    for component in Path::new(path).components() {
        match component {
            Component::Normal(segment) => sanitized.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Invalid web asset path: {}", path));
            }
        }
    }

    Ok(sanitized)
}

/// Literal `<base href>` marker used in `index.html`.
///
/// Controller backends that know the runtime mount point replace this marker
/// with the actual base path before serving the document.
pub(crate) const HTML_BASE_PLACEHOLDER: &str = "{base}";

/// Replaces the `<base href="{base}">` marker with the supplied runtime base.
///
/// Only HTML documents that actually contain the exact `href="{base}"` marker
/// are modified; every other asset is returned unchanged.
///
/// # Arguments
/// * `html` - The HTML document bytes.
/// * `base` - The runtime base path to substitute (e.g. `/entrypoint/`, `/`, or `./`).
///
/// # Returns
/// The updated HTML bytes, or the original bytes when the marker is absent.
pub(crate) fn replace_html_base(html: Vec<u8>, base: &str) -> Vec<u8> {
    let document = match String::from_utf8(html) {
        Ok(document) => document,
        Err(bytes) => return bytes.into_bytes(),
    };

    let needle = format!("href=\"{HTML_BASE_PLACEHOLDER}\"");
    let replacement = format!("href=\"{base}\"");
    if !document.contains(&needle) {
        return document.into_bytes();
    }

    document.replace(&needle, &replacement).into_bytes()
}

/// Determines the MIME type for a file based on its extension.
///
/// # Arguments
/// * `path` - The file path to determine the MIME type for.
///
/// # Returns
/// The appropriate MIME type string for the file extension.
///
/// # Examples
///
/// ```text
/// assert_eq!(mime_type_for_path("style.css"), "text/css; charset=utf-8");
/// assert_eq!(mime_type_for_path("image.png"), "image/png");
/// assert_eq!(mime_type_for_path("unknown.xyz"), "application/octet-stream");
/// ```
fn mime_type_for_path(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
    {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "eot" => "application/vnd.ms-fontobject",
        "map" => "application/json; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
