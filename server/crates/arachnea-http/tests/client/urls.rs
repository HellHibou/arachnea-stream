use super::*;

/// Verifies origin normalization keeps the scheme, host, and port.
#[test]
fn origin_url_keeps_authority() {
    assert_eq!(
        origin_url("https://example.com:8443/path?q=1").expect("valid origin"),
        "https://example.com:8443/"
    );
}
