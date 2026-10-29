//! HTTP replies and request metadata adaptation for registered REST commands.

use super::{RestControlerService, RestReply};
use crate::controler::{
    ControlerFunctionInput, ControlerJsonInput, ControlerJsonOutput, ControlerStreamInput,
    ControlerStreamOutput, JsonControlerFunction, ResponseBody, SerializedControlerFunction,
    StreamControlerFunction,
};
use warp::{http::StatusCode, reply, Reply};

impl RestControlerService {
    /// Executes a serialized command and maps its result to a JSON reply.
    ///
    /// # Arguments
    /// - `call`: Registered controller callback.
    /// - `input`: Request input passed unchanged to the callback.
    pub(super) async fn call_and_reply(
        call: &SerializedControlerFunction,
        input: ControlerFunctionInput,
    ) -> (Box<dyn Reply + Send>,) {
        let response: Box<dyn Reply + Send> = match call(input).await {
            Ok(response) => Box::new(reply::json(&response)),
            Err(error) => Box::new(reply::with_status(
                reply::json(&error),
                StatusCode::BAD_REQUEST,
            )),
        };

        (response,)
    }

    /// Executes a header-aware JSON command and builds the HTTP reply.
    ///
    /// Honors the command-provided status code (e.g. `304 Not Modified`) and
    /// additional response headers (e.g. `ETag`). A `304` reply carries no body.
    ///
    /// # Arguments
    /// - `call`: Registered header-aware JSON callback.
    /// - `input`: Payload and request context passed to the callback.
    pub(super) async fn call_and_reply_json(
        call: &JsonControlerFunction,
        input: ControlerJsonInput,
    ) -> (Box<dyn Reply + Send>,) {
        let response: Box<dyn Reply + Send> = match call(input).await {
            Ok(ControlerJsonOutput {
                value,
                status,
                headers,
            }) => {
                let mut builder = warp::http::Response::builder()
                    .status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK))
                    .header("content-type", "application/json");
                for (name, header_value) in &headers {
                    if !name.eq_ignore_ascii_case("content-type") {
                        builder = builder.header(name.as_str(), header_value.as_str());
                    }
                }
                let body = if status == 304 {
                    Vec::new()
                } else {
                    serde_json::to_vec(&value).unwrap_or_else(|_| Vec::new())
                };
                Box::new(builder.body(body).expect("Failed to build JSON response."))
            }
            Err(error) => Box::new(reply::with_status(
                reply::json(&error),
                StatusCode::BAD_REQUEST,
            )),
        };

        (response,)
    }

    /// Executes a stream command, retaining HEAD and streaming-body behavior.
    ///
    /// # Arguments
    /// - `call`: Registered controller callback.
    /// - `input`: Request input passed unchanged to the callback.
    pub(super) async fn call_and_reply_stream(
        call: &StreamControlerFunction,
        input: ControlerStreamInput,
    ) -> RestReply {
        let is_head = input.method.eq_ignore_ascii_case("HEAD");
        match call(input).await {
            Ok(ControlerStreamOutput {
                status,
                body,
                content_type,
                headers,
            }) => {
                let body = match body {
                    ResponseBody::Buffered(mut bytes) => {
                        if is_head {
                            bytes.clear();
                        }
                        warp::hyper::Body::from(bytes)
                    }
                    ResponseBody::Streamed(stream) => {
                        if is_head {
                            warp::hyper::Body::empty()
                        } else {
                            warp::hyper::Body::wrap_stream(stream)
                        }
                    }
                };
                let mut builder = warp::http::Response::builder()
                    .status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK));
                builder = builder.header("content-type", &content_type);

                for (name, value) in &headers {
                    if !name.eq_ignore_ascii_case("content-type") {
                        builder = builder.header(name.as_str(), value.as_str());
                    }
                }

                let response = builder
                    .body(body)
                    .expect("Failed to build stream response.");
                (Box::new(response) as Box<dyn Reply + Send>,)
            }
            Err(error) => {
                let response = warp::http::Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body(error.into_bytes())
                    .expect("Failed to build stream error response.");
                (Box::new(response) as Box<dyn Reply + Send>,)
            }
        }
    }

    /// Copies HTTP headers whose values are representable as strings.
    ///
    /// # Arguments
    /// - `headers`: Request headers supplied by Warp.
    pub(super) fn headers_to_map(
        headers: &warp::http::HeaderMap,
    ) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        for (name, value) in headers.iter() {
            if let Ok(v) = value.to_str() {
                map.insert(name.to_string(), v.to_string());
            }
        }
        map
    }

    /// Returns the peer address of a request, falling back to the unspecified
    /// address when the transport does not expose one.
    ///
    /// The unspecified address is never loopback, so transports without a peer
    /// address are always treated as remote (untrusted) clients.
    ///
    /// # Arguments
    /// - `addr`: Peer address supplied by the trusted transport, when available.
    pub(super) fn remote_peer(addr: Option<std::net::SocketAddr>) -> std::net::SocketAddr {
        addr.unwrap_or_else(|| {
            std::net::SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), 0)
        })
    }
}
