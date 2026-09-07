//! Independent native windows for public frontend browsing.

use anyhow::{Context, Result};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Wry};
use tauri_plugin_opener::OpenerExt;

/// Process-wide unique suffix for secondary native window labels.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Creates the configured public window with native new-window handling.
///
/// # Arguments
/// * `app` - Running application handle.
/// * `config` - Application-owned native window configuration.
/// * `scheme` - Registered frontend scheme.
pub(super) fn setup(
    app: &AppHandle<Wry>,
    config: tauri::utils::config::WindowConfig,
    scheme: String,
) -> Result<()> {
    with_requests(
        WebviewWindowBuilder::from_config(app, &config)?,
        app.clone(),
        config.label,
        scheme,
    )
    .build()?;
    Ok(())
}

/// Adds recursive window handling while preserving the application's builder settings.
///
/// # Arguments
/// * `builder` - Builder for the requesting window.
/// * `app` - Running application handle.
/// * `source_label` - Window supplying the title for new windows.
/// * `scheme` - Registered frontend URI scheme.
fn with_requests<'a>(
    builder: WebviewWindowBuilder<'a, Wry, AppHandle<Wry>>,
    app: AppHandle<Wry>,
    source_label: String,
    scheme: String,
) -> WebviewWindowBuilder<'a, Wry, AppHandle<Wry>> {
    builder.on_new_window(move |url, _| {
        let app = app.clone();
        let source_label = source_label.clone();
        let scheme = scheme.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                let internal = (url.scheme() == scheme && url.host_str() == Some("localhost"))
                    || (matches!(url.scheme(), "http" | "https")
                        && url.host_str() == Some(format!("{scheme}.localhost").as_str()));
                if !internal {
                    if matches!(url.scheme(), "http" | "https" | "mailto" | "tel") {
                        app.opener().open_url(url.as_str(), None::<&str>)?;
                    }
                    return Ok(());
                }
                let source = app
                    .get_webview_window(&source_label)
                    .context("Source window is unavailable")?;
                let title = source.title()?;
                let label = format!("frontend-{}", SEQUENCE.fetch_add(1, Ordering::Relaxed));
                let builder =
                    WebviewWindowBuilder::new(&app, &label, WebviewUrl::CustomProtocol(url))
                        .title(title)
                        .inner_size(980.0, 680.0);
                with_requests(builder, app.clone(), label, scheme).build()?;
                Ok(())
            })();
            if let Err(error) = result {
                tracing::warn!(%error, "failed to open frontend window");
            }
        });
        tauri::webview::NewWindowResponse::Deny
    })
}
