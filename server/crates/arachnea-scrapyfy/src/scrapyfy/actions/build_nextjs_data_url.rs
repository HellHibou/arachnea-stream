use regex::Regex;
use std::sync::OnceLock;
use url::Url;

/// Applies the `build_nextjs_data_url` scraper action, rewriting every value
/// as the public Next.js `/_next/data/...json` URL associated with the page.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline. Each
///   value is interpreted as a page path or full URL.
/// * `request_url` - Final URL used to fetch the current page; used as the
///   resolution base when a relative path is provided.
/// * `response_body` - Raw HTML response body used to extract the Next.js
///   build id. When `None`, the action leaves the value list unchanged.
/// * `data_root` - Optional URL path prefix inserted before `/_next/data`
///   (e.g. `/rtlplay`).
/// * `route_prefix` - Optional path prefix inserted between the build id
///   and the page path (e.g. `detail`).
/// * `page_path_prefix_to_strip` - Optional prefix removed from the public
///   page path before the JSON path is assembled.
///
/// # Returns
///
/// A new value list where each entry is the matching Next.js JSON URL, or
/// the original value when the build id cannot be located or the URL cannot
/// be assembled.
pub(super) fn apply(
    texts: Vec<String>,
    request_url: &str,
    response_body: Option<&str>,
    data_root: Option<&str>,
    route_prefix: Option<&str>,
    page_path_prefix_to_strip: Option<&str>,
) -> Vec<String> {
    let Some(build_id) = response_body.and_then(extract_nextjs_build_id) else {
        return texts;
    };

    texts
        .into_iter()
        .map(|value| {
            build_nextjs_data_url(
                &value,
                request_url,
                &build_id,
                data_root,
                route_prefix,
                page_path_prefix_to_strip,
            )
            .unwrap_or(value)
        })
        .collect()
}

/// Builds the `/_next/data/<buildId>/<routePrefix?>/<pagePath>.json` URL for
/// the given page reference.
///
/// # Arguments
///
/// * `value` - Page path or full URL to rewrite.
/// * `request_url` - Final URL used to fetch the current page; used as the
///   resolution base when `value` is a relative path.
/// * `build_id` - Next.js build id extracted from the response body.
/// * `data_root` - Optional URL path prefix inserted before `/_next/data`.
/// * `route_prefix` - Optional path prefix inserted between the build id
///   and the page path.
/// * `page_path_prefix_to_strip` - Optional prefix removed from the public
///   page path before the JSON path is assembled.
///
/// # Returns
///
/// The rewritten JSON URL, or `None` when `request_url` or `value` cannot be
/// parsed as a URL.
fn build_nextjs_data_url(
    value: &str,
    request_url: &str,
    build_id: &str,
    data_root: Option<&str>,
    route_prefix: Option<&str>,
    page_path_prefix_to_strip: Option<&str>,
) -> Option<String> {
    let request_url = Url::parse(request_url).ok()?;
    let page_url = Url::parse(value)
        .or_else(|_| request_url.join(value))
        .ok()?;

    if page_url.path().contains("/_next/data/") && page_url.path().ends_with(".json") {
        return Some(page_url.to_string());
    }

    let page_path = strip_path_prefix(
        page_url.path().trim_start_matches('/'),
        page_path_prefix_to_strip,
    );
    let data_root = normalize_root_path(data_root);
    let route_prefix = normalize_path_segment(route_prefix);

    let mut json_path = format!("{}/_next/data/{}", data_root, build_id);
    if let Some(route_prefix) = route_prefix {
        json_path.push('/');
        json_path.push_str(&route_prefix);
    }
    if !page_path.is_empty() {
        json_path.push('/');
        json_path.push_str(page_path);
    }
    json_path.push_str(".json");

    let mut json_url = format!("{}{}", page_url.origin().ascii_serialization(), json_path);
    if let Some(query) = page_url.query() {
        json_url.push('?');
        json_url.push_str(query);
    }

    Some(json_url)
}

/// Searches `response_body` for the Next.js build id using the configured
/// inline-JSON and build manifest patterns.
fn extract_nextjs_build_id(response_body: &str) -> Option<String> {
    for pattern in [
        nextjs_build_id_json_regex(),
        nextjs_build_manifest_regex(),
    ] {
        if let Some(captures) = pattern.captures(response_body) {
            if let Some(build_id) = captures.get(1) {
                return Some(build_id.as_str().to_string());
            }
        }
    }

    None
}

/// Returns the cached regex used to detect the Next.js build id embedded as
/// an inline JSON property (`"buildId":"..."`).
fn nextjs_build_id_json_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#""buildId":"([^"]+)""#).expect("Invalid Next.js buildId JSON regex")
    })
}

/// Returns the cached regex used to detect the Next.js build id embedded in
/// the public `/_next/static/<buildId>/_buildManifest.js` script reference.
fn nextjs_build_manifest_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"/_next/static/([^/]+)/_buildManifest\.js"#)
            .expect("Invalid Next.js build manifest regex")
    })
}

/// Normalizes a configured data root path so it always starts with a single
/// leading slash (or is empty when no root is provided).
fn normalize_root_path(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("/{}", value.trim_matches('/')))
        .unwrap_or_default()
}

/// Normalizes a configured route prefix by trimming surrounding slashes and
/// rejecting empty values.
fn normalize_path_segment(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_matches('/').to_string())
        .filter(|value| !value.is_empty())
}

/// Removes `prefix` from `path` (when present) and any leading slash, so the
/// remaining segment can be appended to the JSON URL cleanly.
fn strip_path_prefix<'a>(path: &'a str, prefix: Option<&str>) -> &'a str {
    let Some(prefix) = prefix
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_matches('/').to_string())
    else {
        return path;
    };

    path.strip_prefix(prefix.as_str())
        .unwrap_or(path)
        .trim_start_matches('/')
}
