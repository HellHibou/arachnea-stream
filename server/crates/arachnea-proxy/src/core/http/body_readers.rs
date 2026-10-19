//! Streaming HTTP body decoders.
//!
//! These wrappers implement `AsyncRead` for the three HTTP/1.1 body transfer
//! modes: Content-Length, Transfer-Encoding: chunked, and connection close.

use std::io;
use std::pin::Pin;
use std::task::{ready, Context, Poll};

use tokio::io::{AsyncRead, ReadBuf};

/// Decodes a complete HTTP/1.1 chunked body.
///
/// This is used when a buffered response needs to distinguish a valid chunked
/// body from a non-compliant proxy that has already removed the chunk framing.
pub(crate) fn decode_chunked_body(body: &[u8]) -> io::Result<Vec<u8>> {
    let mut offset = 0;
    let mut decoded = Vec::new();

    loop {
        let line_end = body[offset..]
            .windows(2)
            .position(|window| window == b"\r\n")
            .map(|index| offset + index)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size"))?;
        let size = std::str::from_utf8(&body[offset..line_end])
            .ok()
            .and_then(|line| line.split(';').next())
            .map(str::trim)
            .and_then(|value| usize::from_str_radix(value, 16).ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size"))?;
        offset = line_end + 2;

        if size == 0 {
            loop {
                let trailer_end = body[offset..]
                    .windows(2)
                    .position(|window| window == b"\r\n")
                    .map(|index| offset + index)
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "chunked response trailers ended prematurely",
                        )
                    })?;
                if trailer_end == offset {
                    return Ok(decoded);
                }
                offset = trailer_end + 2;
            }
        }

        let chunk_end = offset
            .checked_add(size)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size"))?;
        if chunk_end + 2 > body.len() || &body[chunk_end..chunk_end + 2] != b"\r\n" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid chunk data",
            ));
        }
        decoded.extend_from_slice(&body[offset..chunk_end]);
        offset = chunk_end + 2;
    }
}

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
        if read == 0 {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "response body ended before Content-Length was reached",
            )));
        }
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
    /// Reading optional trailer fields after the final chunk.
    Trailers,
    /// Final chunk and trailers consumed, no more data.
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
                ChunkedState::Trailers => {
                    if !self.try_read_trailers(cx)? {
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
        let poll_result = Pin::new(&mut self.reader).poll_read(cx, &mut sub)?;
        let read = sub.filled().len();
        // Always truncate back to the bytes actually filled. This must happen on
        // the Pending path too: if we only truncate after `ready!`, the zero-filled
        // grow performed above would be left behind and served as body data on the
        // next poll, corrupting and truncating the stream.
        self.read_buf.truncate(start + read);
        if poll_result.is_pending() {
            return Poll::Pending;
        }
        if read == 0 {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "chunked response body ended prematurely",
            )));
        }
        Poll::Ready(Ok(()))
    }

    fn try_read_chunk_size(&mut self, cx: &mut Context<'_>) -> Result<bool, io::Error> {
        loop {
            if let Some(end) = self.read_buf.windows(2).position(|w| w == b"\r\n") {
                let size_line = &self.read_buf[..end];
                let size_str = std::str::from_utf8(size_line).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "chunk size is not utf-8")
                })?;
                let size_hex = size_str.split(';').next().unwrap_or("").trim();
                let chunk_size = usize::from_str_radix(size_hex, 16).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size")
                })?;

                self.read_buf.drain(..end + 2);

                if chunk_size == 0 {
                    self.state = ChunkedState::Trailers;
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
                self.decoded
                    .extend_from_slice(&self.read_buf.drain(..avail).as_slice());
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

    fn try_read_trailers(&mut self, cx: &mut Context<'_>) -> Result<bool, io::Error> {
        loop {
            if let Some(end) = self
                .read_buf
                .windows(2)
                .position(|window| window == b"\r\n")
            {
                self.read_buf.drain(..end + 2);
                if end == 0 {
                    self.state = ChunkedState::Done;
                }
                return Ok(true);
            }

            if self.read_more(cx)?.is_pending() {
                return Ok(false);
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/body_readers_tests.rs"]
mod tests;
