use url::Url;

/// Applies the `resolve_url` scraper action by resolving every value as a URL
/// relative to `request_url`.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline.
/// * `request_url` - Final URL used to fetch the current page; used as the
///   resolution base for relative paths.
///
/// # Returns
///
/// A new value list where every value is either a fully qualified URL or
/// resolved against `request_url`. Values that fail to parse and cannot be
/// joined onto the base are kept as-is.
pub(super) fn apply(texts: Vec<String>, request_url: &str) -> Vec<String> {
    let base_url = Url::parse(request_url).ok();

    texts
        .into_iter()
        .map(|value| {
            if let Ok(url) = Url::parse(&value) {
                return url.to_string();
            }

            match &base_url {
                Some(base_url) => resolve_relative_url(base_url, &value)
                    .map(|url| url.to_string())
                    .unwrap_or(value),
                _none => value,
            }
        })
        .collect()
}

/// Applies the `resolve_url_from_parent` scraper action by resolving every
/// value against an ancestor directory of `request_url`.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline.
/// * `request_url` - Final URL used to fetch the current page.
/// * `levels` - Number of trailing path segments to remove from
///   `request_url` before joining the relative value.
///
/// # Returns
///
/// A new value list where every value is either a fully qualified URL or
/// joined onto the ancestor of `request_url`. Unparseable values that
/// cannot be joined are kept as-is.
pub(super) fn apply_from_parent(
    texts: Vec<String>,
    request_url: &str,
    levels: usize,
) -> Vec<String> {
    let base_url = Url::parse(request_url)
        .ok()
        .and_then(|url| ancestor_base_url(&url, levels));

    texts
        .into_iter()
        .map(|value| {
            if let Ok(url) = Url::parse(&value) {
                return url.to_string();
            }

            match &base_url {
                Some(base_url) => base_url
                    .join(&value)
                    .map(|url| url.to_string())
                    .unwrap_or(value),
                None => value,
            }
        })
        .collect()
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
fn directory_base_url(base_url: &Url) -> Option<Url> {
    let mut directory_base_url = base_url.clone();
    let path = directory_base_url.path().to_string();
    directory_base_url.set_path(&format!("{}/", path));
    Some(directory_base_url)
}

/// Returns a copy of `base_url` whose path points to the ancestor reached by
/// popping `levels` trailing path segments.
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
