//! Application-site expansion of the generated Tauri context and controller setup.

/// Builds the application controller from parsed runtime options.
///
/// This macro expands [`super::create_application_controler_from_config`] with a
/// [`super::DesktopApplicationConfig`] generated from the caller crate's own Tauri
/// configuration. Because `macro_rules!` expands at the call site, the embedded
/// `::tauri::generate_context!()` is resolved inside the caller crate, where
/// `tauri.conf.json` and `tauri-build` live. The values of `web_scheme` and
/// `api_prefix` come from the supplied [`super::options::CoreApplicationOptions`].
///
/// The Tauri context is generated **exactly once** per binary: on macOS the
/// embedded `Info.plist` is exposed as a single `_EMBED_INFO_PLIST` symbol, so
/// `tauri::generate_context!()` cannot be expanded more than once. The real
/// assets are extracted into the shared web asset provider and the context is
/// routed to the single runtime consumer of the effective application mode:
/// the server tray in [`super::options::ApplicationMode::Server`] or the Tauri controller in
/// [`super::options::ApplicationMode::Desktop`].
///
/// When [`super::options::CoreApplicationOptions::tray_enabled`] is true and no tray factory is
/// already present on the options, the macro installs a
/// [`ServerTrayIconService`](crate::controler::rest::tray::ServerTrayIconService) built from the caller's Tauri context. The tray
/// icon falls back to the context's embedded default window icon (derived from
/// `bundle.icon` in `tauri.conf.json`).
///
/// # Arguments
/// * `$options` - Parsed [`super::options::CoreApplicationOptions`].
///
/// # Returns
/// A boxed `ControlerService` ready for `register_service` and `launch`.
#[macro_export]
macro_rules! create_application_controler {
    ($options:expr) => {{
        let mut options = $options;

        // Resolve the effective application mode with the same defaults as
        // `create_application_controler_from_config`, so the single generated
        // Tauri context is handed to the one component that will consume it.
        let is_server = match &options.application_mode {
            ::std::option::Option::Some(mode) => {
                ::core::matches!(mode, $crate::controler::options::ApplicationMode::Server)
            }
            ::std::option::Option::None => {
                #[cfg(not(debug_assertions))]
                {
                    true // Release default: server.
                }
                #[cfg(debug_assertions)]
                {
                    true // Debug default: server.
                }
            }
        };

        let mut context = ::tauri::generate_context!();
        let web_assets = $crate::controler::desktop_embedded_web_assets(&mut context);

        let desktop_context = if is_server {
            if options.tray_enabled && options.server_tray_factory.is_none() {
                options.server_tray_factory = Some(::std::sync::Arc::new(
                    $crate::controler::rest::tray::ServerTrayIconService::new(context),
                ));
            }
            // In server mode the context is owned by the server tray; the
            // desktop backend is never built so it receives no context.
            ::std::option::Option::None
        } else {
            // In desktop mode the Tauri controller owns the whole context.
            ::std::option::Option::Some(context)
        };

        $crate::controler::create_application_controler_from_config(
            options,
            $crate::controler::DesktopApplicationConfig {
                context: desktop_context,
                web_assets,
            },
        )
    }};
}
