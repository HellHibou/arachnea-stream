use super::*;
use tokio::io::{AsyncWriteExt, DuplexStream};

async fn response_stream(response: &[u8]) -> DuplexStream {
    let (mut writer, reader) = tokio::io::duplex(response.len().max(1));
    let response = response.to_vec();
    tokio::spawn(async move {
        writer.write_all(&response).await.unwrap();
        writer.shutdown().await.unwrap();
    });
    reader
}

#[tokio::test]
async fn reads_complete_headers_without_consuming_body() {
    let mut stream = response_stream(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\ntest").await;
    let headers = read_headers_only(&mut stream).await.unwrap();
    assert_eq!(headers, b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n");

    let headers = HashMap::from([("Content-Length".to_string(), "4".to_string())]);
    let body = read_buffered_response_body(stream, &headers, false, true)
        .await
        .unwrap();
    assert_eq!(body, b"test");

    let headers = HashMap::from([("Transfer-Encoding".to_string(), "chunked".to_string())]);
    for (wire_body, expected) in [
        (&b"4\r\ntest\r\n0\r\n\r\n"[..], Some(&b"test"[..])),
        (&b"0\r\n\r\n"[..], Some(&b""[..])),
        (&b"#EXTM3U\nplaylist"[..], Some(&b"#EXTM3U\nplaylist"[..])),
        (&b"4\r\nte"[..], None),
        (&b"4\r\ntest\r\n0\r\n"[..], None),
        (&b"4\r\ntest\r\ninvalid\r\n"[..], None),
    ] {
        let source = response_stream(wire_body).await;
        let result = read_buffered_response_body(source, &headers, true, true).await;
        match expected {
            Some(expected) => assert_eq!(result.unwrap(), expected),
            None => assert!(result.is_err()),
        }
    }

    // A complete chunked body must not read a subsequent TLS shutdown error.
    let source = tokio_util::io::StreamReader::new(futures::stream::iter([
        Ok(Bytes::from_static(b"4\r\ntest\r\n0\r\n\r\n")),
        Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "missing close_notify",
        )),
    ]));
    let body = read_buffered_response_body(source, &headers, true, true)
        .await
        .unwrap();
    assert_eq!(body, b"test");
}

#[tokio::test]
async fn rejects_truncated_headers() {
    let mut stream = response_stream(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n").await;
    let error = read_headers_only(&mut stream).await.unwrap_err();
    assert!(matches!(error, ProxyError::Protocol(_)));
}
