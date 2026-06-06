use std::error::Error;

use arachnea_proxy::connectors::ArachneaRquestLoopback;
use arachnea_proxy::core::{ArachneaProxyCore, UsageProfile};

// cargo run -p arachnea-proxy --features rquest --example rquest_loopback -- http://example.com/
/// Runs a rquest request through the loopback proxy helper.
///
/// # Errors
///
/// Returns an error when the proxy core cannot be created, the loopback proxy
/// cannot bind, the rquest client cannot be built, or the request fails.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://example.com/".to_owned());

    let core = ArachneaProxyCore::new(UsageProfile::SystemRelay.config())?;
    let loopback = ArachneaRquestLoopback::start(core).await?;
    let client = loopback.client()?;
    let response = client.get(&url).send().await?;
    let status = response.status();
    let body = response.text().await?;

    println!("Loopback proxy: {}", loopback.proxy_url());
    println!("GET {url} -> {status}");
    println!("{}", first_lines(&body, 8));
    Ok(())
}

/// Returns the first lines of a response body for compact console output.
///
/// # Parameters
///
/// - `body`: Response body text.
/// - `max_lines`: Maximum number of lines to return.
///
/// # Returns
///
/// Body prefix containing at most `max_lines` lines.
fn first_lines(body: &str, max_lines: usize) -> String {
    body.lines().take(max_lines).collect::<Vec<_>>().join("\n")
}
