use anyhow::Result;
use arachnea_dns::core::{ArachneaDnsCore, QueryRequest, RecordType, Upstream, UsageProfile};

// cargo run -p arachnea-dns --features doh --example privacy -- www.google.lu
/// Runs the privacy DNS profile example.
///
/// # Errors
///
/// Returns an error when the DNS core cannot be created or resolution fails.
#[tokio::main]
async fn main() -> Result<()> {
    // Create a new DNS core: privacy mode using DoH cloudflare-dns
    let config = UsageProfile::Privacy(vec![Upstream::doh(
        "cloudflare-doh",
        "https://cloudflare-dns.com/dns-query",
        None,
    )])
    .config();

    let core = ArachneaDnsCore::new(config)?;

    // Get arguments from command line
    let domain = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "www.google.lu".to_owned());

    // Resolve IP addresses
    println!("Resolve IP {domain} using privacy mode using DoH :");
    for ip in core.resolve_ip(&domain).await? {
        println!(" - {ip}");
    }
    println!("");

    // Resolve A records
    println!("Resolve A {domain} using privacy mode using DoH :");
    let answer = core
        .resolve(QueryRequest::new(&domain, RecordType::A))
        .await?;
    for record in answer.records {
        println!(" - {record:?}");
    }

    Ok(())
}
