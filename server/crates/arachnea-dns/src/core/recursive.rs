use crate::core::{
    cache::NsecProof, protocol::record_from_hickory, Answer, AnswerMetadata, CacheState, DnsError,
    DnssecState, QueryRequest, RecordData, Result,
};
use hickory_proto::{
    op::{Message, MessageType, OpCode, Query, ResponseCode},
    rr::{Name, RData},
    serialize::binary::{BinEncodable, BinEncoder},
};
use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    time::timeout,
};

/// Result of one recursive DNS resolution attempt.
pub(crate) enum RecursiveResult {
    Answer(Answer),
    Negative {
        error: DnsError,
        nsec_proofs: Vec<NsecProof>,
    },
}

/// Resolves a DNS query by iterating from root hints.
///
/// # Parameters
///
/// - `query`: DNS query to resolve.
/// - `timeout_ms`: Per-network-operation timeout in milliseconds.
/// - `root_hints`: Root server hint entries.
///
/// # Returns
///
/// Recursive resolution result, including negative proofs when available.
///
/// # Errors
///
/// Returns an error when no root hints are usable or resolution exhausts
/// available referrals.
pub(crate) async fn resolve_recursive(
    query: &QueryRequest,
    timeout_ms: u64,
    root_hints: &[String],
) -> Result<RecursiveResult> {
    let roots = parse_root_hints(root_hints)?;
    recursive_lookup(query, roots.clone(), roots, timeout_ms, 0).await
}

/// Performs the recursive lookup state machine.
///
/// # Parameters
///
/// - `query`: DNS query to resolve.
/// - `servers`: Candidate authoritative server IP addresses.
/// - `timeout_ms`: Per-network-operation timeout in milliseconds.
/// - `depth`: Current recursion depth.
///
/// # Returns
///
/// Boxed future that resolves to the recursive result.
fn recursive_lookup<'a>(
    query: &'a QueryRequest,
    roots: Vec<IpAddr>,
    mut servers: Vec<IpAddr>,
    timeout_ms: u64,
    depth: u8,
) -> Pin<Box<dyn Future<Output = Result<RecursiveResult>> + Send + 'a>> {
    Box::pin(async move {
        if depth > 32 {
            return Err(DnsError::UpstreamUnavailable(
                "recursive resolution exceeded referral depth".to_owned(),
            ));
        }

        servers.truncate(8);
        let mut last_error = None;
        for server in servers {
            let response = match query_server(query, server, timeout_ms).await {
                Ok(response) => response,
                Err(error) => {
                    last_error = Some(error);
                    continue;
                }
            };

            match response.response_code() {
                ResponseCode::NoError => {}
                ResponseCode::NXDomain => {
                    return Ok(RecursiveResult::Negative {
                        error: DnsError::Nxdomain(query.name.clone()),
                        nsec_proofs: nsec_proofs(&response),
                    });
                }
                ResponseCode::ServFail => {
                    last_error = Some(DnsError::Servfail);
                    continue;
                }
                code => {
                    last_error = Some(DnsError::Protocol(format!(
                        "authoritative server returned {code:?}"
                    )));
                    continue;
                }
            }

            if let Some(answer) = answer_from_message(query, &response)? {
                return Ok(RecursiveResult::Answer(answer.with_upstream("recursive")));
            }

            let next_servers = referral_servers(&response);
            if !next_servers.is_empty() {
                return recursive_lookup(query, roots, next_servers, timeout_ms, depth + 1).await;
            }

            let ns_names = referral_names(&response);
            if !ns_names.is_empty() {
                let mut resolved_ns = Vec::new();
                for ns_name in ns_names.into_iter().take(4) {
                    let ns_query = QueryRequest::new(ns_name, crate::core::RecordType::A);
                    if let Ok(RecursiveResult::Answer(answer)) = recursive_lookup(
                        &ns_query,
                        roots.clone(),
                        roots.clone(),
                        timeout_ms,
                        depth + 1,
                    )
                    .await
                    {
                        resolved_ns.extend(answer.records.into_iter().filter_map(|record| {
                            if let RecordData::A { address } = record.data {
                                Some(IpAddr::V4(address))
                            } else {
                                None
                            }
                        }));
                    }
                }
                if !resolved_ns.is_empty() {
                    return recursive_lookup(query, roots, resolved_ns, timeout_ms, depth + 1)
                        .await;
                }
            }

            return Ok(RecursiveResult::Negative {
                error: DnsError::NoData {
                    name: query.name.clone(),
                    record_type: query.record_type,
                },
                nsec_proofs: nsec_proofs(&response),
            });
        }

        Err(last_error.unwrap_or_else(|| {
            DnsError::UpstreamUnavailable("no recursive name server responded".to_owned())
        }))
    })
}

/// Parses configured root hints into IP addresses.
///
/// # Parameters
///
/// - `root_hints`: Raw root hint entries.
///
/// # Returns
///
/// Root server IP addresses.
///
/// # Errors
///
/// Returns an error when no root IP address can be parsed.
fn parse_root_hints(root_hints: &[String]) -> Result<Vec<IpAddr>> {
    root_hints
        .iter()
        .map(|hint| {
            hint.parse::<IpAddr>().map_err(|error| {
                DnsError::InvalidConfig(format!("invalid root hint IP address {hint}: {error}"))
            })
        })
        .collect()
}

/// Queries one DNS server, preferring UDP and falling back to TCP when needed.
///
/// # Parameters
///
/// - `query`: DNS query to send.
/// - `server`: Server IP address to contact.
/// - `timeout_ms`: Network timeout in milliseconds.
///
/// # Returns
///
/// Parsed DNS response message.
///
/// # Errors
///
/// Returns an error when UDP and any required TCP fallback fail.
async fn query_server(query: &QueryRequest, server: IpAddr, timeout_ms: u64) -> Result<Message> {
    let socket_addr = SocketAddr::new(server, 53);
    let bind_addr = if server.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let socket = UdpSocket::bind(bind_addr).await?;
    let bytes = encode_iterative_query(query)?;
    socket.send_to(&bytes, socket_addr).await?;

    let mut buf = vec![0_u8; 4096];
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
        return query_server_tcp(query, socket_addr, timeout_ms).await;
    }

    Ok(response)
}

/// Queries one DNS server over TCP.
///
/// # Parameters
///
/// - `query`: DNS query to send.
/// - `server`: Server IP address to contact.
/// - `timeout_ms`: Network timeout in milliseconds.
///
/// # Returns
///
/// Parsed DNS response message.
///
/// # Errors
///
/// Returns an error when the TCP exchange fails or returns malformed data.
async fn query_server_tcp(
    query: &QueryRequest,
    socket_addr: SocketAddr,
    timeout_ms: u64,
) -> Result<Message> {
    let bytes = encode_iterative_query(query)?;
    let mut stream = timeout(
        Duration::from_millis(timeout_ms),
        TcpStream::connect(socket_addr),
    )
    .await
    .map_err(|_| DnsError::Timeout)??;

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

    Message::from_vec(&response).map_err(|error| DnsError::Protocol(error.to_string()))
}

/// Encodes a recursive lookup query.
///
/// # Parameters
///
/// - `query`: DNS query to encode.
///
/// # Returns
///
/// Wire-format DNS query bytes.
///
/// # Errors
///
/// Returns an error when the query name cannot be encoded.
fn encode_iterative_query(query: &QueryRequest) -> Result<Vec<u8>> {
    let mut message = Message::new();
    message.set_id(0);
    message.set_message_type(MessageType::Query);
    message.set_op_code(OpCode::Query);
    message.set_recursion_desired(false);
    let name = Name::from_ascii(format!("{}.", query.name))
        .map_err(|error| DnsError::Protocol(error.to_string()))?;
    message.add_query(Query::query(name, query.record_type.to_hickory()));

    let mut bytes = Vec::with_capacity(512);
    message
        .emit(&mut BinEncoder::new(&mut bytes))
        .map_err(|error| DnsError::Protocol(error.to_string()))?;
    Ok(bytes)
}

/// Extracts a final answer from an authoritative response.
///
/// # Parameters
///
/// - `query`: Original query request.
/// - `response`: DNS response message.
///
/// # Returns
///
/// Final answer when the message contains matching records.
///
/// # Errors
///
/// Returns an error when matching records cannot be converted.
fn answer_from_message(query: &QueryRequest, response: &Message) -> Result<Option<Answer>> {
    let mut records = Vec::new();
    let mut cname_chain = Vec::new();
    for answer in response.answers() {
        if let Some(record) = record_from_hickory(answer)? {
            if let RecordData::CNAME { name } = &record.data {
                cname_chain.push(name.clone());
            }
            if record.data.record_type() == query.record_type
                || record.data.record_type() == crate::core::RecordType::CNAME
            {
                records.push(record);
            }
        }
    }

    if records.is_empty() {
        return Ok(None);
    }

    Ok(Some(Answer {
        query: query.clone(),
        metadata: AnswerMetadata {
            ttl: records.iter().map(|record| record.ttl).min(),
            cname_chain,
            upstream: None,
            cache: CacheState::Miss,
            dnssec: DnssecState::Off,
            policy: None,
        },
        records,
    }))
}

/// Extracts referral glue addresses from a DNS response.
///
/// # Parameters
///
/// - `response`: DNS response message.
///
/// # Returns
///
/// Referral server IP addresses found in additional records.
fn referral_servers(response: &Message) -> Vec<IpAddr> {
    let names = referral_names(response);
    response
        .additionals()
        .iter()
        .filter(|record| {
            names
                .iter()
                .any(|name| *name == crate::core::normalize_name(&record.name().to_ascii()))
        })
        .filter_map(|record| match record.data() {
            RData::A(value) => Some(IpAddr::V4(value.0)),
            RData::AAAA(value) => Some(IpAddr::V6(value.0)),
            _ => None,
        })
        .collect()
}

/// Extracts referral nameserver names from authority records.
///
/// # Parameters
///
/// - `response`: DNS response message.
///
/// # Returns
///
/// Nameserver domain names found in NS records.
fn referral_names(response: &Message) -> Vec<String> {
    response
        .name_servers()
        .iter()
        .filter_map(|record| match record.data() {
            RData::NS(value) => Some(crate::core::normalize_name(&value.to_ascii())),
            _ => None,
        })
        .collect()
}

/// Extracts DNSSEC NSEC proofs from a response.
///
/// # Parameters
///
/// - `response`: DNS response message.
///
/// # Returns
///
/// NSEC proofs present in authority records.
fn nsec_proofs(response: &Message) -> Vec<NsecProof> {
    #[cfg(feature = "dnssec")]
    {
        use hickory_proto::dnssec::rdata::DNSSECRData;

        return response
            .name_servers()
            .iter()
            .filter_map(|record| match record.data() {
                RData::DNSSEC(DNSSECRData::NSEC(nsec)) => Some(NsecProof {
                    owner: crate::core::normalize_name(&record.name().to_ascii()),
                    next_owner: crate::core::normalize_name(&nsec.next_domain_name().to_ascii()),
                    types: nsec.type_bit_maps().collect::<Vec<_>>(),
                }),
                RData::DNSSEC(DNSSECRData::NSEC3(_)) => None,
                _ => None,
            })
            .collect();
    }

    #[cfg(not(feature = "dnssec"))]
    {
        let _ = response;
        Vec::new()
    }
}

/// Helper trait for attaching upstream metadata to answers.
trait WithUpstream {
    /// Attaches upstream metadata to an answer.
    ///
    /// # Parameters
    ///
    /// - `upstream`: Upstream label to record.
    ///
    /// # Returns
    ///
    /// Updated answer.
    fn with_upstream(self, upstream: &str) -> Self;
}

impl WithUpstream for Answer {
    /// Attaches upstream metadata to an answer.
    ///
    /// # Parameters
    ///
    /// - `upstream`: Upstream label to record.
    ///
    /// # Returns
    ///
    /// Updated answer.
    fn with_upstream(mut self, upstream: &str) -> Self {
        self.metadata.upstream = Some(upstream.to_owned());
        self
    }
}
