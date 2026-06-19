use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use crate::core::{
    context_from_parameter_pairs, ApplicationProtocol, ArachneaProxyCore, ClientContext,
    ConnectRequest, Destination, DestinationAddress, ParameterDefinition, ProxyError, Result,
    Socks5UdpAssociation,
};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{lookup_host, TcpStream, UdpSocket};

use crate::server::ServerConfig;

/// Handles one SOCKS TCP connection.
pub async fn handle(
    mut client: TcpStream,
    _peer: SocketAddr,
    config: Arc<ServerConfig>,
    core: ArachneaProxyCore,
) -> Result<()> {
    let credentials = config
        .authentication
        .credentials()
        .map_err(ProxyError::Config)?;
    let parameter_definitions = core.parameter_definitions();
    let mut version = [0u8; 1];
    client.read_exact(&mut version).await?;
    tracing::debug!(
        socks_version = if version[0] == 0x04 { "4" } else if version[0] == 0x05 { "5" } else { "unknown" },
        "socks client connected"
    );
    match version[0] {
        0x04 => {
            let request = read_socks4_request(&mut client).await?;
            if credentials.is_some() {
                write_socks4_reply(&mut client, 0x5b).await?;
                return Err(ProxyError::AccessDenied(
                    "socks4 cannot satisfy configured username/password authentication".to_string(),
                ));
            }
            match request.command {
                0x01 => handle_socks4_connect(client, request.destination, core).await,
                0x02 => {
                    write_socks4_reply(&mut client, 0x5b).await?;
                    Err(ProxyError::Unsupported("socks4 bind"))
                }
                other => {
                    write_socks4_reply(&mut client, 0x5b).await?;
                    Err(ProxyError::Protocol(format!(
                        "unsupported socks4 command {other}"
                    )))
                }
            }
        }
        0x05 => {
            let context =
                negotiate_auth(&mut client, credentials.as_ref(), &parameter_definitions).await?;
            let request = read_request(&mut client).await?;
            match request.command {
                0x01 => handle_connect(client, request.destination, context, core).await,
                0x03 => handle_udp_associate(client, request.destination, context, core).await,
                0x02 => {
                    write_reply(&mut client, 0x07).await?;
                    Err(ProxyError::Unsupported("socks5 bind"))
                }
                other => {
                    write_reply(&mut client, 0x07).await?;
                    Err(ProxyError::Protocol(format!(
                        "unsupported socks5 command {other}"
                    )))
                }
            }
        }
        _ => Err(ProxyError::Protocol(
            "socks client used unsupported version".to_string(),
        )),
    }
}

/// Negotiates the configured SOCKS5 authentication mode with a client.
///
/// # Parameters
///
/// - `client`: TCP stream positioned after the SOCKS5 version byte.
/// - `credentials`: Optional expected username and password.
/// - `definitions`: Registered parameter definitions.
///
/// # Returns
///
/// Client context extracted from the username/password exchange when no
/// SOCKS5 authentication is configured.
///
/// # Errors
///
/// Returns an error when the client uses an unsupported version, omits the
/// required authentication method, supplies invalid credentials, or the stream
/// fails.
async fn negotiate_auth(
    client: &mut TcpStream,
    credentials: Option<&crate::server::ProxyCredentials>,
    definitions: &[ParameterDefinition],
) -> Result<ClientContext> {
    let mut method_count = [0u8; 1];
    client.read_exact(&mut method_count).await?;
    let mut methods = vec![0u8; method_count[0] as usize];
    client.read_exact(&mut methods).await?;

    if let Some(credentials) = credentials {
        if methods.contains(&0x02) {
            client.write_all(&[0x05, 0x02]).await?;
            authenticate_username_password(client, credentials).await?;
            Ok(ClientContext::new())
        } else {
            client.write_all(&[0x05, 0xff]).await?;
            Err(ProxyError::AccessDenied(
                "socks5 username/password method was not offered".to_string(),
            ))
        }
    } else if methods.contains(&0x02) {
        client.write_all(&[0x05, 0x02]).await?;
        read_parameter_username_password(client, definitions).await
    } else if methods.contains(&0x00) {
        client.write_all(&[0x05, 0x00]).await?;
        Ok(ClientContext::new())
    } else {
        client.write_all(&[0x05, 0xff]).await?;
        Err(ProxyError::AccessDenied(
            "socks5 no-auth method was not offered".to_string(),
        ))
    }
}

/// Reads SOCKS5 username/password data as optional request parameters.
///
/// # Parameters
///
/// - `client`: TCP stream positioned after method selection.
/// - `definitions`: Registered parameter definitions used as an allow-list.
///
/// # Returns
///
/// Client context extracted from the username string.
///
/// # Errors
///
/// Returns an error when the sub-negotiation is malformed or the stream fails.
async fn read_parameter_username_password(
    client: &mut TcpStream,
    definitions: &[ParameterDefinition],
) -> Result<ClientContext> {
    let mut version = [0u8; 1];
    client.read_exact(&mut version).await?;
    if version[0] != 0x01 {
        let _ = client.write_all(&[0x01, 0x01]).await;
        return Err(ProxyError::Protocol(
            "socks5 auth subnegotiation used unsupported version".to_string(),
        ));
    }
    let username = read_socks_auth_string(client).await?;
    let _password = read_socks_auth_string(client).await?;
    client.write_all(&[0x01, 0x00]).await?;
    Ok(context_from_socks_username(&username, definitions))
}

/// Builds a client context from a SOCKS5 username parameter string.
///
/// The accepted format is `name=value;name2=value2`. Commas are also accepted
/// as separators for clients that make semicolons awkward to configure.
///
/// # Parameters
///
/// - `username`: SOCKS5 username string.
/// - `definitions`: Registered parameter definitions used as an allow-list.
///
/// # Returns
///
/// Client context containing recognized parameters.
fn context_from_socks_username(
    username: &str,
    definitions: &[ParameterDefinition],
) -> ClientContext {
    let pairs = username
        .split([';', ','])
        .filter_map(|part| part.split_once('='))
        .map(|(name, value)| (name.trim(), value.trim()));
    context_from_parameter_pairs(pairs, definitions)
}

/// Authenticates a SOCKS5 username/password sub-negotiation.
///
/// # Parameters
///
/// - `client`: TCP stream positioned after method selection.
/// - `credentials`: Expected proxy credentials.
///
/// # Errors
///
/// Returns an error when the client sends malformed credentials, the supplied
/// credentials do not match, or the stream fails.
async fn authenticate_username_password(
    client: &mut TcpStream,
    credentials: &crate::server::ProxyCredentials,
) -> Result<()> {
    let mut version = [0u8; 1];
    client.read_exact(&mut version).await?;
    if version[0] != 0x01 {
        let _ = client.write_all(&[0x01, 0x01]).await;
        return Err(ProxyError::Protocol(
            "socks5 auth subnegotiation used unsupported version".to_string(),
        ));
    }

    let username = read_socks_auth_string(client).await?;
    let password = read_socks_auth_string(client).await?;
    if credentials.matches(&username, &password) {
        client.write_all(&[0x01, 0x00]).await?;
        Ok(())
    } else {
        client.write_all(&[0x01, 0x01]).await?;
        Err(ProxyError::AccessDenied(
            "socks5 username/password authentication failed".to_string(),
        ))
    }
}

/// Reads one length-prefixed SOCKS5 authentication string.
///
/// # Parameters
///
/// - `client`: TCP stream positioned at the length byte.
///
/// # Returns
///
/// UTF-8 decoded authentication string.
///
/// # Errors
///
/// Returns an error when the stream fails or the string is not UTF-8.
async fn read_socks_auth_string(client: &mut TcpStream) -> Result<String> {
    let mut len = [0u8; 1];
    client.read_exact(&mut len).await?;
    let mut bytes = vec![0u8; len[0] as usize];
    client.read_exact(&mut bytes).await?;
    String::from_utf8(bytes)
        .map_err(|_| ProxyError::Protocol("socks5 auth string is not utf-8".to_string()))
}

/// Handles a SOCKS5 `CONNECT` request by opening a proxied TCP stream.
///
/// # Parameters
///
/// - `client`: TCP stream connected to the SOCKS5 client.
/// - `destination`: Target requested by the SOCKS5 client.
/// - `context`: Client context extracted during SOCKS5 authentication.
/// - `core`: Proxy core used to establish the outbound stream.
///
/// # Errors
///
/// Returns an error when the route cannot connect, the reply cannot be sent, or
/// bidirectional relay fails.
async fn handle_connect(
    mut client: TcpStream,
    destination: Destination,
    context: ClientContext,
    core: ArachneaProxyCore,
) -> Result<()> {
    tracing::debug!(
        target = %destination.authority(),
        "socks5 connect request received"
    );
    let request = ConnectRequest::new(destination).with_client_context(context);
    let mut upstream = match core.connect_request(request).await {
        Ok(stream) => stream,
        Err(error) => {
            let _ = write_reply(&mut client, 0x05).await;
            return Err(error);
        }
    };
    write_reply(&mut client, 0x00).await?;
    io::copy_bidirectional(&mut client, &mut upstream).await?;
    Ok(())
}

/// Handles a SOCKS4 `CONNECT` request by opening a proxied TCP stream.
///
/// # Parameters
///
/// - `client`: TCP stream connected to the SOCKS4 client.
/// - `destination`: Target requested by the SOCKS4 client.
/// - `core`: Proxy core used to establish the outbound stream.
///
/// # Errors
///
/// Returns an error when the route cannot connect, the reply cannot be sent, or
/// bidirectional relay fails.
async fn handle_socks4_connect(
    mut client: TcpStream,
    destination: Destination,
    core: ArachneaProxyCore,
) -> Result<()> {
    tracing::debug!(
        target = %destination.authority(),
        "socks4 connect request received"
    );
    let mut upstream = match core.connect(destination).await {
        Ok(stream) => stream,
        Err(error) => {
            let _ = write_socks4_reply(&mut client, 0x5b).await;
            return Err(error);
        }
    };
    write_socks4_reply(&mut client, 0x5a).await?;
    io::copy_bidirectional(&mut client, &mut upstream).await?;
    Ok(())
}

/// Handles a SOCKS5 `UDP ASSOCIATE` request.
///
/// # Parameters
///
/// - `client`: TCP control stream connected to the SOCKS5 client.
/// - `destination`: Client-supplied association destination.
/// - `core`: Proxy core used to select a UDP-capable route.
///
/// # Errors
///
/// Returns an error when the relay socket cannot be created or UDP relay fails.
async fn handle_udp_associate(
    mut client: TcpStream,
    destination: Destination,
    context: ClientContext,
    core: ArachneaProxyCore,
) -> Result<()> {
    tracing::debug!(
        target = %destination.authority(),
        "socks5 udp associate request received"
    );
    let tcp_local = client.local_addr()?;
    let udp_bind = SocketAddr::new(tcp_local.ip(), 0);
    let socket = UdpSocket::bind(udp_bind).await?;
    let relay_addr = socket.local_addr()?;
    write_reply_addr(&mut client, 0x00, relay_addr).await?;

    let request = ConnectRequest::new(destination).with_client_context(context);
    match core.connect_socks5_udp_association_request(request).await {
        Ok(association) => {
            tracing::debug!("socks5 udp association established via upstream proxy");
            relay_udp_via_socks(client, socket, association).await
        }
        Err(ProxyError::Unsupported(_)) => {
            tracing::debug!("socks5 udp association falling back to direct relay");
            relay_udp_direct(client, socket).await
        }
        Err(error) => Err(error),
    }
}

/// Relays SOCKS5 UDP datagrams directly to their destinations.
///
/// # Parameters
///
/// - `client`: TCP control stream whose lifetime owns the UDP association.
/// - `socket`: UDP relay socket advertised to the SOCKS5 client.
///
/// # Errors
///
/// Returns an error when UDP parsing, DNS resolution, socket I/O, or the TCP
/// control stream fails.
async fn relay_udp_direct(mut client: TcpStream, socket: UdpSocket) -> Result<()> {
    let mut tcp_buf = [0u8; 1];
    let mut udp_buf = vec![0u8; 65_535];
    let mut client_addr = None;
    let mut remote_to_client = HashMap::<SocketAddr, SocketAddr>::new();

    loop {
        tokio::select! {
            read = client.read(&mut tcp_buf) => {
                let read = read?;
                if read == 0 {
                    return Ok(());
                }
            }
            packet = socket.recv_from(&mut udp_buf) => {
                let (len, source) = packet?;
                if Some(source) == client_addr || client_addr.is_none() {
                    client_addr = Some(source);
                    let datagram = parse_udp_request(&udp_buf[..len])?;
                    if datagram.frag != 0 {
                        continue;
                    }
                    let target = resolve_udp_destination(&datagram.destination).await?;
                    remote_to_client.insert(target, source);
                    socket.send_to(datagram.payload, target).await?;
                } else if let Some(client_udp_addr) =
                    remote_to_client.get(&source).copied().or(client_addr)
                {
                    let response = encode_udp_response(source, &udp_buf[..len]);
                    socket.send_to(&response, client_udp_addr).await?;
                }
            }
        }

        if let Some(client_udp_addr) = client_addr {
            remote_to_client.retain(|_, value| *value == client_udp_addr);
        }
    }
}

/// Relays SOCKS5 UDP datagrams through an upstream SOCKS5 association.
///
/// # Parameters
///
/// - `client`: TCP control stream whose lifetime owns the UDP association.
/// - `client_socket`: UDP relay socket advertised to the downstream client.
/// - `association`: Upstream SOCKS5 UDP association selected by the core.
///
/// # Errors
///
/// Returns an error when downstream parsing, upstream relay, or socket I/O
/// fails.
async fn relay_udp_via_socks(
    mut client: TcpStream,
    client_socket: UdpSocket,
    mut association: Socks5UdpAssociation,
) -> Result<()> {
    let mut tcp_buf = [0u8; 1];
    let mut client_buf = vec![0u8; 65_535];
    let mut upstream_buf = vec![0u8; 65_535];
    let mut client_addr = None;

    loop {
        tokio::select! {
            read = client.read(&mut tcp_buf) => {
                let read = read?;
                if read == 0 {
                    return Ok(());
                }
            }
            packet = client_socket.recv_from(&mut client_buf) => {
                let (len, source) = packet?;
                if Some(source) == client_addr || client_addr.is_none() {
                    client_addr = Some(source);
                    let datagram = parse_udp_request(&client_buf[..len])?;
                    if datagram.frag != 0 {
                        continue;
                    }
                    association
                        .send_datagram(&datagram.destination, datagram.payload)
                        .await?;
                }
            }
            packet = association.recv_datagram(&mut upstream_buf) => {
                let (destination, len) = packet?;
                if let Some(client_udp_addr) = client_addr {
                    let response = encode_udp_response_destination(&destination, &upstream_buf[..len])?;
                    client_socket.send_to(&response, client_udp_addr).await?;
                }
            }
        }
    }
}

/// Resolves a UDP destination into a socket address for direct relay.
///
/// # Parameters
///
/// - `destination`: Destination extracted from a SOCKS5 UDP datagram.
///
/// # Returns
///
/// A concrete socket address suitable for `UdpSocket::send_to`.
///
/// # Errors
///
/// Returns an error when hostname resolution fails or produces no address.
async fn resolve_udp_destination(destination: &Destination) -> Result<SocketAddr> {
    match &destination.address {
        DestinationAddress::Ip(ip) => Ok(SocketAddr::new(*ip, destination.port)),
        DestinationAddress::Host(_) => {
            let mut addrs = lookup_host(destination.authority()).await?;
            addrs.next().ok_or_else(|| {
                ProxyError::Dns("udp destination resolved to no address".to_string())
            })
        }
    }
}

/// Reads a SOCKS4 or SOCKS4a command request from the TCP control stream.
///
/// # Parameters
///
/// - `client`: TCP stream positioned after the SOCKS4 version byte.
///
/// # Returns
///
/// The parsed SOCKS4 command and destination.
///
/// # Errors
///
/// Returns an error when the request is malformed, contains invalid text, or
/// the stream fails.
async fn read_socks4_request(client: &mut TcpStream) -> Result<SocksRequest> {
    let mut head = [0u8; 7];
    client.read_exact(&mut head).await?;
    let command = head[0];
    let port = u16::from_be_bytes([head[1], head[2]]);
    let ip = Ipv4Addr::new(head[3], head[4], head[5], head[6]);
    let _user_id = read_nul_terminated_field(client, 1024, "socks4 user id").await?;
    let address = if head[3] == 0 && head[4] == 0 && head[5] == 0 && head[6] != 0 {
        DestinationAddress::Host(read_nul_terminated_field(client, 255, "socks4a hostname").await?)
    } else {
        DestinationAddress::Ip(IpAddr::V4(ip))
    };

    Ok(SocksRequest {
        command,
        destination: Destination {
            address,
            port,
            protocol: ApplicationProtocol::Tcp,
        },
    })
}

/// Reads a NUL-terminated SOCKS4 text field.
///
/// # Parameters
///
/// - `client`: TCP stream positioned at the start of the text field.
/// - `limit`: Maximum number of bytes accepted before the terminator.
/// - `label`: Field label used in protocol errors.
///
/// # Returns
///
/// UTF-8 decoded field content without the terminator.
///
/// # Errors
///
/// Returns an error when the stream fails, the field is too long, or the field
/// is not UTF-8.
async fn read_nul_terminated_field(
    client: &mut TcpStream,
    limit: usize,
    label: &str,
) -> Result<String> {
    let mut bytes = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        client.read_exact(&mut byte).await?;
        if byte[0] == 0 {
            break;
        }
        if bytes.len() >= limit {
            return Err(ProxyError::Protocol(format!(
                "{label} exceeded {limit} bytes"
            )));
        }
        bytes.push(byte[0]);
    }
    String::from_utf8(bytes).map_err(|_| ProxyError::Protocol(format!("{label} is not utf-8")))
}

/// Reads a SOCKS5 command request from the TCP control stream.
///
/// # Parameters
///
/// - `client`: TCP stream positioned after method negotiation.
///
/// # Returns
///
/// The parsed SOCKS5 command and destination.
///
/// # Errors
///
/// Returns an error when the request is malformed or the stream fails.
async fn read_request(client: &mut TcpStream) -> Result<SocksRequest> {
    let mut head = [0u8; 4];
    client.read_exact(&mut head).await?;
    if head[0] != 0x05 {
        return Err(ProxyError::Protocol(
            "socks request used unsupported version".to_string(),
        ));
    }
    let address = match head[3] {
        0x01 => {
            let mut ip = [0u8; 4];
            client.read_exact(&mut ip).await?;
            DestinationAddress::Ip(IpAddr::from(ip))
        }
        0x03 => {
            let mut len = [0u8; 1];
            client.read_exact(&mut len).await?;
            let mut host = vec![0u8; len[0] as usize];
            client.read_exact(&mut host).await?;
            DestinationAddress::Host(
                String::from_utf8(host)
                    .map_err(|_| ProxyError::Protocol("socks hostname is not utf-8".to_string()))?,
            )
        }
        0x04 => {
            let mut ip = [0u8; 16];
            client.read_exact(&mut ip).await?;
            DestinationAddress::Ip(IpAddr::from(ip))
        }
        other => {
            return Err(ProxyError::Protocol(format!(
                "unsupported socks address type {other}"
            )));
        }
    };
    let mut port = [0u8; 2];
    client.read_exact(&mut port).await?;
    Ok(SocksRequest {
        command: head[1],
        destination: Destination {
            address,
            port: u16::from_be_bytes(port),
            protocol: ApplicationProtocol::Tcp,
        },
    })
}

/// Writes a SOCKS5 reply with an unspecified bind address.
///
/// # Parameters
///
/// - `client`: TCP control stream that receives the reply.
/// - `code`: SOCKS5 reply status code.
///
/// # Errors
///
/// Returns an error when the reply cannot be written.
async fn write_reply(client: &mut TcpStream, code: u8) -> Result<()> {
    write_reply_addr(client, code, SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).await
}

/// Writes a SOCKS5 reply with a concrete bind address.
///
/// # Parameters
///
/// - `client`: TCP control stream that receives the reply.
/// - `code`: SOCKS5 reply status code.
/// - `addr`: Bind address to encode in the reply.
///
/// # Errors
///
/// Returns an error when the reply cannot be written.
async fn write_reply_addr(client: &mut TcpStream, code: u8, addr: SocketAddr) -> Result<()> {
    let mut reply = vec![0x05, code, 0x00, 0x01];
    match addr.ip() {
        IpAddr::V4(ip) => {
            reply[3] = 0x01;
            reply.extend_from_slice(&ip.octets());
        }
        IpAddr::V6(ip) => {
            reply[3] = 0x04;
            reply.extend_from_slice(&ip.octets());
        }
    }
    reply.extend_from_slice(&addr.port().to_be_bytes());
    client.write_all(&reply).await?;
    Ok(())
}

/// Writes a SOCKS4 reply with an unspecified bind address.
///
/// # Parameters
///
/// - `client`: TCP control stream that receives the reply.
/// - `code`: SOCKS4 reply status code.
///
/// # Errors
///
/// Returns an error when the reply cannot be written.
async fn write_socks4_reply(client: &mut TcpStream, code: u8) -> Result<()> {
    client
        .write_all(&[0x00, code, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
        .await?;
    Ok(())
}

/// Parses a downstream SOCKS5 UDP datagram.
///
/// # Parameters
///
/// - `packet`: Raw UDP payload received from the SOCKS5 client.
///
/// # Returns
///
/// The datagram header fields and application payload.
///
/// # Errors
///
/// Returns an error when the datagram is malformed or uses an unsupported
/// address type.
fn parse_udp_request(packet: &[u8]) -> Result<UdpDatagram<'_>> {
    if packet.len() < 4 || packet[0] != 0 || packet[1] != 0 {
        return Err(ProxyError::Protocol(
            "invalid socks5 udp datagram header".to_string(),
        ));
    }
    let frag = packet[2];
    let atyp = packet[3];
    let mut offset = 4;
    let address = match atyp {
        0x01 => {
            if packet.len() < offset + 4 + 2 {
                return Err(ProxyError::Protocol(
                    "short socks5 udp ipv4 datagram".to_string(),
                ));
            }
            let ip = IpAddr::from([
                packet[offset],
                packet[offset + 1],
                packet[offset + 2],
                packet[offset + 3],
            ]);
            offset += 4;
            DestinationAddress::Ip(ip)
        }
        0x03 => {
            if packet.len() < offset + 1 {
                return Err(ProxyError::Protocol(
                    "short socks5 udp domain datagram".to_string(),
                ));
            }
            let len = packet[offset] as usize;
            offset += 1;
            if packet.len() < offset + len + 2 {
                return Err(ProxyError::Protocol(
                    "short socks5 udp domain payload".to_string(),
                ));
            }
            let host = String::from_utf8(packet[offset..offset + len].to_vec())
                .map_err(|_| ProxyError::Protocol("socks5 udp domain is not utf-8".to_string()))?;
            offset += len;
            DestinationAddress::Host(host)
        }
        0x04 => {
            if packet.len() < offset + 16 + 2 {
                return Err(ProxyError::Protocol(
                    "short socks5 udp ipv6 datagram".to_string(),
                ));
            }
            let mut ip = [0u8; 16];
            ip.copy_from_slice(&packet[offset..offset + 16]);
            offset += 16;
            DestinationAddress::Ip(IpAddr::from(ip))
        }
        other => {
            return Err(ProxyError::Protocol(format!(
                "unsupported socks5 udp address type {other}"
            )));
        }
    };
    let port = u16::from_be_bytes([packet[offset], packet[offset + 1]]);
    offset += 2;
    Ok(UdpDatagram {
        frag,
        destination: Destination {
            address,
            port,
            protocol: ApplicationProtocol::Udp,
        },
        payload: &packet[offset..],
    })
}

/// Encodes a direct UDP response for a SOCKS5 client.
///
/// # Parameters
///
/// - `source`: Remote source address that produced `payload`.
/// - `payload`: UDP payload to return to the client.
///
/// # Returns
///
/// A SOCKS5 UDP response datagram.
fn encode_udp_response(source: SocketAddr, payload: &[u8]) -> Vec<u8> {
    encode_udp_response_destination(
        &Destination {
            address: DestinationAddress::Ip(source.ip()),
            port: source.port(),
            protocol: ApplicationProtocol::Udp,
        },
        payload,
    )
    .expect("socket addresses are encodable as socks5 udp responses")
}

/// Encodes a UDP response from a destination that may be a hostname.
///
/// # Parameters
///
/// - `destination`: Source destination to encode in the SOCKS5 response header.
/// - `payload`: UDP payload to return to the client.
///
/// # Returns
///
/// A SOCKS5 UDP response datagram.
///
/// # Errors
///
/// Returns an error when the hostname cannot be represented by SOCKS5.
fn encode_udp_response_destination(destination: &Destination, payload: &[u8]) -> Result<Vec<u8>> {
    let mut packet = vec![0x00, 0x00, 0x00];
    match &destination.address {
        DestinationAddress::Ip(IpAddr::V4(ip)) => {
            packet.push(0x01);
            packet.extend_from_slice(&ip.octets());
        }
        DestinationAddress::Ip(IpAddr::V6(ip)) => {
            packet.push(0x04);
            packet.extend_from_slice(&ip.octets());
        }
        DestinationAddress::Host(host) => {
            let len = u8::try_from(host.len()).map_err(|_| {
                ProxyError::InvalidDestination(
                    "socks5 udp hostnames are limited to 255 bytes".to_string(),
                )
            })?;
            packet.push(0x03);
            packet.push(len);
            packet.extend_from_slice(host.as_bytes());
        }
    }
    packet.extend_from_slice(&destination.port.to_be_bytes());
    packet.extend_from_slice(payload);
    Ok(packet)
}

/// Parsed SOCKS TCP command request.
struct SocksRequest {
    command: u8,
    destination: Destination,
}

/// Parsed SOCKS5 UDP datagram borrowed from a packet buffer.
struct UdpDatagram<'a> {
    frag: u8,
    destination: Destination,
    payload: &'a [u8],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::PROXY_HEADER_PARAMETER_COUNTRY;
    use crate::core::PROXY_PARAMETER_COUNTRY;

    /// Verifies parsing of a SOCKS5 UDP datagram with an IPv4 destination.
    #[test]
    fn parses_socks5_udp_ipv4_datagram() {
        let packet = [
            0x00, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x1f, 0x90, b'p', b'i', b'n', b'g',
        ];
        let datagram = parse_udp_request(&packet).unwrap();
        assert_eq!(datagram.frag, 0);
        assert_eq!(datagram.destination.port, 8080);
        assert_eq!(datagram.payload, b"ping");
    }

    /// Verifies encoding of a SOCKS5 UDP response with an IPv4 source.
    #[test]
    fn encodes_socks5_udp_ipv4_response() {
        let source = SocketAddr::from((Ipv4Addr::new(8, 8, 8, 8), 53));
        let packet = encode_udp_response(source, b"pong");
        assert_eq!(
            packet,
            vec![0x00, 0x00, 0x00, 0x01, 8, 8, 8, 8, 0x00, 0x35, b'p', b'o', b'n', b'g',]
        );
    }

    /// Verifies that SOCKS5 username strings can carry generic parameters.
    #[test]
    fn extracts_parameters_from_socks_username() {
        let definitions = vec![ParameterDefinition::new(
            PROXY_HEADER_PARAMETER_COUNTRY,
            PROXY_PARAMETER_COUNTRY,
            false,
        )];

        let context = context_from_socks_username("country=be", &definitions);

        assert_eq!(context.get_string(PROXY_PARAMETER_COUNTRY), Some("BE"));
    }
}
