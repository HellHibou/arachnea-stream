use super::*;

/// Verifies that new profile names serialize while legacy names still parse.
#[test]
fn proxy_profile_accepts_new_and_legacy_names() {
    let current = serde_json::to_string(&ProxyProfile::Direct).unwrap();
    let legacy: ProxyProfile = serde_json::from_str("\"direct\"").unwrap();
    let current_single: ProxyProfile = serde_json::from_str("\"single_forwarder\"").unwrap();
    let legacy_single: ProxyProfile = serde_json::from_str("\"single_proxy\"").unwrap();

    assert_eq!(current, "\"system_relay\"");
    assert_eq!(legacy, ProxyProfile::Direct);
    assert_eq!(current_single, ProxyProfile::SingleProxy);
    assert_eq!(legacy_single, ProxyProfile::SingleProxy);
}

/// Verifies that the privacy profile disables direct fallback.
#[test]
fn privacy_profile_disables_direct_fallback() {
    let config = ProxyConfig::builder()
        .profile(ProxyProfile::Privacy)
        .build()
        .unwrap();
    assert!(!config.as_config().routing.allow_direct_fallback);
}

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
