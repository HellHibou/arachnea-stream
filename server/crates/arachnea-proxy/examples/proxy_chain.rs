use std::error::Error;
use std::future::poll_fn;

use arachnea_proxy::connectors::ArachneaTowerService;
use arachnea_proxy::core::{ArachneaProxyCore, ProxyNode, TimeoutConfig};
use http::Uri;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower_service::Service;

// cargo run -p arachnea-proxy --example proxy_chain -- http://127.0.0.1:8080 socks5://127.0.0.1:9050 example.com /
/// Runs an HTTP GET through a generated chain with an extra proxy hop.
///
/// # Errors
///
/// Returns an error when a proxy URL is invalid, the proxy core cannot be
/// created, the URI is invalid, the connector cannot open a stream, or the HTTP
/// exchange fails.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let first_proxy_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "socks5://72.49.49.11:31034".to_owned());
    let second_proxy_url = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "socks5://125.141.133.49:5566".to_owned());

    let (host, path) = target_from_args("example.com");
    let uri: Uri = format!("http://{host}{path}").parse()?;

    let timeouts = TimeoutConfig {
        connect_ms: 20_000,
        handshake_ms: 30_000,
        ..TimeoutConfig::default()
    };
    let mut core = ArachneaProxyCore::new(
        ProxyNode::from_url("first-hop", first_proxy_url)?
            .config()
            .with_timeouts(timeouts),
    )?;
    core.append_proxy(ProxyNode::from_url("second-hop", second_proxy_url)?)?;

    let mut service = ArachneaTowerService::new(core);

    poll_fn(|cx| service.poll_ready(cx)).await?;
    let mut stream = service.call(uri).await?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    print_response_head(&response);
    Ok(())
}

/// Reads the target host and path from command-line arguments.
///
/// # Parameters
///
/// - `default_host`: Hostname used when no command-line host is supplied.
///
/// # Returns
///
/// Tuple containing the host or authority and an absolute request path.
fn target_from_args(default_host: &str) -> (String, String) {
    let host = std::env::args()
        .nth(3)
        .unwrap_or_else(|| default_host.to_owned());
    let path = std::env::args().nth(4).unwrap_or_else(|| "/".to_owned());
    let path = if path.starts_with('/') {
        path
    } else {
        format!("/{path}")
    };
    (host, path)
}

/// Prints the response status line and first header bytes.
///
/// # Parameters
///
/// - `response`: Raw HTTP response bytes returned by the peer.
fn print_response_head(response: &[u8]) {
    let end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map_or(response.len(), |index| index + 4)
        .min(512);
    println!("{}", String::from_utf8_lossy(&response[..end]));
}
