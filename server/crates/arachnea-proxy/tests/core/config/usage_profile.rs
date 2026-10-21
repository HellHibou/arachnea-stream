use super::*;

/// Verifies that a single-forwarder usage profile builds one default proxy chain.
#[test]
fn usage_profile_single_forwarder_builds_default_chain() {
    let config = UsageProfile::SingleForwarder(ProxyNode::socks5("tor", "127.0.0.1:9050")).config();
    assert_eq!(config.routing.default_chain.as_deref(), Some("tor"));
    assert!(!config.routing.allow_direct_fallback);
    assert_eq!(config.chains[0].nodes[0].kind, TransportKind::Socks5);
}

/// Verifies that a multi-forwarder usage profile keeps hop order.
#[test]
fn usage_profile_multi_forwarder_keeps_hop_order() {
    let config = UsageProfile::MultiForwarder(vec![
        ProxyNode::http("first", "127.0.0.1:8080"),
        ProxyNode::socks5("second", "127.0.0.1:9050"),
    ])
    .config();
    let chain = config.chains.first().unwrap();
    assert_eq!(chain.name, "multi_forwarder");
    assert_eq!(chain.nodes[0].name, "first");
    assert_eq!(chain.nodes[1].name, "second");
}

/// Verifies that a privacy usage profile disables direct fallback.
#[test]
fn usage_profile_privacy_disables_direct_fallback() {
    let config = UsageProfile::Privacy(vec![ProxyNode::socks5("tor", "127.0.0.1:9050")]).config();
    assert_eq!(config.profile, ProxyProfile::Privacy);
    assert!(!config.routing.allow_direct_fallback);
}

/// Verifies that usage profile timeouts are applied to generated configs.
#[test]
fn usage_profile_config_with_timeouts_sets_core_timeouts() {
    let timeouts = TimeoutConfig {
        connect_ms: 1_500,
        handshake_ms: 2_500,
        idle_ms: 3_500,
    };

    let config = UsageProfile::SingleForwarder(ProxyNode::socks5("proxy", "127.0.0.1:9050"))
        .config_with_timeouts(timeouts.clone());

    assert_eq!(config.timeouts, timeouts);
}
