use std::sync::Arc;

use anyhow::Context;

use crate::{application::{ApplicationOptionDefinition, ApplicationOptionsProvider}, controler::ServerTrayFactory};

/// Default port used by the REST controller when no CLI override is provided.
pub const DEFAULT_SERVER_PORT: u16 = 8080;

/// Custom URI scheme used by the desktop frontend.
pub const DEFAULT_TAURI_WEB_SCHEME: &str = "arachnea-core";

/// API prefix used by the desktop binary stream routes.
pub const DEFAULT_TAURI_API_PREFIX: &str = "api";

/// Runtime mode the application controller runs under.
#[derive(PartialEq)]
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


/// Which client connections the REST server accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
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

/// Default runtime mode used when no CLI mode flag is provided.
///
/// In release builds the backend defaults to server mode; in debug builds it
/// defaults to desktop mode.


/// Runtime options parsed from command line arguments.
pub struct CoreApplicationOptions {
    /// Backend mode (desktop or server).
    pub application_mode: Option<ApplicationMode>,

    /// REST server port used in server mode.
    ///
    /// Falls back to [`DEFAULT_SERVER_PORT`] when `None`.
    pub server_port: Option<u16>,

    /// Which client connections the REST server accepts.
    ///
    /// Defaults to [`ServerNetworkMode::Private`], which binds to all
    /// interfaces and only accepts clients belonging to a local network range.
    pub network_mode: ServerNetworkMode,

    /// Optional public root path prefix for server mode.
    ///
    /// When set to a non-root prefix, a bare `GET /` on the REST server answers
    /// a `302 Found` redirect to the mounted application path (for example
    /// `/prefix/`) instead of `404 Not Found`.
    pub entrypoint_root: Option<String>,

    /// Optional public API path segment for server mode.
    pub entrypoint_api: Option<String>,

    /// Custom URI scheme used by the desktop frontend.
    ///
    /// Falls back to [`DEFAULT_TAURI_WEB_SCHEME`] when `None`.
    pub web_scheme: Option<String>,

    /// API path prefix used by the desktop binary stream routes.
    ///
    /// Falls back to [`DEFAULT_TAURI_API_PREFIX`] when `None`.
    pub api_prefix: Option<String>,

    /// Optional factory creating the server tray icon in server mode.
    ///
    /// When set and a GUI is available, the REST controller shows a tray icon.
    pub server_tray_factory: Option<Arc<dyn ServerTrayFactory>>,

    /// Whether a server tray icon should be shown in server mode.
    ///
    /// When `true` a tray factory is installed and the REST controller shows a
    /// tray icon if a GUI is available. Set to `false` to force-disable the tray
    /// even on a graphical environment.
    pub tray_enabled: bool,

    /// Callback reloading the application configuration from the server tray.
    ///
    /// Invoked on a background thread by the tray "Reload configuration"
    /// action; the returned summary is written to the application log.
    pub reload_configuration: Option<Arc<dyn Fn() -> String + Send + Sync>>,

    /// Additional web-asset mount paths served below the application root.
    ///
    /// Each path mounts the shared embedded asset pool scoped to that
    /// subdirectory (e.g. `admin` to serve the administration bundle at
    /// `/admin/`). Dedicated mounts take precedence over the root bundle.
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
            network_mode: ServerNetworkMode::default(),
            entrypoint_root,
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
}


impl Default for CoreApplicationOptions {
    fn default() -> Self {
        Self {
            application_mode: None,
            server_port: Some(DEFAULT_SERVER_PORT),
            network_mode: ServerNetworkMode::default(),
            entrypoint_root: None,
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

impl ApplicationOptionsProvider for CoreApplicationOptions {

    fn get_options(&self) -> Vec<ApplicationOptionDefinition> {
        vec![
            ApplicationOptionDefinition::new(
                "--help",
                "Show this help message and exit",
            ),
            ApplicationOptionDefinition::new(
                "--desktop",
                "Starts the application in desktop mode.",
            ),
            ApplicationOptionDefinition::new(
                "--server",
                "Starts the application in server mode.",
            ),
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
            ApplicationOptionDefinition::new(
                "--no-tray",
                "Disables the tray icon in server mode.",
            ),
        ]
    }
    
    fn from_vect(args: Vec<String>) -> anyhow::Result<Box<Self>> {
        let mut instance = CoreApplicationOptions::default();
        let mut iter = args.iter();
        
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--desktop" => {
                    instance.application_mode = Some(ApplicationMode::Desktop)
                }
                "--server" => {
                    instance.application_mode = Some(ApplicationMode::Server)
                }
                "--server-port" => {
                    let port = iter.next().context("Missing value for `--server-port`")?;

                    instance.server_port = Some(
                        port.parse::<u16>()
                            .with_context(|| format!("Invalid value for `--server-port`: `{port}`"))?,
                    );
                }
                "--network" => {
                    let value = iter.next().context("Missing value for `--network`")?;
                    instance.network_mode = value
                        .parse::<ServerNetworkMode>()
                        .map_err(|error| anyhow::anyhow!("Invalid value for `--network`: {error}"))?;
                }
                "--entrypoint-root" => {
                    instance.entrypoint_root = Some(
                        iter.next()
                            .context("Missing value for `--entrypoint-root`")?
                            .clone(),
                    );
                }
                "--no-tray" => instance.tray_enabled = false,
                _ => {}
            }
        }

        Ok(Box::new(instance))
    }
}
