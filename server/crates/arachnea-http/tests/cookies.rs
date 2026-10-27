use http::header::SET_COOKIE;

use super::*;

/// Builds a header map with one `Set-Cookie` header.
///
/// # Parameters
///
/// - `value`: Cookie header value.
///
/// # Returns
///
/// A header map containing the supplied cookie.
fn set_cookie_header(value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.append(SET_COOKIE, value.parse().expect("valid Set-Cookie header"));
    headers
}

/// Verifies that Max-Age cookies are parsed and emitted.
#[test]
fn parses_max_age_cookie() {
    let mut cache = SharedCookieCache::default();
    cache
        .set_from_set_cookie_headers(
            "https://example.com/a/b",
            &set_cookie_header("cf_clearance=ok; Max-Age=60; Path=/"),
            Duration::from_secs(3600),
        )
        .expect("cookie parse succeeds");

    let header = cache
        .cookie_header_for("https://example.com/a")
        .expect("cookie header succeeds")
        .expect("cookie exists");
    assert_eq!(header, "cf_clearance=ok");
}

/// Verifies that Max-Age takes precedence over an expired Expires attribute.
#[test]
fn max_age_takes_precedence_over_expires() {
    let mut cache = SharedCookieCache::default();
    cache
        .set_from_set_cookie_headers(
            "https://example.com/",
            &set_cookie_header(
                "cf_clearance=ok; Max-Age=60; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Path=/",
            ),
            Duration::from_secs(3600),
        )
        .expect("cookie parse succeeds");

    assert_eq!(
        cache
            .cookie_header_for("https://example.com/")
            .expect("cookie header succeeds")
            .as_deref(),
        Some("cf_clearance=ok")
    );
}

/// Verifies that Max-Age=0 removes a stored cookie.
#[test]
fn max_age_zero_removes_cookie() {
    let mut cache = SharedCookieCache::default();
    cache
        .set_from_set_cookie_headers(
            "https://example.com/",
            &set_cookie_header("cf_clearance=ok; Max-Age=60; Path=/"),
            Duration::from_secs(3600),
        )
        .expect("initial cookie parse succeeds");
    cache
        .set_from_set_cookie_headers(
            "https://example.com/",
            &set_cookie_header("cf_clearance=gone; Max-Age=0; Path=/"),
            Duration::from_secs(3600),
        )
        .expect("deletion cookie parse succeeds");

    assert!(cache
        .cookie_header_for("https://example.com/")
        .expect("cookie header succeeds")
        .is_none());
}

/// Verifies domain, path, and secure cookie filtering.
#[test]
fn filters_by_domain_path_and_secure() {
    let mut cache = SharedCookieCache::default();
    cache.insert_cookie(CookieEntry {
        name: "a".to_string(),
        value: "b".to_string(),
        domain: "example.com".to_string(),
        path: "/private".to_string(),
        expires_at: Some(SystemTime::now() + Duration::from_secs(60)),
        secure: true,
        http_only: false,
        same_site: None,
    });

    assert!(cache
        .cookie_header_for("http://example.com/private")
        .expect("cookie header succeeds")
        .is_none());
    assert!(cache
        .cookie_header_for("https://example.com/public")
        .expect("cookie header succeeds")
        .is_none());
    assert_eq!(
        cache
            .cookie_header_for("https://sub.example.com/private/page")
            .expect("cookie header succeeds")
            .as_deref(),
        Some("a=b")
    );
}
