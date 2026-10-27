use super::*;

/// Verifies that proxy URL helpers create the expected proxy node.
#[test]
fn proxy_node_from_url_uses_scheme_defaults() {
    let node = ProxyNode::from_url("local-socks", "socks5://127.0.0.1").unwrap();
    let socks4 = ProxyNode::from_url("legacy-socks", "socks4://127.0.0.1").unwrap();
    let socks4a = ProxyNode::from_url("legacy-socks4a", "socks4a://127.0.0.1").unwrap();
    assert_eq!(node.kind, TransportKind::Socks5);
    assert_eq!(node.endpoint.as_deref(), Some("127.0.0.1:1080"));
    assert_eq!(
        node.effective_name_resolution(),
        ProxyNameResolutionMode::ProxyThenLocalFallback
    );
    assert_eq!(socks4.kind, TransportKind::Socks4);
    assert_eq!(socks4.endpoint.as_deref(), Some("127.0.0.1:1080"));
    assert_eq!(
        socks4.effective_name_resolution(),
        ProxyNameResolutionMode::Local
    );
    assert_eq!(socks4a.kind, TransportKind::Socks4);
    assert_eq!(
        socks4a.effective_name_resolution(),
        ProxyNameResolutionMode::ProxyOnly
    );
}

/// Verifies that SOCKS5 URLs can receive an explicit DNS resolution mode.
#[test]
fn proxy_node_from_socks5_url_accepts_dns_resolution_mode() {
    let node = ProxyNode::from_socks5_url(
        "local-socks",
        "socks5://127.0.0.1",
        ProxyNameResolutionMode::ProxyOnly,
    )
    .unwrap();
    let http = ProxyNode::from_socks5_url(
        "not-socks",
        "http://127.0.0.1",
        ProxyNameResolutionMode::ProxyOnly,
    );

    assert_eq!(node.kind, TransportKind::Socks5);
    assert_eq!(
        node.effective_name_resolution(),
        ProxyNameResolutionMode::ProxyOnly
    );
    assert!(matches!(http, Err(ProxyError::Config(_))));
}

/// Verifies that explicit proxy DNS modes override legacy hostname forwarding.
#[test]
fn proxy_node_dns_resolution_mode_overrides_send_hostname() {
    let default = ProxyNode::socks5("default-dns", "127.0.0.1:9050");
    let local = ProxyNode::socks5("local-dns", "127.0.0.1:9050")
        .with_dns_resolution(ProxyNameResolutionMode::Local);
    let fallback = ProxyNode::socks5("fallback-dns", "127.0.0.1:9050")
        .with_dns_resolution(ProxyNameResolutionMode::ProxyThenLocalFallback);
    let legacy = ProxyNode::socks5("legacy", "127.0.0.1:9050").with_hostname_forwarding(false);

    assert_eq!(
        default.effective_name_resolution(),
        ProxyNameResolutionMode::ProxyThenLocalFallback
    );
    assert_eq!(
        local.effective_name_resolution(),
        ProxyNameResolutionMode::Local
    );
    assert_eq!(
        fallback.effective_name_resolution(),
        ProxyNameResolutionMode::ProxyThenLocalFallback
    );
    assert_eq!(
        legacy.effective_name_resolution(),
        ProxyNameResolutionMode::Local
    );
}

/// Verifies that malformed proxy URL ports are rejected early.
#[test]
fn proxy_node_from_url_rejects_invalid_ports() {
    let result = ProxyNode::from_url("bad", "socks5://127.0.0.1:not-a-port");
    assert!(matches!(result, Err(ProxyError::Config(_))));
}

/// Verifies that fixed chains retain their node order.
#[test]
fn fixed_chain_keeps_nodes() {
    let chain = ProxyChain::fixed(
        "chain",
        vec![
            ProxyNode::socks5("first", "127.0.0.1:9050"),
            ProxyNode::http("second", "127.0.0.1:8080"),
        ],
    );
    assert_eq!(chain.nodes.len(), 2);
}
