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
