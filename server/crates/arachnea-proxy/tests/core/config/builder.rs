use super::*;

/// Verifies that the privacy profile disables direct fallback.
#[test]
fn privacy_profile_disables_direct_fallback() {
    let config = ProxyConfig::builder()
        .profile(ProxyProfile::Privacy)
        .build()
        .unwrap();
    assert!(!config.as_config().routing.allow_direct_fallback);
}

/// Verifies that the builder creates a chain for a single upstream.
#[test]
fn builder_adds_single_upstream_chain() {
    let config = ArachneaProxyCore::builder()
        .upstream(ProxyNode::socks5("tor", "127.0.0.1:9050"))
        .build()
        .unwrap();
    assert!(config.chain("tor").is_some());
}
