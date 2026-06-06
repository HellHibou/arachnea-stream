use std::error::Error;

use arachnea_proxy::connectors::ArachneaRquestLoopback;
use arachnea_proxy::core::{ArachneaProxyCore, ProxyConfig, ProxyNode, SmartDnsProxyHandler};

const SMART_DNS_PARAMETER: &str = "smart_dns";

// cargo run -p arachnea-proxy --features rquest --example smart_dns_parameters -- http://example.com/ streaming-us
/// Fetches an HTML page through the rquest loopback helper with a Smart DNS hint.
///
/// Set `ARACHNEA_SMART_DNS_PROXY_URL` to a proxy URL such as
/// `https://127.0.0.1:8443` when the selected hint should append a real
/// request-local egress proxy. The parameter is sent to the loopback proxy
/// through HTTP proxy credentials so it is visible even for HTTPS `CONNECT`
/// requests.
///
/// # Errors
///
/// Returns an error when the core, loopback helper, rquest client, proxy URL,
/// header, or request fails.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://example.com/".to_owned());
    let hint = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "streaming-us".to_owned());

    let smart_dns_handler = smart_dns_handler(&hint)?;
    let core =
        ArachneaProxyCore::new(ProxyConfig::default())?.with_parameter_handler(smart_dns_handler);
    let loopback = ArachneaRquestLoopback::start(core).await?;
    let client = loopback.client_with_parameters([(SMART_DNS_PARAMETER, hint.as_str())])?;
    let response = client.get(&url).send().await?;
    let status = response.status();
    let body = response.text().await?;

    println!("Loopback proxy: {}", loopback.proxy_url());
    println!("GET {url} with {SMART_DNS_PARAMETER}={hint} -> {status}");
    println!("{body}");
    Ok(())
}

/// Builds the Smart DNS parameter handler used by this example.
///
/// # Parameters
///
/// - `hint`: Smart DNS hint that should match the optional configured egress.
///
/// # Returns
///
/// Smart DNS handler.
///
/// # Errors
///
/// Returns an error when `ARACHNEA_SMART_DNS_PROXY_URL` is set but cannot be
/// converted to a proxy node.
fn smart_dns_handler(hint: &str) -> arachnea_proxy::core::Result<SmartDnsProxyHandler> {
    let mut handler = SmartDnsProxyHandler::new();
    if let Ok(proxy_url) = std::env::var("ARACHNEA_SMART_DNS_PROXY_URL") {
        let proxy = ProxyNode::from_url("smart-dns-egress", proxy_url)?;
        handler = handler.with_proxy_for_hint(hint, proxy)?;
    }
    Ok(handler)
}
