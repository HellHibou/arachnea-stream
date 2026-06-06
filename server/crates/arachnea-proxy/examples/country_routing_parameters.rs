use std::error::Error;

use arachnea_proxy::connectors::ArachneaRquestLoopback;
use arachnea_proxy::core::PROXY_HEADER_PARAMETER_COUNTRY;
use arachnea_proxy::core::{ArachneaProxyCore, CountryRoutingProxyHandler, ProxyConfig, ProxyNode};

// Proxy HTTP -> Serveur HTTP
/*
const DEFAULT_COUNTRY_PROXY_URL: &str = "http://40.89.145.14:80";
const DEFAULT_GEOLOCATION_URL: &str = "http://ipwhois.app/json";
// */
// Proxy HTTP -> Serveur HTTPS
/*
const DEFAULT_COUNTRY_PROXY_URL: &str = "http://37.187.109.70:10111";
const DEFAULT_GEOLOCATION_URL: &str = "https://free.freeipapi.com/api/json/";
// */

// Proxy socks5 -> Serveur HTTP/HTTPS
//*
const DEFAULT_COUNTRY_PROXY_URL: &str = "socks5://37.220.83.249:1080";
const DEFAULT_GEOLOCATION_URL: &str = "https://free.freeipapi.com/api/json/";
// */
// cargo run -p arachnea-proxy --features rquest --example country_routing_parameters -- http://ipwhois.app/json/31.12.75.226 FR
/// Fetches an HTML page through the rquest loopback helper with a country parameter.
///
/// Set `ARACHNEA_COUNTRY_PROXY_URL` to override the default country HTTP proxy
/// `http://31.12.75.226:80`. The parameter is sent to the loopback proxy
/// through proxy-level HTTP headers so it is visible even for HTTPS `CONNECT`
/// requests.
///
/// The default geolocation URL queries the proxy IP explicitly because some
/// "current IP" APIs honor forwarded client IP headers and report the original
/// client country instead of the proxy endpoint.
///
/// # Errors
///
/// Returns an error when the core, loopback helper, rquest client, proxy URL,
/// header, or request fails.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_GEOLOCATION_URL.to_owned());

    let country = std::env::args().nth(2).unwrap_or_else(|| "FR".to_owned());
    let proxy_url = std::env::var("ARACHNEA_COUNTRY_PROXY_URL");
    let proxy = match proxy_url {
        Ok(url) => ProxyNode::from_url("country-egress", url)?,
        Err(_) => ProxyNode::from_url("country-egress", DEFAULT_COUNTRY_PROXY_URL)?,
    };

    let country_handler =
        CountryRoutingProxyHandler::new().with_proxy_for_country(country.clone(), proxy)?;
    let core =
        ArachneaProxyCore::new(ProxyConfig::default())?.with_parameter_handler(country_handler);
    let loopback = ArachneaRquestLoopback::start(core).await?;
    let client =
        loopback.client_with_parameters([(PROXY_HEADER_PARAMETER_COUNTRY, country.as_str())])?;
    let response = client.get(&url).send().await?;
    let status = response.status();
    let body = response.text().await?;

    println!("Loopback proxy: {}", loopback.proxy_url());
    println!("GET {url} with {PROXY_HEADER_PARAMETER_COUNTRY}={country} -> {status}");
    println!("{body}");
    Ok(())
}
