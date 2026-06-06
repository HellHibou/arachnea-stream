use http::{HeaderMap, StatusCode};
use std::time::SystemTime;

/// Known Cloudflare cookie state for one origin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudflareCookieState {
    /// Whether `cf_clearance` exists and is not expired.
    pub has_clearance: bool,
    /// Expiration time for `cf_clearance`, when known.
    pub clearance_expires_at: Option<SystemTime>,
    /// Whether `__cf_bm` exists and is not expired.
    pub has_bot_management: bool,
    /// Whether `_cfuvid` exists and is not expired.
    pub has_cfuvid: bool,
}

/// Cloudflare block detection result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CloudflareDetection {
    /// Whether the response is likely a Cloudflare block.
    pub detected: bool,
    /// Bounded signal summary without cookie values.
    pub reason: String,
}

/// Detects Cloudflare block signals from status, headers, and a body preview.
///
/// # Parameters
///
/// - `status`: HTTP status code returned by the server.
/// - `headers`: Response headers.
/// - `body_preview`: Bounded response body preview.
///
/// # Returns
///
/// A conservative detection result. A plain `403` without Cloudflare signals is
/// not enough to count as a Cloudflare block.
pub(crate) fn detect_cloudflare_block(
    status: StatusCode,
    headers: &HeaderMap,
    body_preview: &[u8],
) -> CloudflareDetection {
    let mut signals = Vec::new();
    if !matches!(
        status,
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
    ) {
        return CloudflareDetection {
            detected: false,
            reason: "status is not a Cloudflare challenge candidate".to_string(),
        };
    }

    if header_contains(headers, "server", "cloudflare") {
        signals.push("server=cloudflare");
    }
    if headers.contains_key("cf-ray") {
        signals.push("cf-ray");
    }
    if headers.contains_key("cf-mitigated") {
        signals.push("cf-mitigated");
    }
    if headers
        .keys()
        .any(|name| name.as_str().starts_with("cf-") || name.as_str().starts_with("__cf"))
    {
        signals.push("cf-prefixed-header");
    }

    let body = String::from_utf8_lossy(body_preview).to_ascii_lowercase();
    for marker in [
        "attention required",
        "just a moment",
        "checking your browser",
        "cf-chl",
        "challenge-platform",
        "cloudflare",
    ] {
        if body.contains(marker) {
            signals.push("body-marker");
            break;
        }
    }

    CloudflareDetection {
        detected: !signals.is_empty(),
        reason: signals.join(", "),
    }
}

/// Checks whether a named header contains a case-insensitive substring.
///
/// # Parameters
///
/// - `headers`: Header map to inspect.
/// - `name`: Header name.
/// - `needle`: ASCII case-insensitive marker.
///
/// # Returns
///
/// `true` when any value for `name` contains `needle`.
fn header_contains(headers: &HeaderMap, name: &str, needle: &str) -> bool {
    headers.get_all(name).iter().any(|value| {
        value
            .to_str()
            .map(|text| text.to_ascii_lowercase().contains(needle))
            .unwrap_or(false)
    })
}

#[cfg(test)]
/// Unit tests for Cloudflare detection heuristics.
mod tests {
    use http::header::SERVER;

    use super::*;

    /// Verifies that a plain forbidden response does not trigger fallback.
    #[test]
    fn plain_forbidden_is_not_cloudflare() {
        let result = detect_cloudflare_block(StatusCode::FORBIDDEN, &HeaderMap::new(), b"nope");
        assert!(!result.detected);
    }

    /// Verifies that Cloudflare response headers trigger fallback detection.
    #[test]
    fn cloudflare_headers_are_detected() {
        let mut headers = HeaderMap::new();
        headers.insert(SERVER, "cloudflare".parse().expect("valid server header"));
        headers.insert("cf-ray", "abc".parse().expect("valid cf-ray header"));

        let result = detect_cloudflare_block(StatusCode::FORBIDDEN, &headers, b"");
        assert!(result.detected);
    }
}
