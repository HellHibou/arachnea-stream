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

/// Verifies that duplicate node names inside a chain are rejected.
#[test]
fn duplicate_node_names_inside_chain_are_rejected() {
    let config = ProxyConfig {
        chains: vec![ProxyChain::fixed(
            "proxy",
            vec![
                ProxyNode::socks5("hop", "127.0.0.1:9050"),
                ProxyNode::http("hop", "127.0.0.1:8080"),
            ],
        )],
        routing: RoutePolicy {
            default_chain: Some("proxy".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    };
    assert!(matches!(config.resolve(), Err(ProxyError::Config(_))));
}

/// Verifies that `direct` cannot be mixed with proxy hops in one chain.
#[test]
fn mixed_direct_and_proxy_nodes_are_rejected() {
    let config = ProxyConfig {
        chains: vec![ProxyChain::fixed(
            "mixed",
            vec![
                ProxyNode::direct("direct"),
                ProxyNode::socks5("proxy", "127.0.0.1:9050"),
            ],
        )],
        routing: RoutePolicy {
            default_chain: Some("mixed".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    };
    assert!(matches!(config.resolve(), Err(ProxyError::Config(_))));
}

/// Verifies that malformed proxy endpoints are rejected.
#[test]
fn invalid_proxy_endpoint_is_rejected() {
    let config = ProxyConfig {
        chains: vec![ProxyChain::fixed(
            "proxy",
            vec![ProxyNode::socks5("proxy", "127.0.0.1")],
        )],
        routing: RoutePolicy {
            default_chain: Some("proxy".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    };
    assert!(matches!(config.resolve(), Err(ProxyError::Config(_))));
}

/// Verifies that global TLS verification settings control HTTPS upstream checks.
#[test]
fn disabling_global_upstream_tls_verification_allows_insecure_https_node() {
    let mut node = ProxyNode::https("https", "proxy.example:443");
    node.verify_tls = false;
    let config = ProxyConfig {
        chains: vec![ProxyChain::fixed("https", vec![node])],
        routing: RoutePolicy {
            default_chain: Some("https".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        security: SecurityPolicy {
            verify_upstream_tls: false,
            ..SecurityPolicy::default()
        },
        ..ProxyConfig::default()
    };
    assert!(config.resolve().is_ok());
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

/// Verifies that a core handle can append a proxy hop after construction.
#[test]
fn core_appends_proxy_to_default_chain() {
    let mut core =
        ArachneaProxyCore::new(ProxyNode::http("first", "127.0.0.1:8080").config()).unwrap();

    core.append_proxy(ProxyNode::socks5("second", "127.0.0.1:9050"))
        .unwrap();

    let chain = core.config().chain("first").unwrap();
    assert_eq!(chain.nodes.len(), 2);
    assert_eq!(chain.nodes[0].name, "first");
    assert_eq!(chain.nodes[1].name, "second");
}
