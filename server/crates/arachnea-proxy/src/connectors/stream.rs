use std::pin::Pin;
use std::task::{Context, Poll};

use crate::core::ProxyStream;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Stream returned by connector services.
pub struct ConnectorStream {
    inner: ProxyStream,
}

impl ConnectorStream {
    /// Wraps a proxy stream.
    ///
    /// # Parameters
    ///
    /// - `inner`: Proxy stream returned by the core.
    ///
    /// # Returns
    ///
    /// Connector stream wrapper.
    pub fn new(inner: ProxyStream) -> Self {
        Self { inner }
    }

    /// Returns the wrapped proxy stream.
    pub fn into_inner(self) -> ProxyStream {
        self.inner
    }
}

impl AsyncRead for ConnectorStream {
    /// Polls the wrapped proxy stream for readable bytes.
    ///
    /// # Parameters
    ///
    /// - `cx`: Task context used for wakeups.
    /// - `buf`: Read buffer that receives bytes.
    ///
    /// # Returns
    ///
    /// Poll state for the read operation.
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for ConnectorStream {
    /// Polls the wrapped proxy stream for writable capacity.
    ///
    /// # Parameters
    ///
    /// - `cx`: Task context used for wakeups.
    /// - `buf`: Bytes to write.
    ///
    /// # Returns
    ///
    /// Poll state containing the number of bytes written.
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    /// Polls the wrapped proxy stream for flush completion.
    ///
    /// # Parameters
    ///
    /// - `cx`: Task context used for wakeups.
    ///
    /// # Returns
    ///
    /// Poll state for the flush operation.
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    /// Polls the wrapped proxy stream for shutdown completion.
    ///
    /// # Parameters
    ///
    /// - `cx`: Task context used for wakeups.
    ///
    /// # Returns
    ///
    /// Poll state for the shutdown operation.
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
