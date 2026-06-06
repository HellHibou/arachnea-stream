use anyhow::Result;
use arachnea_dns::core::{ArachneaDnsCore, UsageProfile};
use arachnea_dns::core::{QueryRequest, RecordType};

// cargo run -p arachnea-dns --example system_relay -- www.google.lu
/// Runs the system relay DNS example.
///
/// # Errors
///
/// Returns an error when the DNS core cannot be created or resolution fails.
#[tokio::main]
async fn main() -> Result<()> {
    // Create a new DNS core: system relay mode
    let core = ArachneaDnsCore::new(UsageProfile::SystemRelay.config())?;

    // Get arguments from command line
    let domain = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "www.google.lu".to_owned());

    // Resolve IP addresses
    println!("Resolve IP {domain} using system_relay:");
    for ip in core.resolve_ip(&domain).await? {
        println!(" - {ip}");
    }
    println!("");

    // Resolve A records
    println!("Resolve A {domain} using system_relay:");
    let answer = core
        .resolve(QueryRequest::new(&domain, RecordType::A))
        .await?;

    for record in answer.records {
        println!(" - {record:?}");
    }

    Ok(())
}
