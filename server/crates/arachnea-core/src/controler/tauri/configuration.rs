//! Application-owned context and configuration for the Tauri controller.

use crate::{
    TAURI_CONTROLLER_DEFAULT_API_PREFIX as DEFAULT_TAURI_API_PREFIX,
    TAURI_CONTROLLER_DEFAULT_WEB_SCHEME as DEFAULT_TAURI_WEB_SCHEME,
};

/// Presentation used for public frontend links opened in a new browsing context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TauriBrowsingMode {
    /// Independent webview tabs within the configured native window.
    Tabs,
    /// Independent native windows inheriting the requesting window title.
    Windows,
}

/// Default presentation for new public frontend browsing contexts.
pub use crate::DEFAULT_TAURI_BROWSING_MODE;

/// Configuration required to build a Tauri controller service.
///
/// This struct holds all the configuration needed to create a Tauri controller,
/// including the Tauri context and customization options.
pub struct TauriControlerConfiguration {
    /// The Tauri context owned by the application crate.
    pub(super) context: ::tauri::Context<::tauri::Wry>,
    /// The custom URI scheme used to serve the frontend.
    pub(super) web_scheme: String,
    /// The API path prefix used by binary stream routes.
    pub(super) api_prefix: String,
    /// Presentation used for public frontend browsing contexts.
    pub(super) browsing_mode: TauriBrowsingMode,
}

impl TauriControlerConfiguration {
    /// Creates a Tauri controller configuration from an application-owned Tauri context.
    ///
    /// # Arguments
    /// * `context` - Generated Tauri context owned by the application crate.
    ///
    /// # Returns
    /// A new TauriControlerConfiguration with default settings.
    pub fn new(context: ::tauri::Context<::tauri::Wry>) -> Self {
        Self {
            context,
            web_scheme: DEFAULT_TAURI_WEB_SCHEME.to_string(),
            api_prefix: DEFAULT_TAURI_API_PREFIX.to_string(),
            browsing_mode: DEFAULT_TAURI_BROWSING_MODE,
        }
    }

    /// Selects tabs or independent native windows for public navigation.
    ///
    /// # Arguments
    /// * `mode` - Browsing presentation; defaults to [`DEFAULT_TAURI_BROWSING_MODE`].
    pub fn browsing_mode(mut self, mode: TauriBrowsingMode) -> Self {
        self.browsing_mode = mode;
        self
    }

    /// Sets the custom URI scheme used to serve the frontend.
    ///
    /// # Arguments
    /// * `web_scheme` - Custom protocol scheme registered with Tauri.
    ///
    /// # Returns
    /// The modified configuration for method chaining.
    pub fn web_scheme(mut self, web_scheme: impl Into<String>) -> Self {
        self.web_scheme = normalize_web_scheme(&web_scheme.into());
        self
    }

    /// Sets the API path prefix used by binary stream routes.
    ///
    /// # Arguments
    /// * `api_prefix` - Path prefix used before registered binary stream commands.
    ///
    /// # Returns
    /// The modified configuration for method chaining.
    pub fn api_prefix(mut self, api_prefix: impl Into<String>) -> Self {
        self.api_prefix = normalize_api_prefix(&api_prefix.into());
        self
    }
}

/// Normalizes a Tauri web scheme by removing protocol suffixes and slashes.
///
/// # Arguments
/// * `web_scheme` - The web scheme to normalize.
///
/// # Returns
/// The normalized scheme, or the default scheme if the input is empty.
fn normalize_web_scheme(web_scheme: &str) -> String {
    let normalized = web_scheme
        .trim()
        .trim_end_matches("://")
        .trim_end_matches('/')
        .to_string();

    if normalized.is_empty() {
        DEFAULT_TAURI_WEB_SCHEME.to_string()
    } else {
        normalized
    }
}

/// Normalizes a Tauri API prefix by trimming slashes and adding surrounding slashes.
///
/// # Arguments
/// * `api_prefix` - The API prefix to normalize.
///
/// # Returns
/// The normalized prefix with surrounding slashes, or the default prefix if empty.
fn normalize_api_prefix(api_prefix: &str) -> String {
    let normalized = api_prefix.trim().trim_matches('/');

    if normalized.is_empty() {
        DEFAULT_TAURI_API_PREFIX.to_string()
    } else {
        format!("/{normalized}/")
    }
}
