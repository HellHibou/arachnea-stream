use anyhow::Result;
use arachnea_dns::core::{ArachneaDnsCore, QueryRequest, RecordType, Upstream, UsageProfile};

// cargo run -p arachnea-dns --features doh --example single_forwarder -- www.rtlplay.be
/// Runs the single-forwarder DNS example.
///
/// # Errors
///
/// Returns an error when the DNS core cannot be created or resolution fails.
#[tokio::main]
async fn main() -> Result<()> {
    // Create a new DNS core: single forwarder mode
    let config = UsageProfile::SingleForwarder(Upstream::doh(
        "cloudflare-doh",
        "https://cloudflare-dns.com/dns-query",
        None,
    ))
    .config();

    let core = ArachneaDnsCore::new(config)?;

    // Get arguments from command line
    let domain = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "www.google.lu".to_owned());

    // Resolve IP addresses
    println!("Resolve IP {domain} using single_forwarder and DoH:");
    for ip in core.resolve_ip(&domain).await? {
        println!(" - {ip}");
    }
    println!("");

    // Resolve A records
    println!("Resolve A {domain} using single_forwarder and DoH:");
    let answer = core
        .resolve(QueryRequest::new(&domain, RecordType::A))
        .await?;

    for record in answer.records {
        println!(" - {record:?}");
    }

    Ok(())
}
