use super::*;

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
