use anyhow::Result;
use arachnea_dns::core::{ArachneaDnsCore, QueryRequest, RecordType, Upstream, UsageProfile};

// cargo run -p arachnea-dns --features doq --example doq -- www.google.lu
/// Runs the DNS-over-QUIC example.
///
/// # Errors
///
/// Returns an error when the DNS core cannot be created or resolution fails.
#[tokio::main]
async fn main() -> Result<()> {
    let config = UsageProfile::SingleForwarder(Upstream::doq(
        "adguard-doq",
        "unfiltered.adguard-dns.com",
        853,
        Some(10_000),
    ))
    .config();

    let core = ArachneaDnsCore::new(config)?;

    // Get arguments from command line
    let domain = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "www.google.lu".to_owned());

    // Resolve IP addresses
    println!("Resolve IP {domain} using DoQ via Hickory :");
    for ip in core.resolve_ip(&domain).await? {
        println!(" - {ip}");
    }
    println!("");

    // Resolve A records
    println!("Resolve A {domain} using DoQ via Hickory :");
    let answer = core
        .resolve(QueryRequest::new(&domain, RecordType::A))
        .await?;
    for record in answer.records {
        println!(" - {record:?}");
    }

    Ok(())
}
