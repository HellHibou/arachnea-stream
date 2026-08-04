//! Configuration options for the REST controller backend.

use std::sync::Arc;

use super::tray::ServerTrayFactory;

/// Configuration options for the REST controller backend.
#[derive(Default)]
pub struct RestControlerConfiguration {
    /// Server socket address to bind to. Takes precedence over `server_ip` and `server_port` if set.
    pub(crate) server_socket: Option<std::net::SocketAddr>,

    /// Server port. Used when `server_socket` is not set.
    pub(crate) server_port: Option<u16>,

    /// Server IP address. Used when `server_socket` is not set.
    pub(crate) server_ip: Option<std::net::IpAddr>,

    /// Entry point root path prefix mounted by the HTTP server.
    pub(crate) entrypoint_root: Option<String>,

    /// Entry point API path segment mounted under the root prefix.
    pub(crate) entrypoint_api: Option<String>,

    /// Factory producing the server tray icon, supplied by the application crate.
    pub(crate) tray_factory: Option<Arc<dyn ServerTrayFactory>>,

    /// Whether the server tray should be shown when a GUI is available.
    pub(crate) tray_enabled: bool,
}

impl RestControlerConfiguration {
    /// Sets the socket address used by the HTTP server.
    ///
    /// # Arguments
    /// * `server_socket` - Socket address to bind to.
    ///
    /// # Returns
    /// The updated configuration.
    #[allow(dead_code)]
    pub fn server_socket(mut self, server_socket: std::net::SocketAddr) -> Self {
        self.server_socket = Some(server_socket);
        self
    }

    /// Sets the IP address used by the HTTP server.
    ///
    /// # Arguments
    /// * `server_ip` - Server IP address.
    ///
    /// # Returns
    /// The updated configuration.
    #[allow(dead_code)]
    pub fn server_ip(mut self, server_ip: std::net::IpAddr) -> Self {
        self.server_ip = Some(server_ip);
        self
    }

    /// Sets the port used by the HTTP server.
    ///
    /// # Arguments
    /// * `server_port` - Server port.
    ///
    /// # Returns
    /// The updated configuration.
    #[allow(dead_code)]
    pub fn server_port(mut self, server_port: u16) -> Self {
        self.server_port = Some(server_port);
        self
    }

    /// Sets the root path prefix mounted by the HTTP server.
    ///
    /// # Arguments
    /// * `entrypoint_root` - Root path prefix.
    ///
    /// # Returns
    /// The updated configuration.
    #[allow(dead_code)]
    pub fn entrypoint_root(mut self, entrypoint_root: &str) -> Self {
        self.entrypoint_root = Some(entrypoint_root.to_string());
        self
    }

    /// Sets the API path segment mounted under the root prefix.
    ///
    /// # Arguments
    /// * `entrypoint_api` - API path segment.
    ///
    /// # Returns
    /// The updated configuration.
    #[allow(dead_code)]
    pub fn entrypoint_api(mut self, entrypoint_api: &str) -> Self {
        self.entrypoint_api = Some(entrypoint_api.to_string());
        self
    }

    /// Sets whether the server tray icon should be shown when a GUI is available.
    ///
    /// # Arguments
    /// * `tray_enabled` - Enable the server tray.
    ///
    /// # Returns
    /// The updated configuration.
    #[allow(dead_code)]
    pub fn tray_enabled(mut self, tray_enabled: bool) -> Self {
        self.tray_enabled = tray_enabled;
        self
    }

    /// Sets the factory that creates the server tray icon.
    ///
    /// # Arguments
    /// * `tray_factory` - Application-supplied tray factory.
    ///
    /// # Returns
    /// The updated configuration.
    #[allow(dead_code)]
    pub fn server_tray_factory(mut self, tray_factory: Arc<dyn ServerTrayFactory>) -> Self {
        self.tray_factory = Some(tray_factory);
        self
    }
}
