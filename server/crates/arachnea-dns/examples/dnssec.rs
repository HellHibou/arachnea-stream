use anyhow::Result;
use arachnea_dns::core::{
    ArachneaDnsCore, DnssecState, QueryRequest, RecordType, Upstream, UsageProfile,
};

// cargo run -p arachnea-dns --features dnssec --example dnssec -- www.google.lu
/// Runs the DNSSEC example.
///
/// # Errors
///
/// Returns an error when the DNS core cannot be created or resolution fails.
#[tokio::main]
async fn main() -> Result<()> {
    let config = UsageProfile::Secure(
        vec![Upstream::udp("cloudflare-udp", "1.1.1.1:53".parse()?)],
        DnssecState::Strict,
    )
    .config();

    let core = ArachneaDnsCore::new(config)?;

    // Get arguments from command line
    let domain = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "www.google.lu".to_owned());

    // Resolve IP addresses
    println!("Resolve IP {domain} using DNSSEC mode, Hickory :");
    for ip in core.resolve_ip(&domain).await? {
        println!(" - {ip}");
    }
    println!("");

    // Resolve A records
    println!("Resolve A {domain} using DNSSEC mode, Hickory :");
    let answer = core
        .resolve(QueryRequest::new(&domain, RecordType::A))
        .await?;
    for record in answer.records {
        println!(" - {record:?}");
    }

    Ok(())
}
