use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use crate::core::{
    ApplicationProtocol, ArachneaProxyCore, Destination, ProxyChain, ProxyConfig,
    ProxyNameResolutionMode, ProxyNode, RoutePolicy, SecurityPolicy,
};
use base64::Engine;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig as RustlsServerConfig;
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;

use crate::server::{handlers, ProxyAuthConfig, ServerConfig};

/// Verifies that the HTTP proxy relays absolute-form HTTP requests directly.
#[tokio::test]
async fn http_proxy_relays_absolute_request() {
    let target = spawn_http_target().await;
    let core = ArachneaProxyCore::new(ProxyConfig::default()).unwrap();
    let config = Arc::new(ServerConfig::default());
    let (mut client, server_side) = duplex(16 * 1024);
    let peer = SocketAddr::from((Ipv4Addr::LOCALHOST, 50_000));
    let handler =
        tokio::spawn(async move { handlers::http::handle(server_side, peer, config, core).await });

    let request = format!(
        "GET http://{}/hello HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        target, target
    );
    client.write_all(request.as_bytes()).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    handler.await.unwrap().unwrap();

    let text = String::from_utf8(response).unwrap();
    assert!(text.starts_with("HTTP/1.1 200 OK"));
    assert!(text.ends_with("proxied"));

    let runtime = Arc::new(crate::server::LoopbackProxyRuntime::new(0));
    let (first, second) = tokio::join!(runtime.start(), runtime.start());
    let addr = first.unwrap();
    assert_eq!(addr, second.unwrap());
    assert!(addr.ip().is_loopback());
    assert_ne!(addr.port(), 0);
    let occupied = crate::server::LoopbackProxyRuntime::new(addr.port());
    assert_eq!(
        occupied.start().await.unwrap_err().kind(),
        std::io::ErrorKind::AddrInUse
    );
    let lease = runtime
        .register(
            ArachneaProxyCore::new(ProxyConfig::default())
                .unwrap()
                .with_parameter_handler(crate::core::DynamicCountryRoutingProxyHandler::new(
                    crate::core::CoexistencePolicy::StaticOnly,
                )),
            crate::core::ClientContext::new(),
            true,
        )
        .unwrap();
    runtime.set_default_route(&lease).unwrap();
    for (authorization, expected) in [
        (None, "407 Proxy Authentication Required"),
        (Some("invalid:local"), "400 Bad Request"),
        (Some("default:wrong"), "400 Bad Request"),
        (Some("arachnea-context:expired"), "403 Forbidden"),
        (Some("default:local"), "200 OK"),
        (
            Some("countries=be,FR&proxy_affinity=browser-session-1:local"),
            "200 OK",
        ),
        (Some("countries=BE&countries=FR:local"), "400 Bad Request"),
        (Some("countries=invalid:local"), "400 Bad Request"),
    ] {
        let target = if expected == "200 OK" {
            spawn_http_target().await
        } else {
            target
        };
        let mut client = TcpStream::connect(addr).await.unwrap();
        let auth = authorization
            .map(|credentials| {
                format!(
                    "Proxy-Authorization: Basic {}\r\n",
                    base64::engine::general_purpose::STANDARD.encode(credentials),
                )
            })
            .unwrap_or_default();
        client.write_all(format!(
            "GET http://{target}/hello HTTP/1.1\r\nHost: {target}\r\n{auth}Connection: close\r\n\r\n",
        ).as_bytes()).await.unwrap();
        let mut response = Vec::new();
        timeout(Duration::from_secs(5), client.read_to_end(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&response).starts_with(&format!("HTTP/1.1 {expected}")),
            "expected {expected}, received {}",
            String::from_utf8_lossy(&response)
        );
    }
    runtime.shutdown();
    assert!(runtime.start().await.is_err());
    timeout(Duration::from_secs(5), async {
        while TcpStream::connect(addr).await.is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

/// Verifies that HTTP proxy authentication is disabled when no password is configured.
#[tokio::test]
async fn http_proxy_does_not_challenge_without_password_env() {
    let target = spawn_http_target().await;
    let core = ArachneaProxyCore::new(ProxyConfig::default()).unwrap();
    let server_config = ServerConfig {
        authentication: ProxyAuthConfig {
            username: Some("tester".to_string()),
            password_env: None,
            realm: "Arachnea Test".to_string(),
        },
        ..Default::default()
    };
    let config = Arc::new(server_config);
    let (mut client, server_side) = duplex(16 * 1024);
    let peer = SocketAddr::from((Ipv4Addr::LOCALHOST, 50_001));
    let handler =
        tokio::spawn(async move { handlers::http::handle(server_side, peer, config, core).await });

    let request = format!(
        "GET http://{}/hello HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        target, target
    );
    client.write_all(request.as_bytes()).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    handler.await.unwrap().unwrap();

    let text = String::from_utf8(response).unwrap();
    assert!(text.starts_with("HTTP/1.1 200 OK"));
    assert!(!text.contains("407 Proxy Authentication Required"));
}

/// Verifies that HTTP CONNECT establishes a bidirectional tunnel.
#[tokio::test]
async fn http_connect_establishes_tcp_tunnel() {
    let target = spawn_echo_target().await;
    let core = ArachneaProxyCore::new(ProxyConfig::default()).unwrap();
    let config = Arc::new(ServerConfig::default());
    let (mut client, server_side) = duplex(16 * 1024);
    let peer = SocketAddr::from((Ipv4Addr::LOCALHOST, 50_002));
    let handler =
        tokio::spawn(async move { handlers::http::handle(server_side, peer, config, core).await });

    let request = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n");
    client.write_all(request.as_bytes()).await.unwrap();
    let mut response = [0u8; 39];
    client.read_exact(&mut response).await.unwrap();
    assert_eq!(&response, b"HTTP/1.1 200 Connection Established\r\n\r\n");

    client.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    client.read_exact(&mut echoed).await.unwrap();
    assert_eq!(&echoed, b"ping");
    drop(client);
    handler.await.unwrap().unwrap();
}

/// Verifies that SOCKS5 no-auth CONNECT relays TCP payloads.
#[tokio::test]
async fn socks5_connect_relays_tcp_payloads_without_auth() {
    let target = spawn_echo_target().await;
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    let core = ArachneaProxyCore::new(ProxyConfig::default()).unwrap();
    let config = Arc::new(ServerConfig::default());
    let handler = tokio::spawn(async move {
        let (stream, peer) = proxy_listener.accept().await.unwrap();
        handlers::socks::handle(stream, peer, config, core).await
    });

    let mut client = TcpStream::connect(proxy_addr).await.unwrap();
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut method = [0u8; 2];
    client.read_exact(&mut method).await.unwrap();
    assert_eq!(method, [0x05, 0x00]);

    write_socks_connect_request(&mut client, target).await;
    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], 0x00);

    client.write_all(b"pong").await.unwrap();
    let mut echoed = [0u8; 4];
    client.read_exact(&mut echoed).await.unwrap();
    assert_eq!(&echoed, b"pong");
    drop(client);
    handler.await.unwrap().unwrap();

    let runtime = crate::server::LoopbackProxyRuntime::new(0);
    let addr = runtime.start().await.unwrap();
    let lease = runtime
        .register(
            ArachneaProxyCore::new(ProxyConfig::default()).unwrap(),
            crate::core::ClientContext::new(),
            true,
        )
        .unwrap();
    let mut anonymous = TcpStream::connect(addr).await.unwrap();
    anonymous.write_all(&[5, 1, 0]).await.unwrap();
    anonymous.read_exact(&mut method).await.unwrap();
    assert_eq!(method, [5, 255]);
    drop(anonymous);
    let target = spawn_echo_target().await;
    for (password, command) in [
        ("expired", 1),
        (lease.credential(), 3),
        (lease.credential(), 1),
    ] {
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(&[5, 1, 2]).await.unwrap();
        client.read_exact(&mut method).await.unwrap();
        assert_eq!(method, [5, 2]);
        let username = b"arachnea-context";
        let mut auth = vec![1, username.len() as u8];
        auth.extend_from_slice(username);
        auth.push(password.len() as u8);
        auth.extend_from_slice(password.as_bytes());
        client.write_all(&auth).await.unwrap();
        client.read_exact(&mut method).await.unwrap();
        assert_eq!(method, [1, if password == "expired" { 1 } else { 0 }]);
        if password == "expired" {
            continue;
        }
        if command == 3 {
            client
                .write_all(&[5, 3, 0, 1, 0, 0, 0, 0, 0, 0])
                .await
                .unwrap();
            client.read_exact(&mut reply).await.unwrap();
            assert_eq!(reply[1], 7);
            continue;
        }
        write_socks_connect_request(&mut client, target).await;
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply[1], 0);
        client.write_all(b"ping").await.unwrap();
        client.read_exact(&mut echoed).await.unwrap();
        assert_eq!(&echoed, b"ping");
    }
    assert_eq!(runtime.local_addr(), Some(addr));
    runtime.shutdown();
}

/// Verifies that SOCKS4 CONNECT relays TCP payloads.
#[tokio::test]
async fn socks4_connect_relays_tcp_payloads_without_auth() {
    let target = spawn_echo_target().await;
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    let core = ArachneaProxyCore::new(ProxyConfig::default()).unwrap();
    let config = Arc::new(ServerConfig::default());
    let handler = tokio::spawn(async move {
        let (stream, peer) = proxy_listener.accept().await.unwrap();
        handlers::socks::handle(stream, peer, config, core).await
    });

    let mut client = TcpStream::connect(proxy_addr).await.unwrap();
    write_socks4_connect_request(&mut client, target, "").await;
    let mut reply = [0u8; 8];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], 0x5a);

    client.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    client.read_exact(&mut echoed).await.unwrap();
    assert_eq!(&echoed, b"ping");
    drop(client);
    handler.await.unwrap().unwrap();
}

/// Verifies that SOCKS5 UDP ASSOCIATE relays datagrams to a UDP target.
#[tokio::test]
async fn socks5_udp_associate_relays_datagrams() {
    let target = spawn_udp_echo_target().await;
    let proxy_addr =
        spawn_socks_proxy(ArachneaProxyCore::new(ProxyConfig::default()).unwrap()).await;

    let mut control = TcpStream::connect(proxy_addr).await.unwrap();
    negotiate_socks_no_auth(&mut control).await;
    control
        .write_all(&[0x05, 0x03, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await
        .unwrap();
    let relay_addr = read_socks_reply_addr(&mut control).await;

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let packet = encode_socks_udp_packet(target, b"ping");
    client.send_to(&packet, relay_addr).await.unwrap();

    let mut response = vec![0u8; 1024];
    let (len, _) = timeout(Duration::from_secs(5), client.recv_from(&mut response))
        .await
        .unwrap()
        .unwrap();
    let (source, payload) = decode_socks_udp_packet(&response[..len]);

    assert_eq!(source, target);
    assert_eq!(payload, b"ping");
}

/// Verifies that an HTTPS upstream proxy can establish a CONNECT tunnel.
#[tokio::test]
async fn https_upstream_proxy_connect_tunnel_works() {
    let target = spawn_echo_target().await;
    let proxy_addr = spawn_https_connect_proxy(target).await;
    let node = ProxyNode::https("https-proxy", proxy_addr.to_string()).with_tls_verification(false);
    let core = ArachneaProxyCore::new(ProxyConfig {
        chains: vec![ProxyChain::single("https-proxy", node)],
        routing: RoutePolicy {
            default_chain: Some("https-proxy".to_string()),
            allow_direct_fallback: false,
            rules: Vec::new(),
        },
        security: SecurityPolicy {
            verify_upstream_tls: false,
            allow_insecure_upstreams: true,
            ..SecurityPolicy::default()
        },
        ..ProxyConfig::default()
    })
    .unwrap();

    let mut stream = core
        .connect(
            Destination::from_authority(&target.to_string(), ApplicationProtocol::Tcp).unwrap(),
        )
        .await
        .unwrap();
    stream.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    stream.read_exact(&mut echoed).await.unwrap();

    assert_eq!(&echoed, b"ping");
}

/// Verifies that the core can chain two upstream SOCKS5 proxy hops.
#[tokio::test]
async fn upstream_socks_chain_reaches_target() {
    let target = spawn_echo_target().await;
    let second_proxy =
        spawn_socks_proxy(ArachneaProxyCore::new(ProxyConfig::default()).unwrap()).await;
    let first_proxy =
        spawn_socks_proxy(ArachneaProxyCore::new(ProxyConfig::default()).unwrap()).await;

    let mut core =
        ArachneaProxyCore::new(ProxyNode::socks5("first-hop", first_proxy.to_string()).config())
            .unwrap();
    core.append_proxy(ProxyNode::socks5("second-hop", second_proxy.to_string()))
        .unwrap();

    let mut stream = core
        .connect(
            Destination::from_authority(&target.to_string(), ApplicationProtocol::Tcp).unwrap(),
        )
        .await
        .unwrap();
    stream.write_all(b"pong").await.unwrap();
    let mut echoed = [0u8; 4];
    stream.read_exact(&mut echoed).await.unwrap();

    assert_eq!(&echoed, b"pong");
    assert_eq!(
        stream.metadata().hops,
        vec!["first-hop".to_string(), "second-hop".to_string()]
    );
}

/// Verifies that the core can use a SOCKS4a upstream for hostname targets.
#[tokio::test]
async fn upstream_socks4a_reaches_target() {
    let target = spawn_echo_target().await;
    let proxy_addr = spawn_socks4_proxy().await;
    let node = ProxyNode::socks4("legacy-socks", proxy_addr.to_string())
        .with_dns_resolution(ProxyNameResolutionMode::ProxyOnly);
    let core = ArachneaProxyCore::new(node.config()).unwrap();

    let mut stream = core
        .connect(Destination::host_port("127.0.0.1", target.port()))
        .await
        .unwrap();
    stream.write_all(b"pong").await.unwrap();
    let mut echoed = [0u8; 4];
    stream.read_exact(&mut echoed).await.unwrap();

    assert_eq!(&echoed, b"pong");
}

/// Verifies that a SOCKS5 upstream can fall back to local DNS after remote DNS fails.
#[tokio::test]
async fn upstream_socks_falls_back_to_local_dns_after_remote_resolution_failure() {
    let target = spawn_echo_target().await;
    let proxy_addr = spawn_socks_proxy_rejecting_domain_requests().await;
    let node = ProxyNode::socks5("flaky-dns", proxy_addr.to_string())
        .with_dns_resolution(ProxyNameResolutionMode::ProxyThenLocalFallback);
    let core = ArachneaProxyCore::new(node.config()).unwrap();

    let mut stream = core
        .connect(Destination::host_port("127.0.0.1", target.port()))
        .await
        .unwrap();
    stream.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    stream.read_exact(&mut echoed).await.unwrap();

    assert_eq!(&echoed, b"ping");
}

#[path = "../support/server.rs"]
mod support;
use support::*;
