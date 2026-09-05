use std::{collections::BTreeMap, path::Path, sync::Arc};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    application::{ApplicationOptionDefinition, ApplicationOptionsProvider},
    controler::ServerTrayFactory,
    persistence::{set_owner_only_permissions, JsonPersistenceFileCodec, PersistenceFileCodec},
};

/// Default port used by the REST controller when no CLI override is provided.
pub const DEFAULT_SERVER_PORT: u16 = 8080;

/// Custom URI scheme used by the desktop frontend.
pub const DEFAULT_TAURI_WEB_SCHEME: &str = "arachnea-core";

/// API prefix used by the desktop binary stream routes.
pub const DEFAULT_TAURI_API_PREFIX: &str = "api";

/// Runtime mode the application controller runs under.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ApplicationMode {
    /// Desktop application mode backed by the Tauri controller.
    Desktop,

    /// Headless HTTP server mode backed by the REST controller.
    Server,
}

impl Default for ApplicationMode {
    fn default() -> Self {
        return ApplicationMode::Server;
    }
}

/// Provenance of one effective runtime setting.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingSource {
    /// Explicit command-line argument.
    CommandLine,
    /// Persisted application configuration file.
    Configuration,
    /// Built-in default.
    #[default]
    Default,
}

/// Which client connections the REST server accepts.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ServerNetworkMode {
    /// Loopback only: the server binds to `127.0.0.1`.
    Local,

    /// Loopback plus the machine's local networks, detected from the interface
    /// netmasks. The server binds to all interfaces and rejects clients whose
    /// address does not belong to a local network range.
    #[default]
    Private,

    /// Any network: the server binds to all interfaces and accepts every client.
    Public,
}

impl std::str::FromStr for ServerNetworkMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "local" => Ok(Self::Local),
            "private" => Ok(Self::Private),
            "public" => Ok(Self::Public),
            _ => Err(format!(
                "expected one of `local`, `private` or `public`, got `{value}`"
            )),
        }
    }
}

impl std::fmt::Display for ServerNetworkMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Local => "local",
            Self::Private => "private",
            Self::Public => "public",
        })
    }
}

/// Default runtime mode used when no CLI mode flag is provided.
///
/// In release builds the backend defaults to server mode; in debug builds it
/// defaults to desktop mode.

/// Runtime and persisted options used by the application controller.
///
/// Only the server settings annotated with Serde are persisted. Controller,
/// desktop, tray and web-asset settings remain runtime-only.
#[derive(Clone, Deserialize, Serialize)]
pub struct CoreApplicationOptions {
    /// Backend mode (desktop or server).
    #[serde(skip)]
    pub application_mode: Option<ApplicationMode>,

    /// REST server port used in server mode.
    ///
    /// Falls back to [`DEFAULT_SERVER_PORT`] when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_port: Option<u16>,

    /// Where the effective REST server port comes from.
    #[serde(skip)]
    pub server_port_source: SettingSource,

    /// Which client connections the REST server accepts.
    ///
    /// `None` means the built-in default ([`ServerNetworkMode::Private`]);
    /// only an explicit `--network` flag or a persisted configuration entry
    /// fills this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_mode: Option<ServerNetworkMode>,

    /// Where the effective REST network mode comes from.
    #[serde(skip)]
    pub network_mode_source: SettingSource,

    /// Optional public root path prefix for server mode.
    ///
    /// When set to a non-root prefix, a bare `GET /` on the REST server answers
    /// a `302 Found` redirect to the mounted application path (for example
    /// `/prefix/`) instead of `404 Not Found`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoint_root: Option<String>,

    /// Where the effective public root path comes from.
    #[serde(skip)]
    pub entrypoint_root_source: SettingSource,

    /// Argon2id encoded administrator password hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,

    /// Optional public API path segment for server mode.
    #[serde(skip)]
    pub entrypoint_api: Option<String>,

    /// Custom URI scheme used by the desktop frontend.
    ///
    /// Falls back to [`DEFAULT_TAURI_WEB_SCHEME`] when `None`.
    #[serde(skip)]
    pub web_scheme: Option<String>,

    /// API path prefix used by the desktop binary stream routes.
    ///
    /// Falls back to [`DEFAULT_TAURI_API_PREFIX`] when `None`.
    #[serde(skip)]
    pub api_prefix: Option<String>,

    /// Optional factory creating the server tray icon in server mode.
    ///
    /// When set and a GUI is available, the REST controller shows a tray icon.
    #[serde(skip)]
    pub server_tray_factory: Option<Arc<dyn ServerTrayFactory>>,

    /// Whether a server tray icon should be shown in server mode.
    ///
    /// When `true` a tray factory is installed and the REST controller shows a
    /// tray icon if a GUI is available. Set to `false` to force-disable the tray
    /// even on a graphical environment.
    #[serde(skip)]
    pub tray_enabled: bool,

    /// Callback reloading the application configuration from the server tray.
    ///
    /// Invoked on a background thread by the tray "Reload configuration"
    /// action; the returned summary is written to the application log.
    #[serde(skip)]
    pub reload_configuration: Option<Arc<dyn Fn() -> String + Send + Sync>>,

    /// Additional web-asset mount paths served below the application root.
    ///
    /// Each path mounts the shared embedded asset pool scoped to that
    /// subdirectory (e.g. `admin` to serve the administration bundle at
    /// `/admin/`). Dedicated mounts take precedence over the root bundle.
    #[serde(skip)]
    pub web_mount_paths: Vec<String>,
}

impl CoreApplicationOptions {
    /// Creates runtime options for the application controller.
    ///
    /// # Arguments
    /// * `application_mode` - Backend mode (desktop or server).
    /// * `server_port` - Optional REST server port used in server mode.
    /// * `entrypoint_root` - Optional public root path prefix for server mode.
    /// * `entrypoint_api` - Optional public API path segment for server mode.
    /// * `web_scheme` - Optional custom URI scheme for the desktop frontend.
    /// * `api_prefix` - Optional API path prefix for desktop binary stream routes.
    pub fn new(
        application_mode: Option<ApplicationMode>,
        server_port: Option<u16>,
        entrypoint_root: Option<String>,
        entrypoint_api: Option<String>,
        web_scheme: Option<String>,
        api_prefix: Option<String>,
    ) -> Self {
        Self {
            application_mode,
            server_port,
            server_port_source: SettingSource::Default,
            network_mode: None,
            network_mode_source: SettingSource::Default,
            entrypoint_root,
            entrypoint_root_source: SettingSource::Default,
            password_hash: None,
            entrypoint_api,
            web_scheme,
            api_prefix,
            server_tray_factory: None,
            tray_enabled: true,
            reload_configuration: None,
            web_mount_paths: Vec::new(),
        }
    }

    /// Sets the factory creating the server tray icon in server mode.
    ///
    /// # Arguments
    /// * `server_tray_factory` - Application-supplied tray factory.
    ///
    /// # Returns
    /// The modified options for chaining.
    pub fn with_server_tray_factory(
        mut self,
        server_tray_factory: Arc<dyn ServerTrayFactory>,
    ) -> Self {
        self.server_tray_factory = Some(server_tray_factory);
        self
    }

    /// Sets whether a server tray icon should be shown in server mode.
    ///
    /// # Arguments
    /// * `tray_enabled` - `true` to show a tray icon when a GUI is available,
    ///   `false` to force-disable it.
    ///
    /// # Returns
    /// The modified options for chaining.
    pub fn with_tray_enabled(mut self, tray_enabled: bool) -> Self {
        self.tray_enabled = tray_enabled;
        self
    }

    /// Sets the callback reloading the application configuration from the tray.
    ///
    /// # Arguments
    /// * `reload_configuration` - Called on a background thread by the tray
    ///   "Reload configuration" action, returning a log summary.
    ///
    /// # Returns
    /// The modified options for chaining.
    pub fn with_reload_configuration(
        mut self,
        reload_configuration: Arc<dyn Fn() -> String + Send + Sync>,
    ) -> Self {
        self.reload_configuration = Some(reload_configuration);
        self
    }

    /// Adds a web-asset mount path served below the application root.
    ///
    /// # Arguments
    /// * `mount_path` - Relative path (e.g. `admin`) under the application
    ///   root where the shared embedded asset pool is mounted scoped to that
    ///   subdirectory.
    ///
    /// # Returns
    /// The modified options for chaining.
    pub fn with_web_mount_path(mut self, mount_path: impl Into<String>) -> Self {
        let mount_path = mount_path.into().trim_matches('/').to_string();
        if !mount_path.is_empty() {
            self.web_mount_paths.push(mount_path);
        }
        self
    }

    /// Validates and writes the persisted options atomically as JSON.
    ///
    /// # Arguments
    /// * `path` - Absolute or application-resolved configuration file path.
    ///
    /// # Errors
    /// Returns an error when validation, serialization, or atomic file writing
    /// fails.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        self.validate()?;
        let mut document = BTreeMap::new();
        self.export(&mut document);
        let path = path.as_ref();
        JsonPersistenceFileCodec
            .save(path, &document)
            .context("Failed to save application configuration.")?;
        set_owner_only_permissions(path)
            .context("Failed to restrict application configuration permissions.")
    }

    /// Validates all persisted values before applying or writing them.
    ///
    /// # Errors
    /// Returns an error when a configuration value is unsupported or unsafe.
    pub fn validate(&self) -> Result<()> {
        if self.server_port == Some(0) {
            bail!("Application configuration server port cannot be zero.");
        }
        if let Some(root) = self.entrypoint_root.as_deref() {
            validate_entrypoint_root(root)?;
        }
        if self.password_hash.as_deref().is_some_and(str::is_empty) {
            bail!("Application configuration password hash cannot be empty.");
        }
        Ok(())
    }
}

impl Default for CoreApplicationOptions {
    fn default() -> Self {
        Self {
            application_mode: None,
            server_port: Some(DEFAULT_SERVER_PORT),
            server_port_source: SettingSource::Default,
            network_mode: None,
            network_mode_source: SettingSource::Default,
            entrypoint_root: None,
            entrypoint_root_source: SettingSource::Default,
            password_hash: None,
            entrypoint_api: Some(DEFAULT_TAURI_API_PREFIX.to_string()),
            web_scheme: Some(DEFAULT_TAURI_WEB_SCHEME.to_string()),
            api_prefix: None,
            server_tray_factory: None,
            tray_enabled: true,
            reload_configuration: None,
            web_mount_paths: Vec::new(),
        }
    }
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

impl ApplicationOptionsProvider for CoreApplicationOptions {
    fn get_options(&self) -> Vec<ApplicationOptionDefinition> {
        vec![
            ApplicationOptionDefinition::new("--help", "Show this help message and exit"),
            ApplicationOptionDefinition::new(
                "--desktop",
                "Starts the application in desktop mode.",
            ),
            ApplicationOptionDefinition::new("--server", "Starts the application in server mode."),
            ApplicationOptionDefinition::new(
                "--server-port",
                format!("Override the server port (default: {DEFAULT_SERVER_PORT})").as_str(),
            ),
            ApplicationOptionDefinition::new(
                "--network",
                "Client access rule: local, private or public (default: private).",
            ),
            ApplicationOptionDefinition::new(
                "--entrypoint-root",
                "Public root path in server mode.",
            ),
            ApplicationOptionDefinition::new("--no-tray", "Disables the tray icon in server mode."),
        ]
    }

    fn export(&self, output: &mut BTreeMap<String, Option<String>>) {
        if let Some(server_port) = self.server_port {
            output.insert("server-port".to_string(), Some(server_port.to_string()));
        }
        if let Some(network_mode) = self.network_mode {
            output.insert("network".to_string(), Some(network_mode.to_string()));
        }
        if let Some(entrypoint_root) = &self.entrypoint_root {
            output.insert("entrypoint-root".to_string(), Some(entrypoint_root.clone()));
        }
        if let Some(password_hash) = &self.password_hash {
            output.insert("password_hash".to_string(), Some(password_hash.clone()));
        }
    }

    fn parse_vect(&mut self, args: Vec<String>, source: SettingSource) -> anyhow::Result<()> {
        let mut iter = args.iter();

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--desktop" => self.application_mode = Some(ApplicationMode::Desktop),
                "--server" => self.application_mode = Some(ApplicationMode::Server),
                "--server-port" => {
                    let port = iter.next().context("Missing value for `--server-port`")?;

                    self.server_port =
                        Some(port.parse::<u16>().with_context(|| {
                            format!("Invalid value for `--server-port`: `{port}`")
                        })?);
                    self.server_port_source = source;
                }
                "--network" => {
                    let value = iter.next().context("Missing value for `--network`")?;
                    self.network_mode = Some(value.parse::<ServerNetworkMode>().map_err(|error| {
                        anyhow::anyhow!("Invalid value for `--network`: {error}")
                    })?);
                    self.network_mode_source = source;
                }
                "--entrypoint-root" => {
                    let root = iter
                        .next()
                        .context("Missing value for `--entrypoint-root`")?;
                    self.entrypoint_root = match source {
                        SettingSource::Configuration if root.is_empty() => None,
                        _ => Some(root.clone()),
                    };
                    self.entrypoint_root_source = source;
                }
                "--no-tray" => self.tray_enabled = false,
                "--password_hash" if source == SettingSource::Configuration => {
                    self.password_hash = Some(
                        iter.next()
                            .context("Missing value for configuration key `password_hash`")?
                            .clone(),
                    );
                }
                _ => {}
            }
        }

        self.validate()
    }
}
