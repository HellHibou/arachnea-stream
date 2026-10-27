use super::*;
use std::time::Duration;

/// Verifies absolute-form forwarding for HTTP upstream proxy requests.
#[tokio::test]
async fn http_request_uses_absolute_form_for_http_upstream() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let accept = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = [0u8; 3];
        stream.read_exact(&mut bytes).await.unwrap();
        bytes
    });
    let core = ArachneaProxyCore::new(ProxyConfig {
        chains: vec![ProxyChain::fixed(
            "proxy",
            vec![ProxyNode::http("proxy", endpoint)],
        )],
        routing: RoutePolicy {
            default_chain: Some("proxy".to_string()),
            allow_direct_fallback: true,
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
    outbound.stream.write_all(b"GET").await.unwrap();
    assert_eq!(accept.await.unwrap(), *b"GET");
}

/// Verifies that HTTP upstream credentials are not sent when no password is defined.
#[tokio::test]
async fn http_upstream_without_password_does_not_send_proxy_authorization() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let accept = tokio::spawn(async move {
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
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .unwrap();
        String::from_utf8(data).unwrap()
    });
    let mut node = ProxyNode::http("proxy", endpoint);
    node.username = Some("tester".to_string());
    let core = ArachneaProxyCore::new(ProxyConfig {
        chains: vec![ProxyChain::fixed("proxy", vec![node])],
        routing: RoutePolicy {
            default_chain: Some("proxy".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    })
    .unwrap();

    let stream = core
        .connect(Destination::host_port("example.com", 443))
        .await;
    assert!(stream.is_ok());
    let request = accept.await.unwrap();
    assert!(!request.to_ascii_lowercase().contains("proxy-authorization"));
    for status in [400, 407, 429, 502, 503] {
        let (mut client, mut peer) = tokio::io::duplex(4096);
        let rejected = tokio::spawn(async move {
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(peer.read_u8().await.unwrap());
            }
            peer.write_all(format!("HTTP/1.1 {status} Rejected\r\n\r\n").as_bytes())
                .await
                .unwrap();
        });
        let error = crate::core::transport::http::connect_tunnel(
            &mut client,
            &Destination::host_port("example.com", 443),
            Duration::from_secs(1),
            None,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, crate::core::ProxyError::HttpConnectRejected { status: actual } if actual == status)
        );
        rejected.await.unwrap();
    }
}

/// Verifies that SOCKS5 upstreams use no-auth when no password is defined.
#[tokio::test]
async fn socks_upstream_without_password_uses_no_auth_method() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let accept = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut methods = [0u8; 3];
        stream.read_exact(&mut methods).await.unwrap();
        stream.write_all(&[0x05, 0x00]).await.unwrap();

        let mut request_head = [0u8; 4];
        stream.read_exact(&mut request_head).await.unwrap();
        assert_eq!(request_head[1], 0x01);
        match request_head[3] {
            0x01 => {
                let mut rest = [0u8; 6];
                stream.read_exact(&mut rest).await.unwrap();
            }
            0x03 => {
                let mut len = [0u8; 1];
                stream.read_exact(&mut len).await.unwrap();
                let mut rest = vec![0u8; len[0] as usize + 2];
                stream.read_exact(&mut rest).await.unwrap();
            }
            0x04 => {
                let mut rest = [0u8; 18];
                stream.read_exact(&mut rest).await.unwrap();
            }
            other => panic!("unexpected SOCKS5 address type {other}"),
        }
        stream
            .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0, 0])
            .await
            .unwrap();
        methods
    });
    let mut node = ProxyNode::socks5("proxy", endpoint);
    node.username = Some("tester".to_string());
    let core = ArachneaProxyCore::new(ProxyConfig {
        chains: vec![ProxyChain::fixed("proxy", vec![node])],
        routing: RoutePolicy {
            default_chain: Some("proxy".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    })
    .unwrap();

    let stream = core
        .connect(Destination::host_port("example.com", 443))
        .await;
    assert!(stream.is_ok());
    assert_eq!(accept.await.unwrap(), [0x05, 0x01, 0x00]);
}

/// Verifies that SOCKS5 targets are locally resolved when hostname forwarding is disabled.
#[tokio::test]
async fn socks_upstream_without_hostname_forwarding_sends_ip_target() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let accept = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut methods = [0u8; 3];
        stream.read_exact(&mut methods).await.unwrap();
        stream.write_all(&[0x05, 0x00]).await.unwrap();

        let mut request_head = [0u8; 4];
        stream.read_exact(&mut request_head).await.unwrap();
        match request_head[3] {
            0x01 => {
                let mut rest = [0u8; 6];
                stream.read_exact(&mut rest).await.unwrap();
            }
            0x04 => {
                let mut rest = [0u8; 18];
                stream.read_exact(&mut rest).await.unwrap();
            }
            other => panic!("expected an IP target, got SOCKS5 address type {other}"),
        }
        stream
            .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0, 0])
            .await
            .unwrap();
        request_head[3]
    });
    let node = ProxyNode::socks5("proxy", endpoint).with_hostname_forwarding(false);
    let core = ArachneaProxyCore::new(ProxyConfig {
        chains: vec![ProxyChain::fixed("proxy", vec![node])],
        routing: RoutePolicy {
            default_chain: Some("proxy".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    })
    .unwrap();

    let stream = core.connect(Destination::host_port("localhost", 443)).await;

    assert!(stream.is_ok());
    assert!(matches!(accept.await.unwrap(), 0x01 | 0x04));
}
