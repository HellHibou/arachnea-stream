use crate::core::{record_to_hickory, Answer, ArachneaDnsCore, DnsError, QueryRequest, RecordType};
use anyhow::{Context, Result};
use hickory_proto::{
    op::{Message, MessageType, OpCode, ResponseCode},
    serialize::binary::{BinEncodable, BinEncoder},
};
use tracing::{debug, warn};

/// Handles one DNS wire-format packet by delegating resolution to the core.
///
/// # Parameters
///
/// - `core`: DNS core used to resolve the decoded query.
/// - `packet`: Wire-format DNS request bytes.
///
/// # Returns
///
/// Wire-format DNS response bytes.
///
/// # Errors
///
/// Returns an error when the request or response cannot be decoded or encoded.
pub async fn handle_packet(core: &ArachneaDnsCore, packet: &[u8]) -> Result<Vec<u8>> {
    let request = Message::from_vec(packet).context("invalid DNS message")?;
    let Some(query) = request.queries().first() else {
        return encode_error(&request, ResponseCode::FormErr);
    };

    let name = query.name().to_ascii().trim_end_matches('.').to_owned();
    let record_type = match RecordType::try_from(query.query_type()) {
        Ok(record_type) => record_type,
        Err(_) => return encode_error(&request, ResponseCode::NotImp),
    };
    debug!(%name, ?record_type, "handling DNS packet");

    match core.resolve(QueryRequest::new(name, record_type)).await {
        Ok(answer) => encode_answer(&request, &answer),
        Err(DnsError::Nxdomain(_)) => encode_error(&request, ResponseCode::NXDomain),
        Err(DnsError::NoData { .. }) => {
            encode_answer(&request, &empty_answer(&request, record_type))
        }
        Err(DnsError::Blocked(_)) => encode_error(&request, ResponseCode::Refused),
        Err(error) => {
            warn!(%error, "core resolution failed");
            encode_error(&request, ResponseCode::ServFail)
        }
    }
}

/// Encodes a DNS answer for a wire-format response.
///
/// # Parameters
///
/// - `request`: Original DNS request message.
/// - `answer`: Resolver answer to encode.
///
/// # Returns
///
/// Wire-format DNS response bytes.
///
/// # Errors
///
/// Returns an error when the response cannot be encoded.
fn encode_answer(request: &Message, answer: &Answer) -> Result<Vec<u8>> {
    let mut response = response_base(request, ResponseCode::NoError);
    for record in &answer.records {
        response.add_answer(record_to_hickory(record)?);
    }
    encode_message(response)
}

/// Encodes an error DNS response.
///
/// # Parameters
///
/// - `request`: Original DNS request message.
/// - `code`: DNS response code to send.
///
/// # Returns
///
/// Wire-format DNS response bytes.
///
/// # Errors
///
/// Returns an error when the response cannot be encoded.
fn encode_error(request: &Message, code: ResponseCode) -> Result<Vec<u8>> {
    encode_message(response_base(request, code))
}

/// Creates the common response envelope for a request.
///
/// # Parameters
///
/// - `request`: Original DNS request message.
/// - `code`: Response code to place in the envelope.
///
/// # Returns
///
/// DNS response message with header fields copied from the request.
fn response_base(request: &Message, code: ResponseCode) -> Message {
    let mut response = Message::new();
    response.set_id(request.id());
    response.set_message_type(MessageType::Response);
    response.set_op_code(OpCode::Query);
    response.set_response_code(code);
    response.set_recursion_desired(request.recursion_desired());
    response.set_recursion_available(true);
    for query in request.queries() {
        response.add_query(query.clone());
    }
    response
}

/// Serializes a DNS message into wire-format bytes.
///
/// # Parameters
///
/// - `message`: DNS message to serialize.
///
/// # Returns
///
/// Encoded DNS message bytes.
///
/// # Errors
///
/// Returns an error when serialization fails.
fn encode_message(message: Message) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(512);
    message
        .emit(&mut BinEncoder::new(&mut bytes))
        .context("cannot encode DNS response")?;
    Ok(bytes)
}

/// Builds an empty answer for a query and record type.
///
/// # Parameters
///
/// - `request`: Original DNS request message.
/// - `record_type`: Requested DNS record type.
///
/// # Returns
///
/// Empty resolver answer.
fn empty_answer(request: &Message, record_type: RecordType) -> Answer {
    let name = request
        .queries()
        .first()
        .map(|query| query.name().to_ascii())
        .unwrap_or_default();
    Answer {
        query: QueryRequest::new(name, record_type),
        records: Vec::new(),
        metadata: crate::core::AnswerMetadata {
            ttl: None,
            cname_chain: Vec::new(),
            upstream: None,
            cache: crate::core::CacheState::Disabled,
            dnssec: crate::core::DnssecState::Off,
            policy: None,
        },
    }
}
