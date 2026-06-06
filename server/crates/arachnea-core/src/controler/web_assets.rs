//! Shared frontend asset loading for controller backends.

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Binary frontend asset and its response metadata.
#[derive(Clone)]
pub(crate) struct WebAsset {
    pub bytes: Vec<u8>,
    pub mime_type: &'static str,
}

/// Embedded frontend assets provided by an application crate.
pub trait EmbeddedWebAssets: Send + Sync {
    /// Loads an embedded asset by normalized relative path.
    fn get(&self, path: &str) -> Option<Vec<u8>>;
}

/// Shared embedded frontend asset provider.
pub type SharedWebAssets = Arc<dyn EmbeddedWebAssets>;

/// Frontend asset source shared by the REST and Tauri controllers.
#[derive(Clone)]
pub(crate) enum WebAssetSource {
    Directory(PathBuf),
    Embedded(SharedWebAssets),
}

impl WebAssetSource {
    /// Creates a frontend asset source backed by a directory on disk.
    pub(crate) fn directory(directory_path: &str) -> Self {
        Self::Directory(PathBuf::from(directory_path))
    }

    /// Creates a frontend asset source backed by embedded application assets.
    pub(crate) fn embedded(assets: SharedWebAssets) -> Self {
        Self::Embedded(assets)
    }

    /// Loads a frontend asset from the source, applying SPA-style fallbacks.
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

    fn load_candidate(&self, candidate: &str) -> Result<Option<Vec<u8>>, String> {
        match self {
            Self::Directory(directory_path) => load_directory_asset(directory_path, candidate),
            Self::Embedded(assets) => Ok(assets.get(candidate)),
        }
    }
}

/// Normalizes a mount path by trimming leading and trailing slashes.
pub(crate) fn normalize_mount_path(path: &str) -> String {
    path.trim_matches('/').to_string()
}

/// Strips a controller mount path from a requested asset path.
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

fn decode_request_path(request_path: &str) -> Result<String, String> {
    let encoded_path = request_path
        .split(&['?', '#'][..])
        .next()
        .unwrap_or_default();

    urlencoding::decode(encoded_path)
        .map(Cow::into_owned)
        .map_err(|error| format!("Failed to decode web asset path `{encoded_path}`: {error}"))
}

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
