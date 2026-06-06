use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::UdpSocket;
use tokio::time;

use crate::core::{
    ApplicationProtocol, Destination, DestinationAddress, ProxyError, ProxyNodeCredentials,
    ProxyStream, Result,
};

/// SOCKS5 UDP association opened against an upstream proxy.
pub struct Socks5UdpAssociation {
    socket: UdpSocket,
    relay_addr: SocketAddr,
    _control: ProxyStream,
}

impl Socks5UdpAssociation {
    /// Creates a SOCKS5 UDP association from a relay socket and TCP control stream.
    ///
    /// # Parameters
    ///
    /// - `socket`: UDP socket used to communicate with the upstream relay.
    /// - `relay_addr`: Upstream SOCKS5 UDP relay address.
    /// - `control`: TCP control stream that keeps the association alive.
    ///
    /// # Returns
    ///
    /// SOCKS5 UDP association wrapper.
    pub fn new(socket: UdpSocket, relay_addr: SocketAddr, control: ProxyStream) -> Self {
        Self {
            socket,
            relay_addr,
            _control: control,
        }
    }

    /// Returns the upstream UDP relay address.
    pub fn relay_addr(&self) -> SocketAddr {
        self.relay_addr
    }

    /// Sends one UDP payload through the upstream SOCKS5 relay.
    ///
    /// # Parameters
    ///
    /// - `destination`: Target destination encoded in the SOCKS5 UDP header.
    /// - `payload`: UDP payload to relay.
    ///
    /// # Returns
    ///
    /// Number of packet bytes sent to the relay.
    ///
    /// # Errors
    ///
    /// Returns an error when encoding or socket send fails.
    pub async fn send_datagram(
        &mut self,
        destination: &Destination,
        payload: &[u8],
    ) -> Result<usize> {
        let packet = encode_udp_packet(destination, payload)?;
        Ok(self.socket.send_to(&packet, self.relay_addr).await?)
    }

    /// Receives one UDP payload from the upstream SOCKS5 relay.
    ///
    /// # Parameters
    ///
    /// - `payload`: Caller-provided buffer that receives the decoded payload.
    ///
    /// # Returns
    ///
    /// Source destination and number of payload bytes written.
    ///
    /// # Errors
    ///
    /// Returns an error when socket receive, packet decoding, or buffer sizing
    /// fails.
    pub async fn recv_datagram(&mut self, payload: &mut [u8]) -> Result<(Destination, usize)> {
        let mut packet = vec![0u8; 65_535];
        let (len, _source) = self.socket.recv_from(&mut packet).await?;
        let datagram = decode_udp_packet(&packet[..len])?;
        if datagram.payload.len() > payload.len() {
            return Err(ProxyError::Protocol(
                "socks5 udp payload exceeds caller buffer".to_string(),
            ));
        }
        payload[..datagram.payload.len()].copy_from_slice(datagram.payload);
        Ok((datagram.destination, datagram.payload.len()))
    }
}

/// Performs a SOCKS5 CONNECT handshake over an existing stream.
///
/// # Parameters
///
/// - `stream`: Stream connected to the SOCKS5 upstream.
/// - `destination`: Target destination requested through the upstream.
/// - `timeout`: Maximum duration for the handshake.
/// - `credentials`: Optional username/password credentials for the upstream.
///
/// # Errors
///
/// Returns an error when negotiation, request encoding, upstream response, or
/// timeout handling fails.
pub async fn connect_tunnel<S>(
    stream: &mut S,
    destination: &Destination,
    timeout: Duration,
    credentials: Option<&ProxyNodeCredentials>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    time::timeout(timeout, async {
        negotiate_method(stream, credentials).await?;

        let request = encode_request(0x01, destination)?;
        stream.write_all(&request).await?;
        read_reply(stream).await.map(|_| ())
    })
    .await
    .map_err(|_| ProxyError::Timeout("socks5 connect handshake"))?
}

/// Performs a SOCKS4 or SOCKS4a CONNECT handshake over an existing stream.
///
/// Hostname destinations are encoded with the SOCKS4a extension. Literal IPv4
/// destinations use the original SOCKS4 address field.
///
/// # Parameters
///
/// - `stream`: Stream connected to the SOCKS4 upstream.
/// - `destination`: Target destination requested through the upstream.
/// - `timeout`: Maximum duration for the handshake.
/// - `user_id`: Optional SOCKS4 user identifier sent before the target domain.
///
/// # Errors
///
/// Returns an error when request encoding, upstream response, or timeout
/// handling fails.
pub async fn connect_tunnel_v4<S>(
    stream: &mut S,
    destination: &Destination,
    timeout: Duration,
    user_id: Option<&str>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    time::timeout(timeout, async {
        let request = encode_socks4_connect_request(destination, user_id.unwrap_or(""))?;
        stream.write_all(&request).await?;
        read_socks4_reply(stream).await
    })
    .await
    .map_err(|_| ProxyError::Timeout("socks4 connect handshake"))?
}

/// Performs a SOCKS5 UDP ASSOCIATE handshake over an existing stream.
///
/// # Parameters
///
/// - `stream`: Stream connected to the SOCKS5 upstream.
/// - `timeout`: Maximum duration for the handshake.
/// - `credentials`: Optional username/password credentials for the upstream.
///
/// # Returns
///
/// Relay endpoint returned by the upstream proxy.
///
/// # Errors
///
/// Returns an error when negotiation, upstream response, or timeout handling
/// fails.
pub async fn udp_associate<S>(
    stream: &mut S,
    timeout: Duration,
    credentials: Option<&ProxyNodeCredentials>,
) -> Result<String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    time::timeout(timeout, async {
        negotiate_method(stream, credentials).await?;
        let destination = Destination::host_port("0.0.0.0", 0);
        let request = encode_request(0x03, &destination)?;
        stream.write_all(&request).await?;
        read_reply(stream).await
    })
    .await
    .map_err(|_| ProxyError::Timeout("socks5 udp associate handshake"))?
}

/// Negotiates SOCKS5 no-auth or username/password authentication.
///
/// # Parameters
///
/// - `stream`: Stream connected to the SOCKS5 upstream.
/// - `credentials`: Optional credentials to send to the upstream.
///
/// # Errors
///
/// Returns an error when the upstream rejects the selected method or the
/// authentication exchange fails.
async fn negotiate_method<S>(
    stream: &mut S,
    credentials: Option<&ProxyNodeCredentials>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if let Some(credentials) = credentials {
        stream.write_all(&[0x05, 0x01, 0x02]).await?;
        let mut method = [0u8; 2];
        stream.read_exact(&mut method).await?;
        if method != [0x05, 0x02] {
            return Err(ProxyError::UpstreamRejected(
                "socks5 upstream does not accept username/password".to_string(),
            ));
        }
        authenticate_username_password(stream, credentials).await
    } else {
        stream.write_all(&[0x05, 0x01, 0x00]).await?;
        let mut method = [0u8; 2];
        stream.read_exact(&mut method).await?;
        if method != [0x05, 0x00] {
            return Err(ProxyError::UpstreamRejected(
                "socks5 upstream does not accept no-auth".to_string(),
            ));
        }
        Ok(())
    }
}

/// Sends SOCKS5 username/password credentials to an upstream.
///
/// # Parameters
///
/// - `stream`: Stream connected to the SOCKS5 upstream.
/// - `credentials`: Credentials to send.
///
/// # Errors
///
/// Returns an error when credentials exceed SOCKS5 length limits, the stream
/// fails, or the upstream rejects authentication.
async fn authenticate_username_password<S>(
    stream: &mut S,
    credentials: &ProxyNodeCredentials,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let username_len = u8::try_from(credentials.username.len()).map_err(|_| {
        ProxyError::InvalidDestination("socks5 username is longer than 255 bytes".to_string())
    })?;
    let password_len = u8::try_from(credentials.password.len()).map_err(|_| {
        ProxyError::InvalidDestination("socks5 password is longer than 255 bytes".to_string())
    })?;
    let mut request = vec![0x01, username_len];
    request.extend_from_slice(credentials.username.as_bytes());
    request.push(password_len);
    request.extend_from_slice(credentials.password.as_bytes());
    stream.write_all(&request).await?;
    let mut response = [0u8; 2];
    stream.read_exact(&mut response).await?;
    if response == [0x01, 0x00] {
        Ok(())
    } else {
        Err(ProxyError::UpstreamRejected(
            "socks5 upstream rejected username/password".to_string(),
        ))
    }
}

/// Encodes a SOCKS5 command request.
///
/// # Parameters
///
/// - `command`: SOCKS5 command byte, such as `CONNECT` or `UDP ASSOCIATE`.
/// - `destination`: Destination to encode in the request.
///
/// # Returns
///
/// Raw SOCKS5 request bytes.
///
/// # Errors
///
/// Returns an error when the destination hostname exceeds SOCKS5 limits.
fn encode_request(command: u8, destination: &Destination) -> Result<Vec<u8>> {
    let mut request = vec![0x05, command, 0x00];
    match &destination.address {
        DestinationAddress::Ip(IpAddr::V4(ip)) => {
            request.push(0x01);
            request.extend_from_slice(&ip.octets());
        }
        DestinationAddress::Ip(IpAddr::V6(ip)) => {
            request.push(0x04);
            request.extend_from_slice(&ip.octets());
        }
        DestinationAddress::Host(host) => {
            let len = u8::try_from(host.len()).map_err(|_| {
                ProxyError::InvalidDestination(
                    "socks5 hostnames are limited to 255 bytes".to_string(),
                )
            })?;
            request.push(0x03);
            request.push(len);
            request.extend_from_slice(host.as_bytes());
        }
    }
    request.extend_from_slice(&destination.port.to_be_bytes());
    Ok(request)
}

/// Encodes a SOCKS4 CONNECT request.
///
/// # Parameters
///
/// - `destination`: Target destination to encode.
/// - `user_id`: SOCKS4 user identifier. It must not contain NUL bytes.
///
/// # Returns
///
/// Raw SOCKS4 or SOCKS4a request bytes.
///
/// # Errors
///
/// Returns an error when the destination cannot be represented by SOCKS4 or
/// SOCKS4a.
fn encode_socks4_connect_request(destination: &Destination, user_id: &str) -> Result<Vec<u8>> {
    if user_id.as_bytes().contains(&0) {
        return Err(ProxyError::InvalidDestination(
            "socks4 user id must not contain nul bytes".to_string(),
        ));
    }

    let mut request = vec![0x04, 0x01];
    request.extend_from_slice(&destination.port.to_be_bytes());
    match &destination.address {
        DestinationAddress::Ip(IpAddr::V4(ip)) => {
            request.extend_from_slice(&ip.octets());
            request.extend_from_slice(user_id.as_bytes());
            request.push(0x00);
        }
        DestinationAddress::Ip(IpAddr::V6(_)) => {
            return Err(ProxyError::InvalidDestination(
                "socks4 cannot encode ipv6 destinations".to_string(),
            ));
        }
        DestinationAddress::Host(host) => {
            if host.is_empty() || host.as_bytes().contains(&0) {
                return Err(ProxyError::InvalidDestination(
                    "socks4a hostnames must be non-empty and contain no nul bytes".to_string(),
                ));
            }
            request.extend_from_slice(&Ipv4Addr::new(0, 0, 0, 1).octets());
            request.extend_from_slice(user_id.as_bytes());
            request.push(0x00);
            request.extend_from_slice(host.as_bytes());
            request.push(0x00);
        }
    }
    Ok(request)
}

/// Reads a SOCKS4 reply from an upstream proxy.
///
/// # Parameters
///
/// - `stream`: Stream connected to the SOCKS4 peer.
///
/// # Errors
///
/// Returns an error when the reply is malformed, rejected, or cannot be read.
async fn read_socks4_reply<S>(stream: &mut S) -> Result<()>
where
    S: AsyncRead + Unpin,
{
    let mut reply = [0u8; 8];
    stream.read_exact(&mut reply).await?;
    if reply[0] != 0x00 && reply[0] != 0x04 {
        return Err(ProxyError::Protocol(
            "socks4 reply used an invalid version byte".to_string(),
        ));
    }
    if reply[1] == 0x5a {
        Ok(())
    } else {
        let label = socks4_reply_label(reply[1]);
        Err(ProxyError::UpstreamRejected(format!(
            "socks4 reply code {} ({label})",
            reply[1],
        )))
    }
}

/// Reads a SOCKS5 reply and returns its bound endpoint.
///
/// # Parameters
///
/// - `stream`: Stream connected to the SOCKS5 peer.
///
/// # Returns
///
/// Bound endpoint returned by the upstream proxy.
///
/// # Errors
///
/// Returns an error when the reply is malformed, rejected, or cannot be read.
async fn read_reply<S>(stream: &mut S) -> Result<String>
where
    S: AsyncRead + Unpin,
{
    let mut head = [0u8; 4];
    stream.read_exact(&mut head).await?;
    if head[0] != 0x05 {
        return Err(ProxyError::Protocol(
            "socks5 reply used an invalid version".to_string(),
        ));
    }
    if head[1] != 0x00 {
        let label = socks5_reply_label(head[1]);
        return Err(ProxyError::UpstreamRejected(format!(
            "socks5 reply code {} ({label})",
            head[1],
        )));
    }
    let host = match head[3] {
        0x01 => {
            let mut ip = [0u8; 4];
            stream.read_exact(&mut ip).await?;
            IpAddr::from(ip).to_string()
        }
        0x03 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await?;
            let mut host = vec![0u8; len[0] as usize];
            stream.read_exact(&mut host).await?;
            String::from_utf8(host)
                .map_err(|_| ProxyError::Protocol("socks5 domain is not utf-8".to_string()))?
        }
        0x04 => {
            let mut ip = [0u8; 16];
            stream.read_exact(&mut ip).await?;
            format!("[{}]", IpAddr::from(ip))
        }
        other => {
            return Err(ProxyError::Protocol(format!(
                "unsupported socks5 address type {other}"
            )));
        }
    };
    let mut port = [0u8; 2];
    stream.read_exact(&mut port).await?;
    Ok(format!("{host}:{}", u16::from_be_bytes(port)))
}

/// Returns a readable label for a SOCKS5 reply code.
///
/// # Parameters
///
/// - `code`: SOCKS5 reply code from an upstream response.
///
/// # Returns
///
/// Human-readable reply label.
fn socks5_reply_label(code: u8) -> &'static str {
    match code {
        0x01 => "general failure",
        0x02 => "connection not allowed",
        0x03 => "network unreachable",
        0x04 => "host unreachable",
        0x05 => "connection refused",
        0x06 => "ttl expired",
        0x07 => "command not supported",
        0x08 => "address type not supported",
        _ => "unknown failure",
    }
}

/// Returns a readable label for a SOCKS4 reply code.
///
/// # Parameters
///
/// - `code`: SOCKS4 reply code from an upstream response.
///
/// # Returns
///
/// Human-readable reply label.
fn socks4_reply_label(code: u8) -> &'static str {
    match code {
        0x5a => "request granted",
        0x5b => "request rejected or failed",
        0x5c => "request failed because identd is not reachable",
        0x5d => "request failed because identd user does not match",
        _ => "unknown failure",
    }
}

/// Encodes a UDP payload for a SOCKS5 UDP relay.
///
/// # Parameters
///
/// - `destination`: Target destination for the UDP payload.
/// - `payload`: Application UDP payload.
///
/// # Returns
///
/// SOCKS5 UDP packet ready to send to the upstream relay.
///
/// # Errors
///
/// Returns an error when the hostname cannot be represented by SOCKS5.
fn encode_udp_packet(destination: &Destination, payload: &[u8]) -> Result<Vec<u8>> {
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

/// Decodes a SOCKS5 UDP packet received from an upstream relay.
///
/// # Parameters
///
/// - `packet`: Raw UDP packet received from the SOCKS5 relay.
///
/// # Returns
///
/// Decoded source destination and borrowed payload.
///
/// # Errors
///
/// Returns an error when the packet is malformed or fragmented.
fn decode_udp_packet(packet: &[u8]) -> Result<Socks5UdpDatagram<'_>> {
    if packet.len() < 4 || packet[0] != 0 || packet[1] != 0 {
        return Err(ProxyError::Protocol(
            "invalid socks5 udp datagram header".to_string(),
        ));
    }
    if packet[2] != 0 {
        return Err(ProxyError::Unsupported("fragmented socks5 udp datagrams"));
    }
    let mut offset = 4;
    let address = match packet[3] {
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
    Ok(Socks5UdpDatagram {
        destination: Destination {
            address,
            port,
            protocol: ApplicationProtocol::Udp,
        },
        payload: &packet[offset..],
    })
}

/// Decoded SOCKS5 UDP datagram borrowed from a receive buffer.
struct Socks5UdpDatagram<'a> {
    destination: Destination,
    payload: &'a [u8],
}
