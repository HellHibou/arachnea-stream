//! Configuration snapshots used by the REST supervisor.

use super::{Arc, RestServerSettings, ServerNetworkMode, ServerTrayFactory};

/// Configuration pieces required to rebuild a [`super::RestControlerService`].
///
/// The supervisor keeps the snapshot of the original configuration and derives
/// the rebuilt snapshots from the dynamic settings (port, network mode,
/// entrypoint root).
#[derive(Clone)]
pub(crate) struct RestServiceSnapshot {
    /// IP address the server binds to (`None` means loopback).
    pub(crate) server_ip: Option<std::net::IpAddr>,
    /// Server port.
    pub(crate) server_port: u16,
    /// Networks whose clients are allowed to connect to the server.
    pub(crate) allowed_networks: Vec<ipnet::IpNet>,
    /// Public root path prefix, when set.
    pub(crate) entrypoint_root: Option<String>,
    /// API path segment mounted under the root prefix.
    pub(crate) entrypoint_api: Option<String>,
    /// Tray factory supplied by the application crate.
    pub(crate) tray_factory: Option<Arc<dyn ServerTrayFactory>>,
    /// Whether the server tray should be shown when a GUI is available.
    pub(crate) tray_enabled: bool,
    /// Callback reloading the application configuration from the tray.
    pub(crate) reload_configuration: Option<Arc<dyn Fn() -> String + Send + Sync>>,
}

impl RestServiceSnapshot {
    /// Returns the socket address the server binds to.
    pub(crate) fn socket_addr(&self) -> std::net::SocketAddr {
        std::net::SocketAddr::new(
            self.server_ip.unwrap_or(crate::REST_DEFAULT_BIND_IP),
            self.server_port,
        )
    }

    /// Returns the parsed entry point root path segments.
    ///
    /// Empty segments produced by leading, trailing or doubled slashes are
    /// dropped so every segment is safe to feed to `warp::path`.
    pub(crate) fn root_segments(&self) -> Vec<String> {
        self.entrypoint_root
            .as_deref()
            .filter(|root| !root.is_empty())
            .map(|root| {
                root.split('/')
                    .map(str::trim)
                    .filter(|segment| !segment.is_empty())
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns the dynamic settings describing this snapshot.
    pub(crate) fn initial_settings(&self) -> RestServerSettings {
        let network_mode = if self.server_ip.is_none_or(|ip| ip.is_loopback()) {
            ServerNetworkMode::Local
        } else if self.allowed_networks.is_empty() {
            ServerNetworkMode::Public
        } else {
            ServerNetworkMode::Private
        };
        RestServerSettings {
            server_port: self.server_port,
            network_mode,
            entrypoint_root: self.entrypoint_root.clone(),
        }
    }
}
