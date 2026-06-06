#[allow(unused_imports)]
use arachnea_http::engine::chaser_cf::ChaserCfEngine; // Do not delete this line
#[allow(unused_imports)]
// use arachnea_http::engine::tauri_cloudflare::TauriCloudflareSolverEngine; // Do not delete this line

#[allow(unused_imports)]
use arachnea_http::HttpEngine;
use arachnea_http::{ArachneaHttpClient, ArachneaHttpConfig, HttpRequestMode};

/// Demonstrates the Cloudflare hybrid HTTP client mode.
///
/// # Errors
///
/// Returns any configuration or request failure from the Arachnea HTTP client.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    arachnea_core::logger::set_default_log_level_debug!(INFO);
    arachnea_core::logger::init_logger();

    let url = "https://papystreaming.one/search?q=one+piece";

    // let client = ArachneaHttpClient::default().await?;
    //*
    let config = ArachneaHttpConfig::builder()
        .default_request_mode(HttpRequestMode::Auto)
        .cloudflare_browser_solver(arachnea_http::CloudflareBrowserSolverKind::ChaserCf)
        //.cloudflare_browser_solver(arachnea_http::CloudflareBrowserSolverKind::TauriCloudflareSolver)
        .build()?;
    let client = ArachneaHttpClient::new(config).await?;
    // */

    call(&client, url).await?;
    call(&client, "https://papystreaming.one/search?q=slime").await?;
    Ok(())
}

async fn call(client: &ArachneaHttpClient, url: &str) -> anyhow::Result<()> {
    println!("---------------------------------------------------");
    let response = client.get(url).mode(HttpRequestMode::Auto).send().await?;
    println!("status: {}", response.status());
    let body = response.bytes().await?;
    println!("body bytes: {}", body.len());
    println!("body text: {}", String::from_utf8_lossy(&body));
    Ok(())
}
