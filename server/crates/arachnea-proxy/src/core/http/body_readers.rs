//! Streaming HTTP body decoders.
//!
//! These wrappers implement `AsyncRead` for the three HTTP/1.1 body transfer
//! modes: Content-Length, Transfer-Encoding: chunked, and connection close.

use std::io;
use std::pin::Pin;
use std::task::{ready, Context, Poll};

use tokio::io::{AsyncRead, ReadBuf};

/// Reads a fixed number of bytes from the inner stream, then returns EOF.
pub struct ContentLengthBodyReader<S> {
    stream: S,
    remaining: usize,
}

impl<S> ContentLengthBodyReader<S> {
    /// Wraps a stream, limiting reads to at most `content_length` bytes.
    pub fn new(stream: S, content_length: usize) -> Self {
        Self {
            stream,
            remaining: content_length,
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for ContentLengthBodyReader<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.remaining == 0 {
            return Poll::Ready(Ok(()));
        }
        let max = buf.remaining().min(self.remaining);
        let mut local = vec![0u8; max];
        let mut local_buf = ReadBuf::new(&mut local);
        ready!(Pin::new(&mut self.stream).poll_read(cx, &mut local_buf))?;
        let read = local_buf.filled().len();
        buf.put_slice(&local[..read]);
        self.remaining -= read;
        Poll::Ready(Ok(()))
    }
}

/// Reads until the underlying stream closes (EOF).
pub struct UntilEofBodyReader<S> {
    stream: S,
}

impl<S> UntilEofBodyReader<S> {
    /// Wraps a stream, reading until EOF.
    pub fn new(stream: S) -> Self {
        Self { stream }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for UntilEofBodyReader<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

/// Decodes a chunked transfer-encoded body from the inner stream.
///
/// Reads chunk size lines, chunk data, and trailing CRLF pairs,
/// exposing only the decoded body data through `AsyncRead`.
pub struct ChunkedBodyReader<R> {
    reader: R,
    state: ChunkedState,
    /// Buffer for bytes read from the inner stream but not yet processed.
    read_buf: Vec<u8>,
    /// Buffer for decoded chunk data ready to serve to the caller.
    decoded: Vec<u8>,
    /// Read position within `decoded`.
    decoded_pos: usize,
}

enum ChunkedState {
    /// Reading the hex chunk size line (until \r\n).
    ChunkSize,
    /// Reading chunk data bytes.
    ChunkData(usize),
    /// Reading the trailing \r\n after chunk data.
    ChunkCrLf,
    /// Final chunk (size 0) consumed, no more data.
    Done,
}

impl<R> ChunkedBodyReader<R> {
    /// Wraps a stream, decoding chunked transfer encoding on reads.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            state: ChunkedState::ChunkSize,
            read_buf: Vec::new(),
            decoded: Vec::new(),
            decoded_pos: 0,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for ChunkedBodyReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            // Serve decoded data first
            if self.decoded_pos < self.decoded.len() {
                let avail = &self.decoded[self.decoded_pos..];
                let to_copy = avail.len().min(buf.remaining());
                buf.put_slice(&avail[..to_copy]);
                self.decoded_pos += to_copy;
                if self.decoded_pos == self.decoded.len() {
                    self.decoded.clear();
                    self.decoded_pos = 0;
                }
                return Poll::Ready(Ok(()));
            }

            match self.state {
                ChunkedState::Done => return Poll::Ready(Ok(())),
                ChunkedState::ChunkSize => {
                    if !self.try_read_chunk_size(cx)? {
                        return Poll::Pending;
                    }
                }
                ChunkedState::ChunkData(remaining) => {
                    if !self.try_read_chunk_data(cx, remaining)? {
                        return Poll::Pending;
                    }
                }
                ChunkedState::ChunkCrLf => {
                    if !self.try_read_chunk_crlf(cx)? {
                        return Poll::Pending;
                    }
                }
            }
        }
    }
}

impl<R: AsyncRead + Unpin> ChunkedBodyReader<R> {
    fn read_more(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let start = self.read_buf.len();
        self.read_buf.resize(start + 4096, 0);
        let mut sub = ReadBuf::new(&mut self.read_buf[start..]);
        ready!(Pin::new(&mut self.reader).poll_read(cx, &mut sub))?;
        let read = sub.filled().len();
        self.read_buf.truncate(start + read);
        Poll::Ready(Ok(()))
    }

    fn try_read_chunk_size(&mut self, cx: &mut Context<'_>) -> Result<bool, io::Error> {
        loop {
            if let Some(end) = self
                .read_buf
                .windows(2)
                .position(|w| w == b"\r\n")
            {
                let size_line = &self.read_buf[..end];
                let size_str = std::str::from_utf8(size_line)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "chunk size is not utf-8"))?;
                let size_hex = size_str.split(';').next().unwrap_or("").trim();
                let chunk_size = usize::from_str_radix(size_hex, 16)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size"))?;

                self.read_buf.drain(..end + 2);

                if chunk_size == 0 {
                    self.state = ChunkedState::Done;
                    return Ok(true);
                }

                self.state = ChunkedState::ChunkData(chunk_size);
                return Ok(true);
            }

            if self.read_more(cx)?.is_pending() {
                return Ok(false);
            }
        }
    }

    fn try_read_chunk_data(
        &mut self,
        cx: &mut Context<'_>,
        remaining: usize,
    ) -> Result<bool, io::Error> {
        loop {
            let avail = self.read_buf.len().min(remaining);
            if avail > 0 {
                self.decoded.extend_from_slice(&self.read_buf.drain(..avail).as_slice());
                let new_remaining = remaining - avail;
                if new_remaining == 0 {
                    self.state = ChunkedState::ChunkCrLf;
                } else {
                    self.state = ChunkedState::ChunkData(new_remaining);
                }
                return Ok(true);
            }

            if self.read_more(cx)?.is_pending() {
                return Ok(false);
            }
        }
    }

    fn try_read_chunk_crlf(&mut self, cx: &mut Context<'_>) -> Result<bool, io::Error> {
        loop {
            if self.read_buf.len() >= 2 {
                if self.read_buf[0] != b'\r' || self.read_buf[1] != b'\n' {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "expected CRLF after chunk data",
                    ));
                }
                self.read_buf.drain(..2);
                self.state = ChunkedState::ChunkSize;
                return Ok(true);
            }

            if self.read_more(cx)?.is_pending() {
                return Ok(false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn content_length_reads_exact_bytes() {
        let data = b"hello world";
        let reader = ContentLengthBodyReader::new(&data[..], 5);
        let mut buf = Vec::new();
        tokio::pin!(reader);
        reader.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"hello");
    }

    #[tokio::test]
    async fn content_length_zero_returns_empty() {
        let data = b"hello";
        let reader = ContentLengthBodyReader::new(&data[..], 0);
        let mut buf = Vec::new();
        tokio::pin!(reader);
        reader.read_to_end(&mut buf).await.unwrap();
        assert!(buf.is_empty());
    }

    #[tokio::test]
    async fn content_length_stops_at_content_length() {
        let data = b"abcdefghij";
        let reader = ContentLengthBodyReader::new(&data[..], 10);
        let mut buf = Vec::new();
        tokio::pin!(reader);
        reader.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"abcdefghij");
    }

    #[tokio::test]
    async fn until_eof_reads_all() {
        let data = b"hello world";
        let reader = UntilEofBodyReader::new(&data[..]);
        let mut buf = Vec::new();
        tokio::pin!(reader);
        reader.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"hello world");
    }

    #[tokio::test]
    async fn until_eof_empty() {
        let data: &[u8] = b"";
        let reader = UntilEofBodyReader::new(data);
        let mut buf = Vec::new();
        tokio::pin!(reader);
        reader.read_to_end(&mut buf).await.unwrap();
        assert!(buf.is_empty());
    }

    #[tokio::test]
    async fn chunked_single_chunk() {
        let raw = b"5\r\nhello\r\n0\r\n\r\n";
        let reader = ChunkedBodyReader::new(&raw[..]);
        let mut buf = Vec::new();
        tokio::pin!(reader);
        reader.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"hello");
    }

    #[tokio::test]
    async fn chunked_multiple_chunks() {
        let raw = b"6\r\nhello \r\n5\r\nworld\r\n0\r\n\r\n";
        let reader = ChunkedBodyReader::new(&raw[..]);
        let mut buf = Vec::new();
        tokio::pin!(reader);
        reader.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"hello world");
    }

    #[tokio::test]
    async fn chunked_empty_body() {
        let raw = b"0\r\n\r\n";
        let reader = ChunkedBodyReader::new(&raw[..]);
        let mut buf = Vec::new();
        tokio::pin!(reader);
        reader.read_to_end(&mut buf).await.unwrap();
        assert!(buf.is_empty());
    }

    #[tokio::test]
    async fn chunked_with_extensions() {
        let raw = b"5;ext=1\r\nhello\r\n0\r\n\r\n";
        let reader = ChunkedBodyReader::new(&raw[..]);
        let mut buf = Vec::new();
        tokio::pin!(reader);
        reader.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"hello");
    }

    #[tokio::test]
    async fn chunked_large_chunk() {
        let chunk_data = vec![b'a'; 8192];
        let size_hex = format!("{:x}", chunk_data.len());
        let mut raw = format!("{}\r\n", size_hex).into_bytes();
        raw.extend_from_slice(&chunk_data);
        raw.extend_from_slice(b"\r\n0\r\n\r\n");
        let reader = ChunkedBodyReader::new(&raw[..]);
        let mut buf = Vec::new();
        tokio::pin!(reader);
        reader.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, chunk_data);
    }
}
