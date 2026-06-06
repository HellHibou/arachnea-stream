use std::pin::Pin;
use std::task::{Context, Poll};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::core::{ClientContext, Destination, TransportKind};

/// Internal trait object boundary for boxed proxy streams.
trait AsyncReadWrite: AsyncRead + AsyncWrite + Unpin {}

impl<T> AsyncReadWrite for T where T: AsyncRead + AsyncWrite + Unpin {}

/// Metadata describing an established proxy connection.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConnectMetadata {
    /// Selected route or chain name.
    pub route_name: Option<String>,
    /// Transport kind used for the final hop.
    pub transport_kind: Option<TransportKind>,
    /// Hop names used to establish the stream.
    pub hops: Vec<String>,
}

/// Request passed to the proxy core.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConnectRequest {
    /// Destination to connect to.
    pub destination: Destination,
    /// Optional caller or inbound-protocol context.
    pub client_context: ClientContext,
    /// Optional metadata supplied by an adapter.
    pub metadata: ConnectMetadata,
}

impl ConnectRequest {
    /// Creates a request for a destination with an empty client context.
    ///
    /// # Parameters
    ///
    /// - `destination`: Target destination for the request.
    ///
    /// # Returns
    ///
    /// Connect request with empty context and metadata.
    pub fn new(destination: Destination) -> Self {
        Self {
            destination,
            client_context: ClientContext::new(),
            metadata: ConnectMetadata::default(),
        }
    }

    /// Replaces the client context on this request.
    ///
    /// # Parameters
    ///
    /// - `client_context`: Context to attach to the request.
    ///
    /// # Returns
    ///
    /// Updated connect request.
    pub fn with_client_context(mut self, client_context: ClientContext) -> Self {
        self.client_context = client_context;
        self
    }
}

/// Boxed asynchronous stream returned by the proxy core.
pub struct ProxyStream {
    inner: Pin<Box<dyn AsyncReadWrite + Send + 'static>>,
    metadata: ConnectMetadata,
}

impl ProxyStream {
    /// Wraps an asynchronous stream and attaches metadata.
    ///
    /// # Parameters
    ///
    /// - `stream`: Async stream to box.
    /// - `metadata`: Metadata describing the connection.
    ///
    /// # Returns
    ///
    /// Boxed proxy stream.
    pub fn new<S>(stream: S, metadata: ConnectMetadata) -> Self
    where
        S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        Self {
            inner: Box::pin(stream),
            metadata,
        }
    }

    /// Returns immutable connection metadata.
    pub fn metadata(&self) -> &ConnectMetadata {
        &self.metadata
    }

    /// Returns mutable connection metadata for internal adapters.
    pub fn metadata_mut(&mut self) -> &mut ConnectMetadata {
        &mut self.metadata
    }
}

impl AsyncRead for ProxyStream {
    /// Polls the inner stream for readable bytes.
    ///
    /// # Parameters
    ///
    /// - `cx`: Task context used for wakeups.
    /// - `buf`: Read buffer that receives bytes from the stream.
    ///
    /// # Returns
    ///
    /// Poll state for the read operation.
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.inner.as_mut().poll_read(cx, buf)
    }
}

impl AsyncWrite for ProxyStream {
    /// Polls the inner stream for writable capacity.
    ///
    /// # Parameters
    ///
    /// - `cx`: Task context used for wakeups.
    /// - `buf`: Bytes to write to the stream.
    ///
    /// # Returns
    ///
    /// Poll state containing the number of bytes written.
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.inner.as_mut().poll_write(cx, buf)
    }

    /// Polls the inner stream for flush completion.
    ///
    /// # Parameters
    ///
    /// - `cx`: Task context used for wakeups.
    ///
    /// # Returns
    ///
    /// Poll state for the flush operation.
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.inner.as_mut().poll_flush(cx)
    }

    /// Polls the inner stream for shutdown completion.
    ///
    /// # Parameters
    ///
    /// - `cx`: Task context used for wakeups.
    ///
    /// # Returns
    ///
    /// Poll state for the shutdown operation.
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.inner.as_mut().poll_shutdown(cx)
    }
}
