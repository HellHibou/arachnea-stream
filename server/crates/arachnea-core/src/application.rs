//! Application resource path resolution helpers.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::path::Path;

static APP_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Returns the application root directory as a string slice.
///
/// In debug builds, this is the server workspace directory. In release builds,
/// this is the directory containing the current executable.
///
/// # Returns
///
/// A cached, process-wide application root path.
///
/// # Panics
///
/// Panics if the cached application root cannot be represented as valid UTF-8.
///
/// # Examples
///
/// ```
/// let root = arachnea_core::applicatio::get_application_root();
/// println!("Application root: {}", root);
/// ```
pub fn get_application_root() -> &'static str {
    APP_ROOT.get_or_init(init_app_root).to_str().unwrap()
}

/// Builds an absolute path from the application root and a relative path.
///
/// # Arguments
///
/// * `path` - Path segment to append to the application root.
///
/// # Returns
///
/// The joined path as an owned string, using lossy UTF-8 conversion if needed.
///
/// # Examples
///
/// ```
/// let data_path = arachnea_core::application::get_application_path("data/config.json");
/// println!("Config path: {}", data_path);
/// ```
pub fn get_application_path(path: &str) -> String {
    APP_ROOT
        .get_or_init(init_app_root)
        .join(path)
        .to_string_lossy()
        .into_owned()
}

/// Resolves the application root path for the current build profile.
///
/// # Returns
///
/// The server workspace directory in debug builds, or the executable directory in
/// release builds.
///
/// # Panics
///
/// Panics in release builds if the current executable path cannot be read.
/// Panics in debug builds if the server application root cannot be resolved.
fn init_app_root() -> PathBuf {
    if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("Failed to resolve the server application root.")
            .to_path_buf()
    } else {
        let mut path = std::env::current_exe().expect("Failed to get current executable path");

        path.pop();
        path
    }
}


/// Returns the executable name used in help and error messages.
///
/// Derives the name from the first command-line argument (`args_os(0)`),
/// extracting the file name component when available.
///
/// # Returns
///
/// The executable name as an owned `String`.
pub fn program_name() -> String {
    std::env::args_os()
        .next()
        .and_then(|arg0| {
            let path = Path::new(&arg0);

            path.file_name()
                .map(|file_name| file_name.to_string_lossy().into_owned())
                .or_else(|| {
                    let raw = arg0.to_string_lossy().into_owned();
                    (!raw.is_empty()).then_some(raw)
                })
        })
        .unwrap_or_else(|| String::from("?"))
}
