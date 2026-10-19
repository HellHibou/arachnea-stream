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

    let mut body = Vec::new();
    stream.read_to_end(&mut body).await.unwrap();
    assert_eq!(body, b"test");
}

#[tokio::test]
async fn rejects_truncated_headers() {
    let mut stream = response_stream(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n").await;
    let error = read_headers_only(&mut stream).await.unwrap_err();
    assert!(matches!(error, ProxyError::Protocol(_)));
}
