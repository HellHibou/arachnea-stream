//! Precedence of command-line, persisted and default REST settings.

use super::options::{CoreApplicationOptions, SettingSource, DEFAULT_SERVER_PORT};
use super::{RestServerSettings, RestSettingsSource};
use std::sync::Arc;

/// Builds the dynamic REST settings resolver of an application.
///
/// Combines the application-supplied reader of the persisted configuration
/// with the command-line overrides captured from the options provenances
/// (`server_port_source`, `network_mode_source`, `entrypoint_root_source`),
/// applying the priority rule `command line > persisted configuration >
/// built-in default`:
///
/// - a `SettingSource::CommandLine` value is pinned and always wins over the
///   persisted configuration (including values changed later through the
///   administration service);
/// - otherwise the persisted value is used, falling back to the built-in
///   defaults (`DEFAULT_SERVER_PORT`, `ServerNetworkMode::Private`, no root).
///
/// The returned resolver returns `None` when the persisted reader itself
/// returns `None`, which signals that hot application is unavailable.
///
/// # Arguments
///
/// * `persisted` - Reader of the persisted application configuration; the
///   application owns the file lifecycle, Core never opens it directly.
/// * `cli` - Effective application options captured before they are moved into
///   the controller constructor.
pub fn rest_settings_source(
    persisted: Arc<dyn Fn() -> Option<CoreApplicationOptions> + Send + Sync>,
    cli: CoreApplicationOptions,
) -> RestSettingsSource {
    let port_pinned = cli.server_port_source == SettingSource::CommandLine;
    let cli_port = cli.server_port;
    let network_pinned = cli.network_mode_source == SettingSource::CommandLine;
    let cli_network = cli.network_mode;
    let root_pinned = cli.entrypoint_root_source == SettingSource::CommandLine;
    let cli_root = cli.entrypoint_root;

    Arc::new(move || {
        let config = persisted()?;
        let server_port = if port_pinned {
            cli_port?
        } else {
            config.server_port.unwrap_or(DEFAULT_SERVER_PORT)
        };
        let network_mode = if network_pinned {
            cli_network?
        } else {
            config.network_mode.unwrap_or_default()
        };
        let entrypoint_root = if root_pinned {
            cli_root.clone()
        } else {
            config.entrypoint_root.clone()
        };
        Some(RestServerSettings {
            server_port,
            network_mode,
            entrypoint_root,
        })
    })
}

#[cfg(test)]
#[path = "../../tests/controler/rest_settings.rs"]
mod tests;
