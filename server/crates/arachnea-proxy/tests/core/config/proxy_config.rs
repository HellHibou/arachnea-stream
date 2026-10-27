use super::*;

/// Verifies that the default proxy configuration resolves to the direct chain.
#[test]
fn default_config_resolves_to_direct() {
    let config = ProxyConfig::default().resolve().unwrap();
    assert!(config.chain("direct").is_some());
}

/// Verifies that an undeclared default chain is rejected.
#[test]
fn invalid_default_chain_is_rejected() {
    let config = ProxyConfig {
        routing: RoutePolicy {
            default_chain: Some("missing".to_string()),
            allow_direct_fallback: false,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    };
    assert!(matches!(config.resolve(), Err(ProxyError::Config(_))));
}

/// Verifies that route rules cannot reference undeclared chains.
#[test]
fn route_rule_with_missing_chain_is_rejected() {
    let config = ProxyConfig {
        routing: RoutePolicy {
            default_chain: Some("direct".to_string()),
            allow_direct_fallback: true,
            rules: vec![RouteRule::to_chain("missing")],
        },
        ..ProxyConfig::default()
    };
    assert!(matches!(config.resolve(), Err(ProxyError::Config(_))));
}

/// Verifies that duplicate chain names are rejected.
#[test]
fn duplicate_chain_names_are_rejected() {
    let config = ProxyConfig {
        chains: vec![ProxyChain::direct(), ProxyChain::direct()],
        ..ProxyConfig::default()
    };
    assert!(matches!(config.resolve(), Err(ProxyError::Config(_))));
}

/// Verifies that an extra proxy hop can be appended to a generated chain.
#[test]
fn config_appends_extra_proxy_to_generated_chain() {
    let config = ProxyNode::http("first", "127.0.0.1:8080")
        .config()
        .append_proxy(ProxyNode::socks5("second", "127.0.0.1:9050"))
        .unwrap()
        .resolve()
        .unwrap();
    let chain = config.chain("first").unwrap();
    assert_eq!(chain.nodes.len(), 2);
    assert_eq!(chain.nodes[1].name, "second");
}
