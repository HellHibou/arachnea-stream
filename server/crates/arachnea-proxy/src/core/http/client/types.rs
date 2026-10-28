//! Public request and response contracts for the proxy HTTP convenience client.

use crate::core::ClientContext;
use crate::http::actions::{PostActionContext, ProxyHttpPostActionConfig};
use bytes::Bytes;
use std::{collections::HashMap, pin::Pin};

/// Minimal HTTP response returned by the core convenience client.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimpleHttpResponse {
    /// Numeric HTTP status code.
    pub status: u16,
    /// Raw response bytes including headers and body.
    pub bytes: Vec<u8>,
}

/// Response body returned by the proxied HTTP client.
pub enum ProxiedResponseBody {
    /// Entire body loaded in memory, required for body-transforming post-actions.
    Buffered(Vec<u8>),
    /// Body decoded incrementally from the upstream connection.
    Streamed(Pin<Box<dyn futures::Stream<Item = std::io::Result<Bytes>> + Send>>),
}

/// Full proxied HTTP response with parsed headers and body.
pub struct ProxiedHttpResponse {
    /// Original response decision, computed before post-actions.
    pub response_decision: crate::core::http::response_classifier::ProxyResponseDecision,
    /// Exact route used by the final response, including after redirects.
    pub selection_token: Option<crate::core::ProxySelectionToken>,
    /// Numeric HTTP status code.
    pub status: u16,
    /// Response headers.
    pub headers: HashMap<String, String>,
    /// Response body, buffered only when transformations require it.
    pub body: ProxiedResponseBody,
}

/// Input for a proxied HTTP request through the Arachnea proxy core.
#[derive(Clone, Debug)]
pub struct ProxiedHttpRequest {
    /// Code-injected policy, taking precedence over serialized policies.
    pub response_classifier: Option<crate::core::http::response_classifier::ProxyResponsePolicy>,
    /// Absolute target URL (http or https scheme only).
    pub url: String,
    /// HTTP method (GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS).
    pub method: String,
    /// Request headers.
    pub headers: HashMap<String, String>,
    /// Request cookies to send as Cookie header.
    pub cookies: HashMap<String, String>,
    /// Request body bytes.
    pub body: Vec<u8>,
    /// Request-local proxy context used by routing parameter handlers.
    pub client_context: ClientContext,
    /// Post-response actions to apply to the response.
    pub post_actions: Vec<ProxyHttpPostActionConfig>,
    /// Whether the caller requires a fully buffered response body.
    ///
    /// This must be enabled when `post_actions` is non-empty. Buffered response
    /// acceptance is committed only after the complete body has been received.
    pub buffer_response_body: bool,
    /// Whether only response headers should be read (e.g. HEAD requests).
    pub headers_only: bool,
    /// Request context for action variable substitution.
    pub context: PostActionContext,
    /// Whether the target TLS certificate must be validated.
    pub verify_tls: bool,
    /// Optional maximum number of upstream redirects to follow before returning
    /// the final allowed 3xx response.
    pub follow_redirects: Option<usize>,
}
