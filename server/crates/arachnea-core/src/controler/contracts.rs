//! Payloads, request context, response bodies and callbacks shared by controllers.

use super::HEADER_IF_NONE_MATCH;
use bytes::Bytes;
use serde_json::Value;
use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

/// Serialized payload received by controller backends.
///
/// This enum represents the different types of input that controller functions
/// can receive from various sources.
pub enum ControlerFunctionInput {
    /// A JSON payload received from Tauri invocations or HTTP POST requests.
    ///
    /// This variant contains a parsed JSON value that can be deserialized
    /// into specific types.
    Json(Value),
    /// A raw URL query string received from HTTP GET requests.
    ///
    /// This variant contains the raw query string that can be parsed
    /// using URL decoding.
    Query(String),
}

/// Type-erased callback stored by controller backends.
///
/// This type represents a boxed future that resolves to a JSON value or error string.
pub type SerializedControlerFuture =
    Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'static>>;

/// Type-erased async callback stored by controller backends.
///
/// This type represents a function that takes controller input and returns
/// a serialized future.
pub type SerializedControlerFunction =
    Arc<dyn Fn(ControlerFunctionInput) -> SerializedControlerFuture + Send + Sync + 'static>;

/// Output of a header-aware JSON command.
///
/// Extends the plain JSON value with an HTTP status code and additional
/// response headers, used for conditional validation (`ETag` / `304`).
pub struct ControlerJsonOutput {
    /// Serialized JSON payload returned to the caller.
    pub value: Value,
    /// HTTP status code to return (defaults to 200).
    pub status: u16,
    /// Additional response headers (e.g. `ETag`).
    pub headers: HashMap<String, String>,
}

impl From<Value> for ControlerJsonOutput {
    /// Wraps a bare JSON value into a 200 OK output without extra headers.
    ///
    /// # Arguments
    /// * `value` - JSON payload to return with the default status and headers.
    fn from(value: Value) -> Self {
        Self {
            value,
            status: 200,
            headers: HashMap::new(),
        }
    }
}

/// Context of an incoming controller request.
///
/// Carries the HTTP headers received by the controller backend so registered
/// commands can access them without depending on a specific transport
/// (REST HTTP headers, empty Tauri IPC context, ...).
#[derive(Default, Clone)]
pub struct RequestControlerContext {
    /// Incoming request headers as key-value pairs.
    headers: HashMap<String, String>,
    /// TCP address of the remote peer, when the transport exposes one.
    ///
    /// The REST backend fills this from the connection socket address; the
    /// desktop Tauri IPC context leaves it `None`.
    remote_addr: Option<std::net::SocketAddr>,
    /// HTTP method of the incoming request, when the transport exposes one.
    method: Option<String>,
}

impl RequestControlerContext {
    /// Creates a context from the received header map.
    ///
    /// # Arguments
    /// * `headers` - Incoming HTTP request headers as key-value pairs.
    pub fn new(headers: HashMap<String, String>) -> Self {
        Self {
            headers,
            remote_addr: None,
            method: None,
        }
    }

    /// Sets the TCP address of the remote peer.
    ///
    /// # Arguments
    /// * `remote_addr` - Socket address of the client connection.
    pub fn with_remote_addr(mut self, remote_addr: std::net::SocketAddr) -> Self {
        self.remote_addr = Some(remote_addr);
        self
    }

    /// Sets the HTTP method of the incoming request.
    ///
    /// # Arguments
    /// * `method` - HTTP method name, such as `GET` or `POST`.
    pub fn with_method(mut self, method: impl Into<String>) -> Self {
        self.method = Some(method.into());
        self
    }

    /// Returns the optional value of the given header.
    ///
    /// The lookup is case-insensitive, matching HTTP header semantics.
    ///
    /// # Arguments
    /// * `name` - Header name to look up.
    pub fn get_header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Returns the TCP address of the remote peer, when known.
    ///
    /// `None` means the transport does not expose a peer address, such as the
    /// desktop Tauri IPC context; such requests are local by construction.
    pub fn remote_addr(&self) -> Option<std::net::SocketAddr> {
        self.remote_addr
    }

    /// Returns the value of a request header, when present.
    ///
    /// # Arguments
    /// * `name` - Header name, case-sensitive as captured from the transport.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|value| value.as_str())
    }

    /// Returns the HTTP method of the incoming request, when known.
    pub fn method(&self) -> Option<&str> {
        self.method.as_deref()
    }
}

/// Input of a header-aware JSON command.
///
/// Carries the deserialized payload alongside the incoming request context
/// so commands can implement conditional validation.
pub struct ControlerJsonInput {
    /// Deserialized command payload (JSON body or URL query string).
    pub payload: ControlerFunctionInput,
    /// Context of the incoming request (received HTTP headers).
    pub context: RequestControlerContext,
}

impl ControlerJsonInput {
    /// Returns the `If-None-Match` request header value, when present.
    pub fn if_none_match(&self) -> Option<&str> {
        self.context.get_header(HEADER_IF_NONE_MATCH)
    }
}

/// Type-erased future resolving to a header-aware JSON output or error string.
pub type JsonControlerFuture =
    Pin<Box<dyn Future<Output = Result<ControlerJsonOutput, String>> + Send + 'static>>;

/// Type-erased async callback stored by controller backends for header-aware
/// JSON commands.
pub type JsonControlerFunction =
    Arc<dyn Fn(ControlerJsonInput) -> JsonControlerFuture + Send + Sync + 'static>;

/// Raw stream payload received by controller backends.
///
/// This struct represents the complete HTTP request information for stream
/// endpoints that handle binary data.
pub struct ControlerStreamInput {
    /// Relative path mounted under the registered stream command.
    ///
    /// This field contains the path portion of the URL after the command prefix.
    pub path: String,
    /// Raw URL query string when present.
    ///
    /// This field contains the query parameters from the URL.
    #[allow(dead_code)]
    pub query: String,
    /// HTTP method of the incoming request.
    ///
    /// This field contains the HTTP method (GET, POST, etc.).
    #[allow(dead_code)]
    pub method: String,
    /// HTTP headers of the incoming request.
    ///
    /// This field contains all HTTP headers as key-value pairs.
    #[allow(dead_code)]
    pub headers: HashMap<String, String>,
    /// Raw request body bytes.
    ///
    /// This field contains the complete request body as binary data.
    pub body: Vec<u8>,
    /// Entry-point URL prefix that includes the server base, root mount,
    /// API prefix, and command name (e.g. `http://127.0.0.1:8080/api/proxy`).
    ///
    /// This field can be used to construct absolute URLs for responses.
    pub entry_point: String,
    /// When `true`, stream handlers should buffer the full response body
    /// instead of returning a streaming response. This is set by controller
    /// backends that do not support native streaming (e.g. Tauri).
    ///
    /// Handlers that produce `ResponseBody::Streamed` may check this flag
    /// and force body buffering to avoid streaming-specific issues such as
    /// strict chunked encoding parsing on the streaming path.
    #[allow(dead_code)]
    pub force_buffer_response: bool,
}

/// Polymorphic response body that can be either fully buffered or streamed.
pub enum ResponseBody {
    /// Entire body loaded in memory (used when post-actions need body modification).
    Buffered(Vec<u8>),
    /// Streaming body transferred as continuous chunks.
    Streamed(Pin<Box<dyn futures::Stream<Item = Result<Bytes, std::io::Error>> + Send>>),
}

/// Raw stream payload returned by controller backends.
///
/// This struct represents the complete HTTP response for stream endpoints.
pub struct ControlerStreamOutput {
    /// HTTP status code to return.
    ///
    /// This field contains the numeric HTTP status code (e.g., 200, 404, 500).
    pub status: u16,
    /// Stream response body.
    ///
    /// This field contains the binary response data to send to the client.
    pub body: ResponseBody,
    /// MIME type returned to the caller.
    ///
    /// This field specifies the content type of the response.
    pub content_type: String,
    /// Additional response headers.
    ///
    /// This field contains any additional HTTP headers to include in the response.
    pub headers: HashMap<String, String>,
}

/// Type-erased async callback stored by controller backends for binary stream routes.
///
/// This type represents a boxed future that resolves to a stream output or error string.
pub type StreamControlerFuture =
    Pin<Box<dyn Future<Output = Result<ControlerStreamOutput, String>> + Send + 'static>>;

/// Type-erased binary stream callback stored by controller backends.
///
/// This type represents a function that takes stream input and returns
/// a stream future.
pub type StreamControlerFunction =
    Arc<dyn Fn(ControlerStreamInput) -> StreamControlerFuture + Send + Sync + 'static>;
