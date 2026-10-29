use std::collections::HashMap;
use url::Url;

use super::{ProxyFollowRedirects, ProxyReplaceAllConfig, HTTP_PROXY_PUBLIC_PATH_PARAM};
use crate::scrapyfy::query_helpers;

#[cfg(feature = "arachnea-proxy")]
use arachnea_proxy::http::actions::ReplaceAll;
#[cfg(feature = "arachnea-proxy")]
use arachnea_proxy::http::proxy_service::proxied_url_with_countries_and_options;

/// Applies the `resolve_url` scraper action by resolving every value as a URL
/// relative to `request_url`.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline.
/// * `request_url` - Final URL used to fetch the current page; used as the
///   resolution base for relative paths.
/// * `params` - Runtime parameters available to query templates and action pipelines.
/// * `proxy` - Whether HTTP(S) results should use the configured public proxy route.
/// * `proxy_headers` - Extra proxy-request headers resolved from runtime parameters and the request origin.
/// * `proxy_replace_all` - Post-response replacement rules attached to the generated proxy URL.
/// * `proxy_follow_redirects` - Optional redirect-following policy attached to the generated proxy URL.
///
/// # Returns
///
/// A new value list where every value is either a fully qualified URL or
/// resolved against `request_url`. Values that fail to parse and cannot be
/// joined onto the base are kept as-is.
pub(super) fn apply(
    texts: Vec<String>,
    request_url: &str,
    params: &HashMap<String, String>,
    proxy: bool,
    proxy_headers: &HashMap<String, String>,
    proxy_replace_all: &[ProxyReplaceAllConfig],
    proxy_follow_redirects: Option<&ProxyFollowRedirects>,
) -> Vec<String> {
    let base_url = Url::parse(request_url).ok();
    let proxy_path = proxy.then(|| proxy_path(params)).flatten();
    let proxy_headers = resolve_proxy_headers(params, request_url, proxy_headers);

    texts
        .into_iter()
        .map(|value| {
            if let Ok(url) = Url::parse(&value) {
                return resolved_url_string(
                    url,
                    proxy_path,
                    &proxy_headers,
                    proxy_replace_all,
                    proxy_follow_redirects,
                );
            }

            match &base_url {
                Some(base_url) => resolve_relative_url(base_url, &value)
                    .map(|url| {
                        resolved_url_string(
                            url,
                            proxy_path,
                            &proxy_headers,
                            proxy_replace_all,
                            proxy_follow_redirects,
                        )
                    })
                    .unwrap_or(value),
                _none => value,
            }
        })
        .collect()
}

/// Borrowed proxy policy used by ancestor-relative URL resolution.
pub(super) struct ParentUrlProxyPolicy<'a> {
    /// Whether resolved HTTP(S) URLs use the public proxy route.
    pub(super) proxy: bool,
    /// Borrowed extra proxy-request header templates.
    pub(super) proxy_headers: &'a HashMap<String, String>,
    /// Borrowed post-response replacement rules.
    pub(super) proxy_replace_all: &'a [ProxyReplaceAllConfig],
    /// Borrowed redirect-following policy.
    pub(super) proxy_follow_redirects: Option<&'a ProxyFollowRedirects>,
}

/// Applies the `resolve_url_from_parent` scraper action by resolving every
/// value against an ancestor directory of `request_url`.
///
/// # Arguments
///
/// * `texts` - Owned values from earlier actions.
/// * `request_url` - Final page URL used to resolve ancestor-relative values.
/// * `params` - Runtime template parameters.
/// * `levels` - Number of trailing path segments to remove before joining.
/// * `options` - Borrowed proxy flag, headers, replacement and redirect policy.
///
/// # Returns
///
/// A new value list where every value is either a fully qualified URL or
/// joined onto the ancestor of `request_url`. Unparseable values that
/// cannot be joined are kept as-is.
pub(super) fn apply_from_parent(
    texts: Vec<String>,
    request_url: &str,
    params: &HashMap<String, String>,
    levels: usize,
    options: ParentUrlProxyPolicy<'_>,
) -> Vec<String> {
    let ParentUrlProxyPolicy {
        proxy,
        proxy_headers,
        proxy_replace_all,
        proxy_follow_redirects,
    } = options;
    let base_url = Url::parse(request_url)
        .ok()
        .and_then(|url| ancestor_base_url(&url, levels));
    let proxy_path = proxy.then(|| proxy_path(params)).flatten();
    let proxy_headers = resolve_proxy_headers(params, request_url, proxy_headers);

    texts
        .into_iter()
        .map(|value| {
            if let Ok(url) = Url::parse(&value) {
                return resolved_url_string(
                    url,
                    proxy_path,
                    &proxy_headers,
                    proxy_replace_all,
                    proxy_follow_redirects,
                );
            }

            match &base_url {
                Some(base_url) => base_url
                    .join(&value)
                    .map(|url| {
                        resolved_url_string(
                            url,
                            proxy_path,
                            &proxy_headers,
                            proxy_replace_all,
                            proxy_follow_redirects,
                        )
                    })
                    .unwrap_or(value),
                None => value,
            }
        })
        .collect()
}

/// Resolves extra proxy header templates from runtime parameters and the current request origin.
///
/// # Arguments
///
/// * `params` - Runtime parameters available to query templates and action pipelines.
/// * `request_url` - URL of the current response, used as the base for URL and header templates.
/// * `configured_headers` - Header templates to resolve before encoding a proxied media URL.
fn resolve_proxy_headers<'a>(
    params: &HashMap<String, String>,
    request_url: &str,
    configured_headers: &'a HashMap<String, String>,
) -> Vec<(&'a str, String)> {
    let request_origin = Url::parse(request_url)
        .ok()
        .map(|url| url.origin().ascii_serialization())
        .filter(|origin| origin != "null")
        .unwrap_or_default();

    configured_headers
        .iter()
        .map(|(name, value)| {
            let (value, _missing_keys) =
                query_helpers::replace_template_placeholders(value, params);
            (
                name.as_str(),
                value
                    .replace("{request_url}", request_url)
                    .replace("{request_origin}", &request_origin),
            )
        })
        .collect()
}

/// Returns the non-empty configured public proxy path from runtime parameters.
///
/// # Arguments
///
/// * `params` - Runtime parameters available to query templates and action pipelines.
fn proxy_path(params: &HashMap<String, String>) -> Option<&str> {
    params
        .get(HTTP_PROXY_PUBLIC_PATH_PARAM)
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// Formats a resolved URL, optionally wrapping HTTP(S) targets in the controller proxy route.
///
/// # Arguments
///
/// * `url` - Resolved URL to format, wrapping only HTTP(S) schemes when a proxy route is available.
/// * `proxy_path` - Optional public controller proxy path; absent or blank paths disable wrapping.
/// * `proxy_headers` - Extra proxy-request headers resolved from runtime parameters and the request origin.
/// * `proxy_replace_all` - Post-response replacement rules attached to the generated proxy URL.
/// * `proxy_follow_redirects` - Optional redirect-following policy attached to the generated proxy URL.
fn resolved_url_string(
    url: Url,
    proxy_path: Option<&str>,
    proxy_headers: &[(&str, String)],
    proxy_replace_all: &[ProxyReplaceAllConfig],
    proxy_follow_redirects: Option<&ProxyFollowRedirects>,
) -> String {
    if matches!(url.scheme(), "http" | "https") {
        if let Some(proxy_path) = proxy_path {
            #[cfg(feature = "arachnea-proxy")]
            if !proxy_replace_all.is_empty()
                || !proxy_headers.is_empty()
                || proxy_follow_redirects.is_some()
            {
                let actions = proxy_replace_all
                    .iter()
                    .map(|replacement| {
                        ReplaceAll::config(
                            &replacement.pattern,
                            &replacement.replacement,
                            replacement.content_types.clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                let headers = proxy_headers
                    .iter()
                    .map(|(name, value)| (*name, value.as_str()))
                    .collect::<Vec<_>>();
                return proxied_url_with_countries_and_options(
                    url.as_str(),
                    Some(proxy_path),
                    &[],
                    &actions,
                    &headers,
                    proxy_follow_redirects.and_then(ProxyFollowRedirects::as_json_value),
                );
            }

            #[cfg(not(feature = "arachnea-proxy"))]
            let _ = (proxy_headers, proxy_replace_all, proxy_follow_redirects);
            return format!("{}/{}", proxy_path.trim_end_matches('/'), url);
        }
    }

    url.to_string()
}

/// Resolves `value` against `base_url`, falling back to the directory form of
/// the base when the value is a relative file reference.
///
/// # Arguments
///
/// * `base_url` - Base URL used to resolve relative paths.
/// * `value` - Path or URL string to resolve.
///
/// # Returns
///
/// The resolved [`Url`] or `None` when neither the directory form nor the
/// base URL can join `value`.
fn resolve_relative_url(base_url: &Url, value: &str) -> Option<Url> {
    if should_resolve_against_directory(base_url, value) {
        if let Some(directory_base_url) = directory_base_url(base_url) {
            if let Ok(url) = directory_base_url.join(value) {
                return Some(url);
            }
        }
    }

    base_url.join(value).ok()
}

/// Returns `true` when `value` looks like a relative file reference and
/// should be resolved against the directory form of `base_url` instead of
/// the base URL itself.
///
/// # Arguments
///
/// * `base_url` - Base URL providing the reference path segments.
/// * `value` - Candidate relative path.
///
/// # Returns
///
/// `true` when `value` is non-empty, does not start with `/`, `?`, or `#`,
/// and the last segment of `base_url`'s path is a non-extension file name.
fn should_resolve_against_directory(base_url: &Url, value: &str) -> bool {
    if value.is_empty()
        || value.starts_with('/')
        || value.starts_with('?')
        || value.starts_with('#')
    {
        return false;
    }

    let path = base_url.path();
    if path.ends_with('/') {
        return false;
    }

    base_url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .map(|segment| !segment.is_empty() && !segment.contains('.'))
        .unwrap_or(false)
}

/// Returns a copy of `base_url` whose path always ends with a `/` so
/// `Url::join` resolves sibling references as expected.
///
/// # Arguments
///
/// * `base_url` - Source base URL exposed to templates and relative URL resolution.
fn directory_base_url(base_url: &Url) -> Option<Url> {
    let mut directory_base_url = base_url.clone();
    let path = directory_base_url.path().to_string();
    directory_base_url.set_path(&format!("{}/", path));
    Some(directory_base_url)
}

/// Returns a copy of `base_url` whose path points to the ancestor reached by
/// popping `levels` trailing path segments.
///
/// # Arguments
///
/// * `base_url` - Source base URL exposed to templates and relative URL resolution.
/// * `levels` - Number of trailing path components removed before joining the relative URL.
fn ancestor_base_url(base_url: &Url, levels: usize) -> Option<Url> {
    let mut ancestor_base_url = base_url.clone();
    let mut segments = ancestor_base_url
        .path()
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    for _ in 0..levels {
        if segments.pop().is_none() {
            break;
        }
    }

    let path = if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}/", segments.join("/"))
    };

    ancestor_base_url.set_path(&path);
    Some(ancestor_base_url)
}

#[cfg(test)]
#[path = "../../../tests/scrapyfy/actions/resolve_url.rs"]
mod tests;
