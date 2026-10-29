//! Application-supplied desktop assets and REST/Tauri controller construction.

use super::options::{
    ApplicationMode, ServerNetworkMode, DEFAULT_SERVER_PORT, DEFAULT_TAURI_API_PREFIX,
    DEFAULT_TAURI_WEB_SCHEME,
};
use super::rest::{RestControlerConfiguration, RestControlerService};
use super::tauri::{TauriControlerConfiguration, TauriControlerService, TauriEmbeddedWebAssets};
use super::{options, ControlerService, SharedWebAssets};
use std::sync::Arc;

/// Configuration required to build the desktop (Tauri) controller backend.
///
/// This groups the application-owned pieces that only the application crate can
/// produce: the generated Tauri context and its embedded web assets. It is kept
/// separate from [`options::CoreApplicationOptions`] because it is generated for each
/// application crate rather than parsed from runtime options.
pub struct DesktopApplicationConfig {
    /// The generated Tauri context routed to the desktop backend in desktop
    /// mode. `None` in server mode, where the context is owned by the server
    /// tray instead and this backend never builds a desktop app.
    pub context: Option<::tauri::Context<::tauri::Wry>>,
    /// Embedded frontend assets backing the Tauri custom protocol.
    pub web_assets: SharedWebAssets,
}

/// Empty Tauri asset provider placed inside the generated context once its real
/// assets are moved into the shared web asset provider.
///
/// The Tauri context is generated exactly once per binary (macOS embeds the
/// `Info.plist` as a single `_EMBED_INFO_PLIST` symbol, so `generate_context!`
/// cannot be expanded more than once). The desktop app serves the frontend
/// through a custom protocol backed by the shared assets and the server tray is
/// headless, so neither reads assets through this placeholder.
struct EmptyTauriAssets;

impl ::tauri::Assets<::tauri::Wry> for EmptyTauriAssets {
    fn get(&self, _key: &::tauri::utils::assets::AssetKey) -> Option<std::borrow::Cow<'_, [u8]>> {
        None
    }

    fn iter(&self) -> Box<::tauri::utils::assets::AssetsIter<'_>> {
        Box::new(std::iter::empty::<(
            std::borrow::Cow<'static, str>,
            std::borrow::Cow<'static, [u8]>,
        )>())
    }

    fn csp_hashes(
        &self,
        _html_path: &::tauri::utils::assets::AssetKey,
    ) -> Box<dyn Iterator<Item = ::tauri::utils::assets::CspHash<'_>> + '_> {
        Box::new(std::iter::empty())
    }
}

/// Wraps Tauri generated assets into the shared embedded web asset provider.
///
/// The real embedded assets are extracted out of the supplied context (which is
/// generated only once, see `EmptyTauriAssets`) so they can be shared by
/// value, leaving an empty placeholder behind for the context's own copy.
///
/// # Arguments
/// * `context` - Generated Tauri context whose asset bundle is extracted.
///
/// # Returns
/// A `SharedWebAssets` provider backed by the Tauri asset bundle.
pub fn desktop_embedded_web_assets(
    context: &mut ::tauri::Context<::tauri::Wry>,
) -> SharedWebAssets {
    let original_assets = context.set_assets(Box::new(EmptyTauriAssets));
    let assets: Arc<dyn ::tauri::Assets<::tauri::Wry>> = Arc::from(original_assets);
    Arc::new(TauriEmbeddedWebAssets::new(assets))
}

/// Builds the Tauri controller using the application defaults for unset options.
///
/// # Arguments
/// * `context` - Generated Tauri context owned by the desktop backend.
/// * `web_scheme` - Optional custom-protocol scheme; defaults to the application constant.
/// * `api_prefix` - Optional API path prefix; defaults to the application constant.
fn tauri_controler_service(
    context: ::tauri::Context<::tauri::Wry>,
    web_scheme: Option<String>,
    api_prefix: Option<String>,
) -> TauriControlerService {
    let web_scheme = web_scheme.unwrap_or_else(|| DEFAULT_TAURI_WEB_SCHEME.to_string());
    let api_prefix = api_prefix.unwrap_or_else(|| DEFAULT_TAURI_API_PREFIX.to_string());
    TauriControlerService::with_configuration(
        TauriControlerConfiguration::new(context)
            .web_scheme(web_scheme)
            .api_prefix(api_prefix),
    )
}

/// Creates the application controller from explicit runtime and desktop options.
///
/// Desktop mode builds a Tauri controller and embeds the frontend web assets;
/// server mode builds a REST controller and mounts the embedded assets under
/// the optional entry point prefixes.
///
/// # Arguments
/// * `options` - Runtime options selecting the backend mode, port and overrides.
/// * `desktop` - Desktop-specific generated context and embedded assets.
///
/// # Returns
/// A boxed `ControlerService` ready for `register_service` and `launch`.
pub fn create_application_controler_from_config(
    options: options::CoreApplicationOptions,
    desktop: DesktopApplicationConfig,
) -> Box<dyn ControlerService> {
    let application_mode = options.application_mode.unwrap_or_default();

    let mut controler: Box<dyn ControlerService> = if application_mode == ApplicationMode::Server {
        let server_port = options.server_port.unwrap_or(DEFAULT_SERVER_PORT);
        let mut configuration = RestControlerConfiguration::default().server_port(server_port);

        match options.network_mode.unwrap_or_default() {
            ServerNetworkMode::Local => {}
            ServerNetworkMode::Private => {
                configuration =
                    configuration.server_ip(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));

                configuration =
                    configuration.allowed_networks(crate::controler::rest::local_networks());
            }
            ServerNetworkMode::Public => {
                configuration =
                    configuration.server_ip(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
            }
        }
        if let Some(entrypoint_root) = &options.entrypoint_root {
            configuration = configuration.entrypoint_root(entrypoint_root);
        }
        if let Some(entrypoint_api) = &options.entrypoint_api {
            configuration = configuration.entrypoint_api(entrypoint_api);
        }
        if let Some(tray_factory) = &options.server_tray_factory {
            configuration = configuration
                .server_tray_factory(Arc::clone(tray_factory))
                .tray_enabled(true);
        }
        if let Some(reload_configuration) = &options.reload_configuration {
            configuration = configuration.reload_configuration(Arc::clone(reload_configuration));
        }

        Box::new(RestControlerService::new(configuration))
    } else {
        let context = desktop
            .context
            .expect("desktop mode requires the Tauri context routed by the application crate");
        Box::new(tauri_controler_service(
            context,
            options.web_scheme,
            options.api_prefix,
        ))
    };

    // Mount dedicated bundles (e.g. the administration app) before the root so
    // they win over the root fallback for their own paths.
    for mount_path in &options.web_mount_paths {
        controler.register_embedded_web_assets(Arc::clone(&desktop.web_assets), mount_path);
    }
    controler.register_embedded_web_assets(desktop.web_assets, "");

    controler
}
