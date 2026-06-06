use anyhow::Result;
use arachnea_dns::core::{ArachneaDnsCore, QueryRequest, RecordType, Upstream, UsageProfile};

// cargo run -p arachnea-dns --example resilience -- www.google.lu
/// Runs the resilient DNS profile example.
///
/// # Errors
///
/// Returns an error when the DNS core cannot be created or resolution fails.
#[tokio::main]
async fn main() -> Result<()> {
    // Create a new DNS core: resilience mode
    let config = UsageProfile::Resilience(vec![
        Upstream::udp("localhost-udp", "127.0.0.1:666".parse()?),
        Upstream::system(None),
        Upstream::udp("quad9-udp", "9.9.9.9:53".parse()?),
        Upstream::udp("cloudflare-udp", "1.1.1.1:53".parse()?),
    ])
    .config();

    let core = ArachneaDnsCore::new(config)?;

    // Get arguments from command line
    let domain = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "www.google.lu".to_owned());

    // Resolve IP addresses
    println!("Resolve IP {domain} using resilience mode :");
    for ip in core.resolve_ip(&domain).await? {
        println!(" - {ip}");
    }
    println!("");

    // Resolve A records
    let answer = core
        .resolve(QueryRequest::new(&domain, RecordType::A))
        .await?;
    println!(
        "Resolve A {domain} using resilience mode, upstream {} :",
        answer.metadata.upstream.as_deref().unwrap_or("unknown")
    );

    for record in answer.records {
        println!(" - {record:?}");
    }

    Ok(())
}
