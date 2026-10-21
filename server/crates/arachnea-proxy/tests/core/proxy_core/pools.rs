use super::*;

/// Verifies that chain nodes can reference declared proxy pools.
#[test]
fn proxy_pool_chain_marker_resolves() {
    let config = ProxyConfig {
        upstreams: vec![ProxyNode::http("http-a", "127.0.0.1:8080")],
        egress_pools: vec![EgressPool::new(
            "pool-a",
            vec![ProxyNode::http("http-a", "127.0.0.1:8080")],
            None,
        )],
        chains: vec![ProxyChain::pool("pooled", "pool-a")],
        routing: RoutePolicy {
            default_chain: Some("pooled".to_string()),
            allow_direct_fallback: false,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    }
    .resolve()
    .unwrap();

    let chain = config.chain("pooled").unwrap();
    assert_eq!(chain.nodes[0].kind, TransportKind::ProxyPool);
    assert_eq!(chain.nodes[0].name, "pool-a");
}

/// Verifies that pools cannot contain direct nodes.
#[test]
fn proxy_pool_rejects_direct_member() {
    let config = ProxyConfig {
        egress_pools: vec![EgressPool::new(
            "pool-a",
            vec![ProxyNode::direct("direct")],
            None,
        )],
        chains: vec![ProxyChain::pool("pooled", "pool-a")],
        routing: RoutePolicy {
            default_chain: Some("pooled".to_string()),
            allow_direct_fallback: false,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    };

    assert!(matches!(config.resolve(), Err(ProxyError::Config(_))));
}

/// Verifies that proxy pools skip failed members and cache the first working candidate.
#[tokio::test]
async fn proxy_pool_selects_and_caches_first_working_member() {
    let bad_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bad_endpoint = bad_listener.local_addr().unwrap().to_string();
    let bad_accept = tokio::spawn(async move {
        let (mut stream, _) = bad_listener.accept().await.unwrap();
        let mut data = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            stream.read_exact(&mut byte).await.unwrap();
            data.push(byte[0]);
            if data.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        stream
            .write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n")
            .await
            .unwrap();
        String::from_utf8(data).unwrap()
    });

    let good_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let good_endpoint = good_listener.local_addr().unwrap().to_string();
    let good_accept = tokio::spawn(async move {
        let mut requests = Vec::new();
        for _ in 0..2 {
            let (mut stream, _) = good_listener.accept().await.unwrap();
            let mut data = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                stream.read_exact(&mut byte).await.unwrap();
                data.push(byte[0]);
                if data.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            stream
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .unwrap();
            requests.push(String::from_utf8(data).unwrap());
        }
        requests
    });

    let core = ArachneaProxyCore::new(ProxyConfig {
        upstreams: vec![
            ProxyNode::http("bad", bad_endpoint.clone()),
            ProxyNode::http("good", good_endpoint.clone()),
        ],
        egress_pools: vec![EgressPool::new(
            "pool",
            vec![
                ProxyNode::http("bad", bad_endpoint.clone()),
                ProxyNode::http("good", good_endpoint.clone()),
            ],
            Some(1),
        )],
        chains: vec![ProxyChain::pool("pooled", "pool")],
        routing: RoutePolicy {
            default_chain: Some("pooled".to_string()),
            allow_direct_fallback: false,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    })
    .unwrap();

    let initial = core.proxy_pool_statuses("pool").unwrap();
    assert!(initial
        .iter()
        .all(|state| state.status == ProxyPoolMemberStatus::Untested));

    let first = core
        .connect(Destination::host_port("example.com", 443))
        .await;
    assert!(first.is_ok());
    drop(first);

    let statuses = core.proxy_pool_statuses("pool").unwrap();
    assert_eq!(statuses[0].status, ProxyPoolMemberStatus::Ko);
    assert!(!statuses[0].selected);
    // Tunnel selection runs no preflight, so the working member stays Untested.
    assert_eq!(statuses[1].status, ProxyPoolMemberStatus::Untested);
    assert!(statuses[1].selected);

    let second = core
        .connect(Destination::host_port("example.com", 443))
        .await;
    assert!(second.is_ok());
    let bad_request = bad_accept.await.unwrap();
    assert!(bad_request.starts_with("CONNECT example.com:443 "));
    let good_requests = good_accept.await.unwrap();
    assert_eq!(good_requests.len(), 2);
    assert!(good_requests
        .iter()
        .all(|request| request.starts_with("CONNECT example.com:443 ")));
}

/// Verifies that a cached pool member is marked failed and replaced on setup failure.
#[tokio::test]
async fn proxy_pool_fails_over_when_cached_member_stops_accepting_connections() {
    let first_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let first_endpoint = first_listener.local_addr().unwrap().to_string();
    let first_accept = tokio::spawn(async move {
        let (mut stream, _) = first_listener.accept().await.unwrap();
        let mut data = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            stream.read_exact(&mut byte).await.unwrap();
            data.push(byte[0]);
            if data.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        stream
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .unwrap();
        // Returning drops the listener, so the cached member stops accepting.
        String::from_utf8(data).unwrap()
    });

    let second_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second_endpoint = second_listener.local_addr().unwrap().to_string();
    let second_accept = tokio::spawn(async move {
        let (mut stream, _) = second_listener.accept().await.unwrap();
        let mut data = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            stream.read_exact(&mut byte).await.unwrap();
            data.push(byte[0]);
            if data.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        stream
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .unwrap();
        String::from_utf8(data).unwrap()
    });

    let core = ArachneaProxyCore::new(ProxyConfig {
        upstreams: vec![
            ProxyNode::http("first", first_endpoint.clone()),
            ProxyNode::http("second", second_endpoint.clone()),
        ],
        egress_pools: vec![EgressPool::new(
            "pool",
            vec![
                ProxyNode::http("first", first_endpoint.clone()),
                ProxyNode::http("second", second_endpoint.clone()),
            ],
            None,
        )],
        chains: vec![ProxyChain::pool("pooled", "pool")],
        routing: RoutePolicy {
            default_chain: Some("pooled".to_string()),
            allow_direct_fallback: false,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    })
    .unwrap();

    let first = core
        .connect(Destination::host_port("example.com", 443))
        .await;
    assert!(first.is_ok());
    drop(first);
    assert!(core.proxy_pool_statuses("pool").unwrap()[0].selected);
    let first_request = first_accept.await.unwrap();
    assert!(first_request.starts_with("CONNECT example.com:443 "));

    let second = core
        .connect(Destination::host_port("example.com", 443))
        .await;
    assert!(second.is_ok());
    drop(second);
    let statuses = core.proxy_pool_statuses("pool").unwrap();
    let second_request = second_accept.await.unwrap();

    assert_eq!(statuses[0].status, ProxyPoolMemberStatus::Ko);
    assert!(!statuses[0].selected);
    // Tunnel selection runs no preflight, so the replacement stays Untested.
    assert!(statuses[1].selected);
    assert!(second_request.starts_with("CONNECT example.com:443 "));
}

/// Verifies that a final HTTP proxy pool can relay plain HTTP without CONNECT.
#[tokio::test]
async fn proxy_pool_final_http_proxy_accepts_absolute_form_http() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let accept = tokio::spawn(async move {
        let (mut probe, _) = listener.accept().await.unwrap();
        let mut ignored = [0u8; 1];
        assert_eq!(probe.read(&mut ignored).await.unwrap(), 0);

        let (mut stream, _) = listener.accept().await.unwrap();
        let mut data = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            stream.read_exact(&mut byte).await.unwrap();
            data.push(byte[0]);
            if data.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .await
            .unwrap();
        String::from_utf8(data).unwrap()
    });

    let core = ArachneaProxyCore::new(ProxyConfig {
        upstreams: vec![ProxyNode::http("http-proxy", endpoint.clone())],
        egress_pools: vec![EgressPool::new(
            "pool",
            vec![ProxyNode::http("http-proxy", endpoint.clone())],
            None,
        )],
        chains: vec![ProxyChain::pool("pooled", "pool")],
        routing: RoutePolicy {
            default_chain: Some("pooled".to_string()),
            allow_direct_fallback: false,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    })
    .unwrap();
    let mut outbound = core
        .connect_http_request(ConnectRequest::new(
            Destination::host_port("example.com", 80).with_protocol(ApplicationProtocol::Http),
        ))
        .await
        .unwrap();

    assert_eq!(outbound.target_form, HttpRequestTargetForm::AbsoluteForm);
    outbound
        .stream
        .write_all(b"GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\n\r\n")
        .await
        .unwrap();
    let mut body = Vec::new();
    outbound.stream.read_to_end(&mut body).await.unwrap();
    let request = accept.await.unwrap();

    assert!(request.starts_with("GET http://example.com/ "));
    assert_eq!(
        core.proxy_pool_statuses("pool").unwrap()[0].status,
        ProxyPoolMemberStatus::Ok
    );
}

/// Verifies that a SOCKS5 pool selection falls back to local DNS resolution
/// when the proxy rejects domain-form requests.
#[tokio::test]
async fn proxy_pool_socks5_selection_uses_dns_fallback() {
    let target = spawn_echo_target_accepting(2).await;
    let proxy_addr = spawn_socks5_proxy_rejecting_domain_requests(3).await;
    let node = ProxyNode::socks5("socks", proxy_addr.to_string())
        .with_dns_resolution(ProxyNameResolutionMode::ProxyThenLocalFallback);
    let core = ArachneaProxyCore::new(ProxyConfig {
        upstreams: vec![node.clone()],
        egress_pools: vec![EgressPool::new("pool", vec![node], None)],
        chains: vec![ProxyChain::pool("pooled", "pool")],
        routing: RoutePolicy {
            default_chain: Some("pooled".to_string()),
            allow_direct_fallback: false,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    })
    .unwrap();

    let mut stream = core
        .connect(Destination::host_port("127.0.0.1", target.port()))
        .await
        .unwrap();
    stream.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    stream.read_exact(&mut echoed).await.unwrap();
    let statuses = core.proxy_pool_statuses("pool").unwrap();

    assert_eq!(&echoed, b"ping");
    assert!(statuses[0].selected);
    // Tunnel selection runs no preflight, so the member status stays Untested.
    assert_eq!(statuses[0].status, ProxyPoolMemberStatus::Untested);
}
