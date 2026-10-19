use super::*;

/// Starts a TCP echo target that accepts a fixed number of connections.
///
/// # Parameters
///
/// - `connections`: Number of client connections to accept.
///
/// # Returns
///
/// Socket address of the echo target.
pub(super) async fn spawn_echo_target_accepting(connections: usize) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for _ in 0..connections {
            let (mut stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                loop {
                    let read = stream.read(&mut buf).await.unwrap();
                    if read == 0 {
                        break;
                    }
                    stream.write_all(&buf[..read]).await.unwrap();
                }
            });
        }
    });
    addr
}

/// Starts a SOCKS5 proxy that rejects domain targets but accepts IP targets.
///
/// # Parameters
///
/// - `connections`: Number of SOCKS5 client connections to accept.
///
/// # Returns
///
/// Socket address of the SOCKS5 proxy.
pub(super) async fn spawn_socks5_proxy_rejecting_domain_requests(connections: usize) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for _ in 0..connections {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                handle_socks5_proxy_rejecting_domain_request(stream)
                    .await
                    .unwrap();
            });
        }
    });
    addr
}

/// Handles one SOCKS5 request by rejecting domains and tunneling IP targets.
///
/// # Parameters
///
/// - `client`: Client stream connected to the fake SOCKS5 proxy.
///
/// # Errors
///
/// Returns an error when socket I/O fails.
async fn handle_socks5_proxy_rejecting_domain_request(
    mut client: TcpStream,
) -> std::io::Result<()> {
    let mut greeting_head = [0u8; 2];
    client.read_exact(&mut greeting_head).await?;
    let mut methods = vec![0u8; greeting_head[1] as usize];
    client.read_exact(&mut methods).await?;
    client.write_all(&[0x05, 0x00]).await?;

    let mut head = [0u8; 4];
    client.read_exact(&mut head).await?;
    if head[3] == 0x03 {
        let mut len = [0u8; 1];
        client.read_exact(&mut len).await?;
        let mut host = vec![0u8; len[0] as usize];
        client.read_exact(&mut host).await?;
        let mut port = [0u8; 2];
        client.read_exact(&mut port).await?;
        client
            .write_all(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await?;
        return Ok(());
    }

    let target = read_socks5_ip_target(&mut client, head[3]).await?;
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
/// - `client`: SOCKS5 stream positioned after the request header.
/// - `address_type`: SOCKS5 address type byte from the request header.
///
/// # Returns
///
/// Socket address requested by the SOCKS5 client.
///
/// # Errors
///
/// Returns an error when the target cannot be read or is not an IP address.
async fn read_socks5_ip_target(
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
