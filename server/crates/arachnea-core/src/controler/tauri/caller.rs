//! Capture native caller identity only for bundled application documents.

use crate::controler::NativeWebviewContext;

/// Captures a local application's native webview, rejecting remote/provider URLs.
///
/// # Arguments
/// * `webview` - Actual invoking native webview.
/// * `scheme` - Application-configured custom protocol scheme.
///
/// # Returns
/// Trusted native metadata or absence when the document is external/unavailable.
pub(super) fn capture(
    webview: &tauri::Webview<tauri::Wry>,
    scheme: &str,
) -> Option<NativeWebviewContext> {
    let url = webview.url().ok()?;
    let scheme = scheme.trim_end_matches(':');
    let local = (url.scheme() == scheme && url.host_str() == Some("localhost"))
        || (matches!(url.scheme(), "http" | "https")
            && url.host_str() == Some(format!("{scheme}.localhost").as_str()));
    local.then(|| NativeWebviewContext {
        label: webview.label().to_string(),
        url: url.to_string(),
    })
}
