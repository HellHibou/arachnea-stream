//! HTTP body framing, buffered compatibility handling and post-response actions.

use super::response_head::{header_value_case_insensitive, remove_header_case_insensitive};
use crate::core::http::body_readers::{
    ChunkedBodyReader, ContentLengthBodyReader, UntilEofBodyReader,
};
use crate::core::Result;
use crate::http::actions::{apply_post_actions, PostActionContext, ProxyHttpPostActionConfig};
use futures::StreamExt;
use std::collections::HashMap;
use tokio::io::{AsyncRead, AsyncReadExt, BufReader};
use tokio_util::io::ReaderStream;

/// Reads a response body that must be buffered for post-response processing.
///
/// # Arguments
/// * `stream` - Connected stream from which HTTP or proxy-protocol bytes are read or written.
/// * `headers` - HTTP headers inspected or updated by this operation.
/// * `is_chunked` - Whether the response declares chunked transfer encoding.
/// * `tolerate_dechunked_bodies` - Whether to retry raw-body interpretation when chunk framing is already removed.
///
pub(super) async fn read_buffered_response_body<S>(
    stream: S,
    headers: &HashMap<String, String>,
    is_chunked: bool,
    tolerate_dechunked_bodies: bool,
) -> Result<Vec<u8>>
where
    S: AsyncRead + Send + Unpin + 'static,
{
    if is_chunked && tolerate_dechunked_bodies {
        // Inspect only the first line before choosing strict framing or compatibility mode.
        let mut reader = BufReader::new(stream);
        let mut prefix = Vec::new();
        while prefix.len() < crate::PROXY_MEDIA_CHUNK_PREFIX_LIMIT {
            let mut byte = [0];
            if reader.read(&mut byte).await? == 0 {
                break;
            }
            prefix.push(byte[0]);
            if byte[0] == b'\n' {
                break;
            }
        }
        let framed = prefix.strip_suffix(b"\r\n").is_some_and(|line| {
            std::str::from_utf8(line)
                .ok()
                .and_then(|line| line.split(';').next())
                .and_then(|size| usize::from_str_radix(size.trim(), 16).ok())
                .is_some()
        });
        let stream = std::io::Cursor::new(prefix).chain(reader);
        let mut body = Vec::new();
        if framed {
            ChunkedBodyReader::new(stream)
                .read_to_end(&mut body)
                .await?;
        } else {
            tracing::warn!(target: "arachnea_proxy::core::http::client",
                "upstream retained Transfer-Encoding: chunked without chunk framing; using raw body"
            );
            UntilEofBodyReader::new(stream)
                .read_to_end(&mut body)
                .await?;
        }
        return Ok(body);
    }

    let reader = select_body_reader(stream, headers);
    let reader_stream = ReaderStream::new(reader);
    let mut body = Vec::new();
    futures::pin_mut!(reader_stream);
    while let Some(chunk) = reader_stream.next().await {
        body.extend_from_slice(&chunk?);
    }
    Ok(body)
}

/// Applies post-response actions to a buffered body and updates invalidated headers.
///
/// # Arguments
/// * `body` - HTTP body bytes to send or transform.
/// * `headers` - HTTP headers inspected or updated by this operation.
/// * `status_code` - HTTP status passed to post-response transformations.
/// * `post_actions` - Post-response transformations to apply after body decoding.
/// * `context` - Request metadata available when resolving replacement variables.
///
pub(super) fn process_response_body(
    body: Vec<u8>,
    headers: &mut HashMap<String, String>,
    status_code: u16,
    post_actions: &[ProxyHttpPostActionConfig],
    context: &PostActionContext,
) -> Result<Vec<u8>> {
    let original_len = body.len();
    let new_body = apply_post_actions(status_code, headers, body, post_actions, context)?;

    if new_body.len() != original_len {
        remove_header_case_insensitive(headers, "content-length");
        remove_header_case_insensitive(headers, "etag");
        remove_header_case_insensitive(headers, "content-md5");
        remove_header_case_insensitive(headers, "digest");
        headers.insert("Content-Length".to_string(), new_body.len().to_string());
    }

    Ok(new_body)
}

/// Selects an appropriate body reader based on response headers.
///
/// # Arguments
/// * `stream` - Connected stream from which HTTP or proxy-protocol bytes are read or written.
/// * `headers` - HTTP headers inspected or updated by this operation.
///
pub(super) fn select_body_reader<S: AsyncRead + Send + Unpin + 'static>(
    stream: S,
    headers: &HashMap<String, String>,
) -> Box<dyn AsyncRead + Send + Unpin> {
    // Check for chunked transfer encoding
    if transfer_encoding_is_chunked(headers) {
        return Box::new(ChunkedBodyReader::new(stream));
    }

    // Check for content-length
    if let Some(cl) = header_value_case_insensitive(headers, "content-length") {
        if let Ok(length) = cl.parse::<usize>() {
            return Box::new(ContentLengthBodyReader::new(stream, length));
        }
    }

    // Default to reading until EOF
    Box::new(UntilEofBodyReader::new(stream))
}

/// Returns whether transfer encoding includes chunked coding.
///
/// # Arguments
/// * `headers` - HTTP headers inspected or updated by this operation.
///
pub(super) fn transfer_encoding_is_chunked(headers: &HashMap<String, String>) -> bool {
    header_value_case_insensitive(headers, "transfer-encoding").is_some_and(|value| {
        value
            .split(',')
            .any(|part| part.trim().eq_ignore_ascii_case("chunked"))
    })
}
