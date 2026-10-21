use super::*;
use std::pin::pin;
use tokio::io::AsyncReadExt;

struct Piecewise {
    data: Vec<u8>,
    pos: usize,
    step: usize,
    pend_every: usize,
    calls: usize,
    already_pending: bool,
}
impl AsyncRead for Piecewise {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        this.calls += 1;
        if !this.already_pending && this.calls % this.pend_every == 0 {
            this.already_pending = true;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        this.already_pending = false;
        if this.pos >= this.data.len() {
            return Poll::Ready(Ok(()));
        }
        let n = (this.data.len() - this.pos)
            .min(this.step)
            .min(buf.remaining());
        buf.put_slice(&this.data[this.pos..this.pos + n]);
        this.pos += n;
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn chunked_fragmented_lossy() {
    let data: Vec<u8> = (0..120_000u32).map(|i| (i % 251) as u8).collect();
    let mut raw = Vec::new();
    for part in data.chunks(4093) {
        raw.extend_from_slice(format!("{:x}\r\n", part.len()).as_bytes());
        raw.extend_from_slice(part);
        raw.extend_from_slice(b"\r\n");
    }
    raw.extend_from_slice(b"0\r\n\r\n");
    let source = Piecewise {
        data: raw.clone(),
        pos: 0,
        step: 7,
        pend_every: 3,
        calls: 0,
        already_pending: false,
    };
    let reader = ChunkedBodyReader::new(source);
    let mut buf = Vec::new();
    let mut reader = pin!(reader);
    let r = reader.read_to_end(&mut buf).await;
    match &r {
        Ok(_) => eprintln!("OK decoded len={}", buf.len()),
        Err(e) => eprintln!("ERR: {e:?} decoded len={}", buf.len()),
    }
    assert_eq!(data.len(), buf.len());
    assert_eq!(data, buf);
}

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
async fn content_length_rejects_truncated_body() {
    let reader = ContentLengthBodyReader::new(&b"short"[..], 10);
    let mut buf = Vec::new();
    tokio::pin!(reader);
    let error = reader.read_to_end(&mut buf).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
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

#[tokio::test]
async fn chunked_rejects_truncated_body() {
    let reader = ChunkedBodyReader::new(&b"5\\r\\nhel"[..]);
    let mut buf = Vec::new();
    tokio::pin!(reader);
    let error = reader.read_to_end(&mut buf).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
}

#[tokio::test]
async fn chunked_rejects_truncated_trailers() {
    let reader = ChunkedBodyReader::new(&b"0\r\n"[..]);
    let mut buf = Vec::new();
    tokio::pin!(reader);
    let error = reader.read_to_end(&mut buf).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
}
