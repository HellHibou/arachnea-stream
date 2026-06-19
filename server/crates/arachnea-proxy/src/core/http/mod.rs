//! Optional HTTP convenience helpers for the proxy core.

pub mod client;
#[cfg(feature = "controller-service")]
pub mod proxy_service;

pub use client::{ProxiedHttpRequest, ProxiedHttpResponse, SimpleHttpClient, SimpleHttpResponse};
#[cfg(feature = "controller-service")]
pub use proxy_service::handle_proxy_http;

use crate::core::ProxyStream;

/// Request target form expected by the next HTTP peer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttpRequestTargetForm {
    /// Send an origin-form target such as `/path?query` to the final server.
    OriginForm,
    /// Send an absolute-form target such as `http://example.com/path` to an HTTP proxy.
    AbsoluteForm,
}

/// Stream opened for relaying one explicit HTTP proxy request.
pub struct HttpRequestStream {
    /// Connected stream to either the final server or the selected HTTP upstream proxy.
    pub stream: ProxyStream,
    /// Request target form expected on `stream`.
    pub target_form: HttpRequestTargetForm,
    /// Optional `Proxy-Authorization` header value for the selected upstream proxy.
    pub proxy_authorization: Option<String>,
}
