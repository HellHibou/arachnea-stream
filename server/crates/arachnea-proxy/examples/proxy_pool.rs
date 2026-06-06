use std::error::Error;

use arachnea_proxy::core::{
    ApplicationProtocol, ArachneaProxyCore, ConnectRequest, Destination, EgressPool,
    HttpRequestTargetForm, ProxyNode,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// cargo run -p arachnea-proxy --example proxy_pool -- http://127.0.0.1:8080,socks5://127.0.0.1:9050 example.com /
/// Runs an HTTP GET through the first working member of a proxy pool.
///
/// The first connection tests all configured pool members in parallel, marks
/// them as `ok` or `ko`, then caches the first working proxy in declaration
/// order for later requests made by the same core instance.
///
/// # Errors
///
/// Returns an error when a proxy URL is invalid, the proxy core cannot be
/// created, the destination cannot be reached through the selected proxy, or
/// the HTTP exchange fails.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let proxy_urls = std::env::args().nth(1).unwrap_or_else(|| {
        "http://37.187.109.70:10111,http://40.89.145.14:80,socks5://37.220.83.249:1080".to_owned()
    });

    let (host, path) = target_from_args("example.com");
    let pool_name = "example-pool";
    let chain_name = "pooled-egress";
    let proxy_nodes = proxy_nodes_from_csv(&proxy_urls)?;

    let core =
        ArachneaProxyCore::new(EgressPool::new(pool_name, proxy_nodes, None).config(chain_name))?;

    let destination =
        Destination::host_port(host.clone(), 80).with_protocol(ApplicationProtocol::Http);
    let mut outbound = core
        .connect_http_request(ConnectRequest::new(destination))
        .await?;
    let request_target = match outbound.target_form {
        HttpRequestTargetForm::AbsoluteForm => format!("http://{host}{path}"),
        HttpRequestTargetForm::OriginForm => path,
    };
    let mut request =
        format!("GET {request_target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    if let Some(proxy_authorization) = outbound.proxy_authorization {
        request.push_str("Proxy-Authorization: ");
        request.push_str(&proxy_authorization);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    outbound.stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::new();
    outbound.stream.read_to_end(&mut response).await?;
    print_pool_statuses(&core, pool_name)?;
    print_response_head(&response);
    Ok(())
}

/// Creates proxy nodes from a comma-separated proxy URL list.
///
/// # Parameters
///
/// - `proxy_urls`: Comma-separated proxy URLs.
///
/// # Returns
///
/// Proxy nodes named `pool-1`, `pool-2`, and so on.
///
/// # Errors
///
/// Returns an error when the list is empty or one URL is invalid.
fn proxy_nodes_from_csv(proxy_urls: &str) -> Result<Vec<ProxyNode>, Box<dyn Error>> {
    let nodes = proxy_urls
        .split(',')
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .enumerate()
        .map(|(index, url)| ProxyNode::from_url(format!("pool-{}", index + 1), url))
        .collect::<Result<Vec<_>, _>>()?;

    if nodes.is_empty() {
        return Err("at least one proxy URL is required".into());
    }
    Ok(nodes)
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
        .nth(2)
        .unwrap_or_else(|| default_host.to_owned());
    let path = std::env::args().nth(3).unwrap_or_else(|| "/".to_owned());
    let path = if path.starts_with('/') {
        path
    } else {
        format!("/{path}")
    };
    (host, path)
}

/// Prints runtime status for every member of a proxy pool.
///
/// # Parameters
///
/// - `core`: Proxy core that owns the runtime pool state.
/// - `pool_name`: Pool name to inspect.
///
/// # Errors
///
/// Returns an error when the pool is not declared.
fn print_pool_statuses(core: &ArachneaProxyCore, pool_name: &str) -> Result<(), Box<dyn Error>> {
    println!("Proxy pool status:");
    for state in core.proxy_pool_statuses(pool_name)? {
        let selected = if state.selected { " selected" } else { "" };
        println!("- {}: {:?}{selected}", state.upstream, state.status);
    }
    Ok(())
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
