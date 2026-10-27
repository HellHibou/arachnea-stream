//! Bounded literal JavaScript redirect recognition.

use super::*;

/// Extracts the target of the first **simple** JavaScript redirect from an HTML document.
///
/// Only `window.location.href = "…"` (or `'…'`) string-literal assignments are considered, so
/// values computed at runtime (e.g. `window.location.href = currentUrl.toString();`) are skipped
/// in favor of the next literal assignment. Returns the first non-empty literal target found, or
/// `None` when the document contains no eligible simple redirect.
pub(super) fn extract_simple_js_redirect_target(html: &str) -> Option<String> {
    let mut search_from = 0;
    while let Some(start) = html[search_from..].find("window.location.href") {
        let after = search_from + start + "window.location.href".len();
        let after_trimmed = html[after..].trim_start_matches([' ', '\t']);
        if let Some(after_eq) = after_trimmed.strip_prefix('=') {
            let quote_start = after_eq.trim_start_matches([' ', '\t']);
            let quote = quote_start.chars().next()?;
            if quote == '\'' || quote == '"' {
                let body = &quote_start[quote.len_utf8()..];
                if let Some(end) = body.find(quote) {
                    let target = body[..end].trim();
                    if !target.is_empty() {
                        return Some(target.to_string());
                    }
                }
            }
        }
        search_from = after;
    }
    None
}

/// Resolves a JS redirect target against the page URL that emitted it.
///
/// Absolute `http(s)` targets are kept as-is. Relative targets are joined onto the current page
/// URL. Returns `None` when the target cannot be resolved to an HTTP(S) URL.
pub(super) fn resolve_redirect_target(current_url: &str, target: &str) -> Option<String> {
    if target.starts_with("http://") || target.starts_with("https://") {
        return Some(target.to_string());
    }

    let base = Url::parse(current_url).ok()?;
    base.join(target)
        .ok()
        .map(|resolved| resolved.to_string())
        .filter(|resolved| resolved.starts_with("http://") || resolved.starts_with("https://"))
}
