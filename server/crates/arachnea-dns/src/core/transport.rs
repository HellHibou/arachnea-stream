use crate::core::{
    protocol::record_from_hickory, Answer, AnswerMetadata, CacheState, DnsError, DnssecState,
    QueryRequest, Result, Upstream,
};
use hickory_proto::{
    op::{Message, MessageType, OpCode, Query, ResponseCode},
    rr::Name,
    serialize::binary::{BinEncodable, BinEncoder},
};
use std::{
    net::SocketAddr,
    sync::atomic::{AtomicU16, Ordering},
    time::Duration,
};
#[cfg(feature = "dot")]
use std::{net::ToSocketAddrs, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio::{net::UdpSocket, time::timeout};

/// Resolves a query through a classic UDP upstream.
///
/// # Parameters
///
/// - `query`: DNS query to resolve.
/// - `upstream`: Upstream DNS server configuration.
/// - `addr`: Socket address of the upstream server.
/// - `default_timeout_ms`: Default timeout in milliseconds for the query.
///
/// # Returns
///
/// DNS answer containing the resolved records.
///
/// # Errors
///
/// Returns an error when UDP communication fails, the query times out, or the
/// DNS response cannot be parsed.
pub(crate) async fn resolve_udp(
    query: &QueryRequest,
    upstream: &Upstream,
    addr: SocketAddr,
    default_timeout_ms: u64,
) -> Result<Answer> {
    let bytes = encode_query(query)?;

    let bind_addr = if addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let socket = UdpSocket::bind(bind_addr).await?;
    socket.send_to(&bytes, addr).await?;

    let mut buf = vec![0_u8; 4096];
    let timeout_ms = upstream.timeout_ms.unwrap_or(default_timeout_ms);
    let (len, _) = timeout(
        Duration::from_millis(timeout_ms),
        socket.recv_from(&mut buf),
    )
    .await
    .map_err(|_| DnsError::Timeout)??;
    buf.truncate(len);

    let response =
        Message::from_vec(&buf).map_err(|error| DnsError::Protocol(error.to_string()))?;
    if response.truncated() {
        return resolve_tcp(query, upstream, addr, default_timeout_ms).await;
    }

    decode_message(query, upstream, response)
}

/// Resolves a query through classic DNS-over-TCP.
///
/// # Parameters
///
/// - `query`: DNS query to resolve.
/// - `upstream`: Upstream DNS server configuration.
/// - `addr`: Socket address of the upstream server.
/// - `default_timeout_ms`: Default timeout in milliseconds for the query.
///
/// # Returns
///
/// DNS answer containing the resolved records.
///
/// # Errors
///
/// Returns an error when TCP connection fails, the query times out, I/O operations
/// fail, or the DNS response cannot be parsed.
pub(crate) async fn resolve_tcp(
    query: &QueryRequest,
    upstream: &Upstream,
    addr: SocketAddr,
    default_timeout_ms: u64,
) -> Result<Answer> {
    let bytes = encode_query(query)?;
    let timeout_ms = upstream.timeout_ms.unwrap_or(default_timeout_ms);
    let mut stream = timeout(Duration::from_millis(timeout_ms), TcpStream::connect(addr))
        .await
        .map_err(|_| DnsError::Timeout)??;

    timeout(Duration::from_millis(timeout_ms), async {
        stream
            .write_all(&(bytes.len() as u16).to_be_bytes())
            .await?;
        stream.write_all(&bytes).await?;

        let mut len_bytes = [0_u8; 2];
        stream.read_exact(&mut len_bytes).await?;
        let len = u16::from_be_bytes(len_bytes) as usize;
        let mut response = vec![0_u8; len];
        stream.read_exact(&mut response).await?;
        Ok::<_, std::io::Error>(response)
    })
    .await
    .map_err(|_| DnsError::Timeout)?
    .map_err(DnsError::Io)
    .and_then(|response| decode_response(query, upstream, &response))
}

/// Resolves a query through DNS-over-TLS.
///
/// # Parameters
///
/// - `query`: DNS query to resolve.
/// - `upstream`: Upstream DNS server configuration.
/// - `host`: Hostname of the DoT server.
/// - `port`: Port number of the DoT server.
/// - `default_timeout_ms`: Default timeout in milliseconds for the query.
///
/// # Returns
///
/// DNS answer containing the resolved records.
///
/// # Errors
///
/// Returns an error when hostname resolution fails, TLS connection cannot be
/// established, TLS certificate validation fails, or the DNS response cannot be parsed.
#[cfg(feature = "dot")]
pub(crate) async fn resolve_dot(
    query: &QueryRequest,
    upstream: &Upstream,
    host: &str,
    port: u16,
    default_timeout_ms: u64,
) -> Result<Answer> {
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|error| DnsError::UpstreamUnavailable(error.to_string()))?
        .next()
        .ok_or_else(|| DnsError::UpstreamUnavailable(format!("cannot resolve DoT host {host}")))?;
    let timeout_ms = upstream.timeout_ms.unwrap_or(default_timeout_ms);
    let tcp = timeout(Duration::from_millis(timeout_ms), TcpStream::connect(addr))
        .await
        .map_err(|_| DnsError::Timeout)??;

    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| DnsError::UpstreamUnavailable(error.to_string()))?
    .with_root_certificates(root_store)
    .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let server_name = rustls::pki_types::ServerName::try_from(host.to_owned())
        .map_err(|error| DnsError::InvalidConfig(format!("invalid DoT server name: {error}")))?;
    let mut stream = timeout(
        Duration::from_millis(timeout_ms),
        connector.connect(server_name, tcp),
    )
    .await
    .map_err(|_| DnsError::Timeout)?
    .map_err(|error| DnsError::UpstreamUnavailable(error.to_string()))?;

    let bytes = encode_query(query)?;
    let response = timeout(Duration::from_millis(timeout_ms), async {
        stream
            .write_all(&(bytes.len() as u16).to_be_bytes())
            .await?;
        stream.write_all(&bytes).await?;

        let mut len_bytes = [0_u8; 2];
        stream.read_exact(&mut len_bytes).await?;
        let len = u16::from_be_bytes(len_bytes) as usize;
        let mut response = vec![0_u8; len];
        stream.read_exact(&mut response).await?;
        Ok::<_, std::io::Error>(response)
    })
    .await
    .map_err(|_| DnsError::Timeout)?
    .map_err(DnsError::Io)?;

    decode_response(query, upstream, &response)
}

/// Resolves a query through DNS-over-HTTPS using RFC 8484 wire-format POST.
///
/// # Parameters
///
/// - `query`: DNS query to resolve.
/// - `upstream`: Upstream DNS server configuration.
/// - `url`: HTTPS URL of the DoH endpoint.
/// - `default_timeout_ms`: Default timeout in milliseconds for the query.
///
/// # Returns
///
/// DNS answer containing the resolved records.
///
/// # Errors
///
/// Returns an error when HTTP request fails, the request times out, the HTTP
/// response indicates an error status, or the DNS response cannot be parsed.
#[cfg(feature = "doh")]
pub(crate) async fn resolve_doh(
    query: &QueryRequest,
    upstream: &Upstream,
    url: &str,
    default_timeout_ms: u64,
) -> Result<Answer> {
    let body = encode_query(query)?;
    let timeout_ms = upstream.timeout_ms.unwrap_or(default_timeout_ms);
    let client = rquest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|error| DnsError::UpstreamUnavailable(error.to_string()))?;

    let response = client
        .post(url)
        .header(rquest::header::ACCEPT, "application/dns-message")
        .header(rquest::header::CONTENT_TYPE, "application/dns-message")
        .body(body)
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                DnsError::Timeout
            } else {
                DnsError::UpstreamUnavailable(error.to_string())
            }
        })?;

    if !response.status().is_success() {
        return Err(DnsError::UpstreamUnavailable(format!(
            "DoH upstream returned HTTP {}",
            response.status()
        )));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|error| DnsError::Protocol(error.to_string()))?;
    decode_response(query, upstream, &bytes)
}

/// Encodes a DNS query into wire-format bytes.
///
/// # Parameters
///
/// - `query`: Query request to encode.
///
/// # Returns
///
/// DNS message bytes suitable for UDP or TCP transport.
///
/// # Errors
///
/// Returns an error when the DNS name cannot be encoded.
fn encode_query(query: &QueryRequest) -> Result<Vec<u8>> {
    let mut message = Message::new();
    message.set_id(rand_id());
    message.set_message_type(MessageType::Query);
    message.set_op_code(OpCode::Query);
    message.set_recursion_desired(true);

    let name = Name::from_ascii(format!("{}.", query.name))
        .map_err(|error| DnsError::Protocol(error.to_string()))?;
    message.add_query(Query::query(name, query.record_type.to_hickory()));

    let mut bytes = Vec::with_capacity(512);
    message
        .emit(&mut BinEncoder::new(&mut bytes))
        .map_err(|error| DnsError::Protocol(error.to_string()))?;
    Ok(bytes)
}

/// Decodes a DNS response from wire-format bytes.
///
/// # Parameters
///
/// - `query`: Original query used to validate the response.
/// - `upstream`: Upstream that produced the response.
/// - `bytes`: Wire-format DNS response bytes.
///
/// # Returns
///
/// Decoded DNS answer.
///
/// # Errors
///
/// Returns an error when the response is malformed or does not satisfy the
/// query.
fn decode_response(query: &QueryRequest, upstream: &Upstream, bytes: &[u8]) -> Result<Answer> {
    let response =
        Message::from_vec(bytes).map_err(|error| DnsError::Protocol(error.to_string()))?;
    decode_message(query, upstream, response)
}

/// Decodes a parsed DNS message into an answer.
///
/// # Parameters
///
/// - `query`: Original query used to validate the response.
/// - `upstream`: Upstream that produced the response.
/// - `response`: Parsed DNS message.
///
/// # Returns
///
/// Decoded DNS answer.
///
/// # Errors
///
/// Returns an error when the response code is not successful or no matching
/// data is present.
fn decode_message(query: &QueryRequest, upstream: &Upstream, response: Message) -> Result<Answer> {
    match response.response_code() {
        ResponseCode::NoError => {}
        ResponseCode::NXDomain => return Err(DnsError::Nxdomain(query.name.clone())),
        ResponseCode::ServFail => return Err(DnsError::Servfail),
        code => return Err(DnsError::Protocol(format!("upstream returned {code:?}"))),
    }

    let mut records = Vec::new();
    let mut cname_chain = Vec::new();
    for answer in response.answers() {
        if let Some(record) = record_from_hickory(answer)? {
            if let crate::core::RecordData::CNAME { name } = &record.data {
                cname_chain.push(name.clone());
            }
            if record.data.record_type() == query.record_type
                || record.data.record_type() == crate::core::RecordType::CNAME
            {
                records.push(record);
            }
        }
    }

    let ttl = records.iter().map(|record| record.ttl).min();
    if records.is_empty() {
        return Err(DnsError::NoData {
            name: query.name.clone(),
            record_type: query.record_type,
        });
    }

    Ok(Answer {
        query: query.clone(),
        records,
        metadata: AnswerMetadata {
            ttl,
            cname_chain,
            upstream: Some(upstream.name.clone()),
            cache: CacheState::Miss,
            dnssec: DnssecState::Off,
            policy: None,
        },
    })
}

/// Generates a DNS message identifier.
///
/// # Returns
///
/// A best-effort non-cryptographic DNS transaction identifier.
fn rand_id() -> u16 {
    static NEXT_ID: AtomicU16 = AtomicU16::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}
