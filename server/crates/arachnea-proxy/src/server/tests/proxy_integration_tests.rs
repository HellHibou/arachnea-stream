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
}

/// Verifies that HTTP proxy authentication is disabled when no password is configured.
#[tokio::test]
async fn http_proxy_does_not_challenge_without_password_env() {
    let target = spawn_http_target().await;
    let core = ArachneaProxyCore::new(ProxyConfig::default()).unwrap();
    let mut server_config = ServerConfig::default();
    server_config.authentication = ProxyAuthConfig {
        username: Some("tester".to_string()),
        password_env: None,
        realm: "Arachnea Test".to_string(),
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

/// Starts a tiny HTTP target server and returns its address.
///
/// # Returns
///
/// Socket address of the target server.
async fn spawn_http_target() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            stream.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
            if request.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        assert!(String::from_utf8(request)
            .unwrap()
            .starts_with("GET /hello HTTP/1.1"));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nproxied")
            .await
            .unwrap();
    });
    addr
}

/// Starts a TCP echo target and returns its address.
///
/// # Returns
///
/// Socket address of the target server.
async fn spawn_echo_target() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        loop {
            let read = stream.read(&mut buf).await.unwrap();
            if read == 0 {
                break;
            }
            stream.write_all(&buf[..read]).await.unwrap();
        }
    });
    addr
}

/// Starts a UDP echo target and returns its address.
///
/// # Returns
///
/// Socket address of the UDP echo target.
async fn spawn_udp_echo_target() -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        loop {
            let (len, peer) = socket.recv_from(&mut buf).await.unwrap();
            socket.send_to(&buf[..len], peer).await.unwrap();
        }
    });
    addr
}

/// Starts a SOCKS5 proxy handler and returns its listening address.
///
/// # Parameters
///
/// - `core`: Proxy core used by the SOCKS5 handler.
///
/// # Returns
///
/// Socket address of the SOCKS5 proxy.
async fn spawn_socks_proxy(core: ArachneaProxyCore) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let config = Arc::new(ServerConfig::default());
    tokio::spawn(async move {
        loop {
            let (stream, peer) = listener.accept().await.unwrap();
            let config = Arc::clone(&config);
            let core = core.clone();
            tokio::spawn(async move {
                let _ = handlers::socks::handle(stream, peer, config, core).await;
            });
        }
    });
    addr
}

/// Starts a tiny SOCKS4 proxy handler and returns its listening address.
///
/// # Returns
///
/// Socket address of the SOCKS4 proxy.
async fn spawn_socks4_proxy() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        handle_socks4_proxy(stream).await.unwrap();
    });
    addr
}

/// Handles one SOCKS4 or SOCKS4a CONNECT request for tests.
///
/// # Parameters
///
/// - `client`: Client stream connected to the fake SOCKS4 proxy.
///
/// # Errors
///
/// Returns an error when socket I/O fails.
async fn handle_socks4_proxy(mut client: TcpStream) -> std::io::Result<()> {
    let mut head = [0u8; 8];
    client.read_exact(&mut head).await?;
    assert_eq!(head[0], 0x04);
    assert_eq!(head[1], 0x01);
    let port = u16::from_be_bytes([head[2], head[3]]);
    let ip = Ipv4Addr::new(head[4], head[5], head[6], head[7]);
    let _user_id = read_socks4_nul_string(&mut client).await?;
    let mut upstream = if head[4] == 0 && head[5] == 0 && head[6] == 0 && head[7] != 0 {
        let host = read_socks4_nul_string(&mut client).await?;
        TcpStream::connect((host.as_str(), port)).await?
    } else {
        TcpStream::connect(SocketAddr::from((ip, port))).await?
    };

    client
        .write_all(&[0x00, 0x5a, 0x00, 0x00, 0, 0, 0, 0])
        .await?;
    tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
    Ok(())
}

/// Reads one NUL-terminated SOCKS4 string from a test stream.
///
/// # Parameters
///
/// - `client`: Stream positioned at the start of the string.
///
/// # Returns
///
/// UTF-8 decoded string without the terminator.
///
/// # Errors
///
/// Returns an error when socket I/O fails or the string is not UTF-8.
async fn read_socks4_nul_string(client: &mut TcpStream) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        client.read_exact(&mut byte).await?;
        if byte[0] == 0 {
            break;
        }
        bytes.push(byte[0]);
    }
    String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))
}

/// Starts a SOCKS5 proxy that rejects domain targets but accepts IP targets.
///
/// # Returns
///
/// Socket address of the SOCKS5 proxy.
async fn spawn_socks_proxy_rejecting_domain_requests() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for _ in 0..2 {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                handle_socks_proxy_rejecting_domain_request(stream)
                    .await
                    .unwrap();
            });
        }
    });
    addr
}

/// Handles one request for a SOCKS5 proxy that rejects domain targets.
///
/// # Parameters
///
/// - `client`: Client stream connected to the fake SOCKS5 proxy.
///
/// # Errors
///
/// Returns an error when socket I/O fails.
async fn handle_socks_proxy_rejecting_domain_request(mut client: TcpStream) -> std::io::Result<()> {
    let mut greeting = [0u8; 3];
    client.read_exact(&mut greeting).await?;
    client.write_all(&[0x05, 0x00]).await?;

    let mut head = [0u8; 4];
    client.read_exact(&mut head).await?;
    if head[3] == 0x03 {
        let mut len = [0u8; 1];
        client.read_exact(&mut len).await?;
        let mut _host = vec![0u8; len[0] as usize];
        client.read_exact(&mut _host).await?;
        let mut _port = [0u8; 2];
        client.read_exact(&mut _port).await?;
        client
            .write_all(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await?;
        return Ok(());
    }

    let target = read_socks_ip_target(&mut client, head[3]).await?;
    let mut upstream = TcpStream::connect(target).await?;
    client
        .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0, 0])
        .await?;
    tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
    Ok(())
}

/// Reads an IP target from a SOCKS5 CONNECT request.
///
/// # Parameters
///
/// - `client`: Client stream positioned after the SOCKS5 request header.
/// - `address_type`: SOCKS5 address type byte from the request header.
///
/// # Returns
///
/// Socket address requested by the client.
///
/// # Errors
///
/// Returns an error when the request cannot be read or uses a domain target.
async fn read_socks_ip_target(
    client: &mut TcpStream,
    address_type: u8,
) -> std::io::Result<SocketAddr> {
    let ip = match address_type {
        0x01 => {
            let mut octets = [0u8; 4];
            client.read_exact(&mut octets).await?;
            std::net::IpAddr::from(octets)
        }
        0x04 => {
            let mut octets = [0u8; 16];
            client.read_exact(&mut octets).await?;
            std::net::IpAddr::from(octets)
        }
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "domain target was not expected after fallback",
            ));
        }
    };
    let mut port = [0u8; 2];
    client.read_exact(&mut port).await?;
    Ok(SocketAddr::new(ip, u16::from_be_bytes(port)))
}

/// Starts a TLS-wrapped HTTP CONNECT proxy that forwards to a fixed target.
///
/// # Parameters
///
/// - `target`: TCP target that receives tunneled bytes.
///
/// # Returns
///
/// Socket address of the HTTPS proxy.
async fn spawn_https_connect_proxy(target: SocketAddr) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let acceptor = test_tls_acceptor();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut client = acceptor.accept(tcp).await.unwrap();
        let request = read_http_header_block(&mut client).await;
        assert!(request.starts_with(&format!("CONNECT {target} HTTP/1.1\r\n")));
        let mut upstream = TcpStream::connect(target).await.unwrap();
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .unwrap();
        tokio::io::copy_bidirectional(&mut client, &mut upstream)
            .await
            .unwrap();
    });
    addr
}

/// Completes a SOCKS5 no-auth negotiation.
///
/// # Parameters
///
/// - `client`: TCP stream connected to a SOCKS5 proxy.
async fn negotiate_socks_no_auth(client: &mut TcpStream) {
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut method = [0u8; 2];
    client.read_exact(&mut method).await.unwrap();
    assert_eq!(method, [0x05, 0x00]);
}

/// Reads a SOCKS5 reply address from a TCP stream.
///
/// # Parameters
///
/// - `client`: TCP stream positioned at the start of a SOCKS5 reply.
///
/// # Returns
///
/// Bound address returned by the SOCKS5 proxy.
async fn read_socks_reply_addr(client: &mut TcpStream) -> SocketAddr {
    let mut head = [0u8; 4];
    client.read_exact(&mut head).await.unwrap();
    assert_eq!(head[0], 0x05);
    assert_eq!(head[1], 0x00);
    let ip = match head[3] {
        0x01 => {
            let mut octets = [0u8; 4];
            client.read_exact(&mut octets).await.unwrap();
            std::net::IpAddr::from(octets)
        }
        0x04 => {
            let mut octets = [0u8; 16];
            client.read_exact(&mut octets).await.unwrap();
            std::net::IpAddr::from(octets)
        }
        other => panic!("unexpected SOCKS5 reply address type {other}"),
    };
    let mut port = [0u8; 2];
    client.read_exact(&mut port).await.unwrap();
    SocketAddr::new(ip, u16::from_be_bytes(port))
}

/// Encodes one SOCKS5 UDP datagram.
///
/// # Parameters
///
/// - `destination`: UDP destination encoded in the datagram.
/// - `payload`: UDP payload.
///
/// # Returns
///
/// Encoded SOCKS5 UDP packet.
fn encode_socks_udp_packet(destination: SocketAddr, payload: &[u8]) -> Vec<u8> {
    let SocketAddr::V4(destination) = destination else {
        panic!("test destination must be IPv4");
    };
    let mut packet = vec![0x00, 0x00, 0x00, 0x01];
    packet.extend_from_slice(&destination.ip().octets());
    packet.extend_from_slice(&destination.port().to_be_bytes());
    packet.extend_from_slice(payload);
    packet
}

/// Decodes one SOCKS5 UDP response datagram.
///
/// # Parameters
///
/// - `packet`: Raw SOCKS5 UDP packet.
///
/// # Returns
///
/// Source address and payload bytes.
fn decode_socks_udp_packet(packet: &[u8]) -> (SocketAddr, &[u8]) {
    assert!(packet.len() >= 10);
    assert_eq!(&packet[..4], &[0x00, 0x00, 0x00, 0x01]);
    let ip = Ipv4Addr::new(packet[4], packet[5], packet[6], packet[7]);
    let port = u16::from_be_bytes([packet[8], packet[9]]);
    (SocketAddr::from((ip, port)), &packet[10..])
}

/// Reads an HTTP header block into a string.
///
/// # Parameters
///
/// - `stream`: TLS stream carrying an HTTP request.
///
/// # Returns
///
/// Request header block decoded as UTF-8.
async fn read_http_header_block<S>(stream: &mut S) -> String
where
    S: tokio::io::AsyncRead + Unpin,
{
    let mut data = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte).await.unwrap();
        data.push(byte[0]);
        if data.ends_with(b"\r\n\r\n") {
            return String::from_utf8(data).unwrap();
        }
    }
}

/// Builds a TLS acceptor backed by embedded test certificates.
///
/// # Returns
///
/// TLS acceptor used by HTTPS proxy tests.
fn test_tls_acceptor() -> TlsAcceptor {
    let certs = pem_blocks(TEST_CHAIN_PEM, "CERTIFICATE")
        .into_iter()
        .map(CertificateDer::from)
        .collect::<Vec<_>>();
    let key = PrivateKeyDer::try_from(
        pem_blocks(TEST_KEY_PEM, "PRIVATE KEY")
            .into_iter()
            .next()
            .unwrap(),
    )
    .unwrap();
    let config = RustlsServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(certs, key)
    .unwrap();
    TlsAcceptor::from(Arc::new(config))
}

/// Extracts and decodes PEM blocks matching a label.
///
/// # Parameters
///
/// - `pem`: PEM text to scan.
/// - `label`: PEM block label without markers.
///
/// # Returns
///
/// Decoded DER blocks.
fn pem_blocks(pem: &str, label: &str) -> Vec<Vec<u8>> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let mut blocks = Vec::new();
    let mut remaining = pem;

    while let Some(start) = remaining.find(&begin) {
        let after_begin = &remaining[start + begin.len()..];
        let Some(end_offset) = after_begin.find(&end) else {
            break;
        };
        let body = after_begin[..end_offset]
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<String>();
        blocks.push(
            base64::engine::general_purpose::STANDARD
                .decode(body)
                .unwrap(),
        );
        remaining = &after_begin[end_offset + end.len()..];
    }

    blocks
}

/// Test certificate chain for HTTPS proxy integration tests.
const TEST_CHAIN_PEM: &str = "-----BEGIN CERTIFICATE-----\nMIIBszCCAVmgAwIBAgIUUg3keFcU1xXWK8BNVb1KynPulV8wCgYIKoZIzj0EAwIw\nJjEkMCIGA1UEAwwbUnVzdGxzIFJvYnVzdCBSb290IC0gUnVuZyAyMCAXDTc1MDEw\nMTAwMDAwMFoYDzQwOTYwMTAxMDAwMDAwWjAhMR8wHQYDVQQDDBZyY2dlbiBzZWxm\nIHNpZ25lZCBjZXJ0MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEud6w4gtZ0xbw\nJ3E69SSMy5TZfdIifl9L5ZY+hgEe4UiUsBWS32f6Y5NR5Jo8FO1f6o13b3+FvVHR\nEHCGdvppL6NoMGYwFQYDVR0RBA4wDIIKZm9vYmFyLmNvbTAdBgNVHSUEFjAUBggr\nBgEFBQcDAQYIKwYBBQUHAwIwHQYDVR0OBBYEFELvxbj5tD75n4pYFvJyr+c8qVEi\nMA8GA1UdEwEB/wQFMAMBAQAwCgYIKoZIzj0EAwIDSAAwRQIhALxSSdUsrRFnwNMu\n/doBqI8i8u5HdohVAheFTDwObkOMAiASSjULUtkWSD15u/7Sr01Wm9J1MpqW1pob\nBVqU3CNRlA==\n-----END CERTIFICATE-----\n-----BEGIN CERTIFICATE-----\nMIIBiTCCATCgAwIBAgIUHWiVYIvMMWoZEFYvSz46COf2FqowCgYIKoZIzj0EAwIw\nHTEbMBkGA1UEAwwSUnVzdGxzIFJvYnVzdCBSb290MCAXDTc1MDEwMTAwMDAwMFoY\nDzQwOTYwMTAxMDAwMDAwWjAmMSQwIgYDVQQDDBtSdXN0bHMgUm9idXN0IFJvb3Qg\nLSBSdW5nIDIwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAATAOCcBD7dXjmAZ3te5\nD47cCJ9ec93PWv7BKYIL826CJsKfXQOGrBTthLm77hXLhHu6uv8E5QXNLZpfowLQ\nDo1ao0MwQTAPBgNVHQ8BAf8EBQMDB4QAMB0GA1UdDgQWBBRdza76r11Ok9vRmlg6\nNn/wL/N+jTAPBgNVHRMBAf8EBTADAQH/MAoGCCqGSM49BAMCA0cAMEQCIFmZrXeK\nhnfkahocvkhhNT3cDv1LWf6WBoFaCiBwZXFPAiARaKRiSCMG7PCHmSqFe82TBVmL\nodHGogAVax1Dh/aYAA==\n-----END CERTIFICATE-----\n";

/// Test private key matching the HTTPS proxy test certificate.
const TEST_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgTbAQpfjAT46fgF4B\nmP15n37woNG5ZNJmwcqsred/7tmhRANCAAS53rDiC1nTFvAncTr1JIzLlNl90iJ+\nX0vllj6GAR7hSJSwFZLfZ/pjk1HkmjwU7V/qjXdvf4W9UdEQcIZ2+mkv\n-----END PRIVATE KEY-----\n";

/// Writes a SOCKS5 CONNECT request for an IPv4 socket address.
///
/// # Parameters
///
/// - `client`: Client stream connected to the SOCKS5 proxy.
/// - `target`: IPv4 target address to request.
async fn write_socks_connect_request(client: &mut TcpStream, target: SocketAddr) {
    let SocketAddr::V4(target) = target else {
        panic!("test target must be IPv4");
    };
    let mut request = vec![0x05, 0x01, 0x00, 0x01];
    request.extend_from_slice(&target.ip().octets());
    request.extend_from_slice(&target.port().to_be_bytes());
    client.write_all(&request).await.unwrap();
}

/// Writes a SOCKS4 CONNECT request for an IPv4 socket address.
///
/// # Parameters
///
/// - `client`: Client stream connected to the SOCKS4 proxy.
/// - `target`: IPv4 target address to request.
/// - `user_id`: SOCKS4 user identifier.
async fn write_socks4_connect_request(client: &mut TcpStream, target: SocketAddr, user_id: &str) {
    let SocketAddr::V4(target) = target else {
        panic!("test target must be IPv4");
    };
    let mut request = vec![0x04, 0x01];
    request.extend_from_slice(&target.port().to_be_bytes());
    request.extend_from_slice(&target.ip().octets());
    request.extend_from_slice(user_id.as_bytes());
    request.push(0x00);
    client.write_all(&request).await.unwrap();
}
