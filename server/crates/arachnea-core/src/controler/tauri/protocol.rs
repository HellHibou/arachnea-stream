//! Custom-scheme routing for binary commands and scoped frontend resources.

use super::{assets::TauriWebAssets, stream_response};
use crate::controler::{
    web_assets::{
        asset_etag_for, etag_not_modified, new_web_generation, replace_html_base, web_instance_etag,
    },
    StreamControlerFunction,
};
use crate::TAURI_WEB_BASE;
use ::tauri::Wry;

/// Registers stream routes and frontend mounts on the configured custom scheme.
///
/// # Arguments
/// * `builder` - Native application builder before protocol registration.
/// * `web_assets` - Frontend mounts, selecting the most specific matching path.
/// * `stream_handlers` - Binary callbacks with their public entry points.
/// * `web_scheme` - Application-owned custom protocol scheme.
/// * `api_prefix` - Prefix selecting binary command routes.
pub(super) fn register(
    mut builder: ::tauri::Builder<Wry>,
    web_assets: Vec<TauriWebAssets>,
    stream_handlers: Vec<(String, StreamControlerFunction, String)>,
    web_scheme: String,
    api_prefix: String,
) -> ::tauri::Builder<Wry> {
    if !web_assets.is_empty() {
        let uri_scheme = web_scheme.clone();
        // No hot-apply in desktop mode (fixed bases `/` and `/{mount}/`):
        // one generation taken at launch covers every HTML document.
        let tauri_generation = new_web_generation();
        let tauri_instance = web_instance_etag().to_string();
        builder = builder.register_asynchronous_uri_scheme_protocol(
            uri_scheme,
            move |_app, request, responder| {
                let request_path = request_path_from_tauri_uri(request.uri(), &web_scheme);

                if let Some((command, path, query)) = split_stream_route(&request_path, &api_prefix)
                {
                    if let Some((_cmd, handler, entry_point)) = stream_handlers
                        .iter()
                        .find(|(registered, _, _)| registered == command)
                    {
                        let handler = std::sync::Arc::clone(handler);
                        let entry_point = entry_point.clone();
                        stream_response::respond(
                            request,
                            responder,
                            handler,
                            entry_point,
                            path,
                            query,
                        );
                        return;
                    }
                }

                // Select the most specific mount serving this request: dedicated
                // bundles (e.g. `/admin/`) take precedence over the root mount.
                let asset_path = request_path.split(['?', '#']).next().unwrap_or_default();
                let if_none_match = request
                    .headers()
                    .get("if-none-match")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string);
                let selected = web_assets
                    .iter()
                    .filter(|assets| assets.matches(asset_path))
                    .max_by_key(|assets| assets.mount_path.len())
                    .map(|assets| {
                        (
                            assets.mount_path.clone(),
                            assets.is_embedded(),
                            assets.load_request(asset_path),
                        )
                    });
                let response = match selected {
                    Some((mount_path, conditional, Ok(asset))) => {
                        // Every bundle needs an absolute mount base for direct History-API routes.
                        let html_base = if mount_path.is_empty() {
                            TAURI_WEB_BASE.to_string()
                        } else {
                            format!("/{mount_path}/")
                        };
                        if conditional {
                            let (etag_header, bare_etag) =
                                asset_etag_for(asset.mime_type, &tauri_generation, &tauri_instance);
                            if etag_not_modified(if_none_match.as_deref(), &bare_etag) {
                                ::tauri::http::Response::builder()
                                    .status(304)
                                    .header("ETag", etag_header)
                                    .body(Vec::new())
                                    .expect("Failed to build Tauri 304 protocol response.")
                            } else {
                                let bytes = if asset.mime_type.starts_with("text/html") {
                                    replace_html_base(asset.bytes, &html_base)
                                } else {
                                    asset.bytes
                                };
                                ::tauri::http::Response::builder()
                                    .header("Content-Type", asset.mime_type)
                                    .header("ETag", etag_header)
                                    .body(bytes)
                                    .expect("Failed to build Tauri protocol response.")
                            }
                        } else {
                            let bytes = if asset.mime_type.starts_with("text/html") {
                                replace_html_base(asset.bytes, &html_base)
                            } else {
                                asset.bytes
                            };
                            ::tauri::http::Response::builder()
                                .header("Content-Type", asset.mime_type)
                                .body(bytes)
                                .expect("Failed to build Tauri protocol response.")
                        }
                    }
                    Some((_, _, Err(_))) | None => ::tauri::http::Response::builder()
                        .status(404)
                        .body(Vec::new())
                        .expect("Failed to build Tauri 404 protocol response."),
                };
                responder.respond(response);
            },
        );
    }

    builder
}

/// Extracts the request path from a Tauri URI.
///
/// This function strips the custom scheme and host from Tauri URIs to extract
/// the actual request path. The query string is preserved because stream
/// handlers (e.g. the HTTP proxy) forward it to signed upstream targets; the
/// fragment is client-side only and is dropped.
///
/// # Arguments
/// * `uri` - The Tauri HTTP URI.
/// * `web_scheme` - The custom URI scheme to strip.
///
/// # Returns
/// The request path (with its query string) and the scheme and host removed.
fn request_path_from_tauri_uri(uri: &::tauri::http::Uri, web_scheme: &str) -> String {
    let uri = uri.to_string();
    let uri = uri.split('#').next().unwrap_or_default();

    for prefix in [
        format!("{web_scheme}://localhost"),
        format!("https://{web_scheme}.localhost"),
        format!("http://{web_scheme}.localhost"),
    ] {
        if let Some(path) = uri.strip_prefix(&prefix) {
            return path.to_string();
        }
    }

    uri.to_string()
}

/// Splits a stream route path into its components.
///
/// This function parses stream route paths to extract the command name,
/// remaining path, and query parameters.
///
/// # Arguments
/// * `request_path` - The full request path.
/// * `api_prefix` - The API prefix to strip from the path.
///
/// # Returns
/// `Some((command, remaining_path, query))` if the path matches a stream route.
/// `None` if the path doesn't start with the API prefix.
fn split_stream_route<'a>(
    request_path: &'a str,
    api_prefix: &str,
) -> Option<(&'a str, String, String)> {
    let (path_only, query) = match request_path.split_once('?') {
        Some((path, query)) => (path, query.to_string()),
        None => (request_path, String::new()),
    };

    let stream_path = path_only.strip_prefix(api_prefix)?;
    let (command, remaining_path) = match stream_path.split_once('/') {
        Some((command, tail)) => (command, tail.to_string()),
        None => (stream_path, String::new()),
    };

    Some((command, remaining_path, query))
}
