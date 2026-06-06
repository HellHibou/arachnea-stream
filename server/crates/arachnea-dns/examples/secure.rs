use anyhow::Result;
use arachnea_dns::core::{
    ArachneaDnsCore, DnssecState, QueryRequest, RecordType, Upstream, UsageProfile,
};

// cargo run -p arachnea-dns --features doh,dnssec --example secure -- www.google.lu
/// Runs the secure DNS profile example.
///
/// # Errors
///
/// Returns an error when the DNS core cannot be created or resolution fails.
#[tokio::main]
async fn main() -> Result<()> {
    // Create a new DNS core: secure, DoH and DNSSEC
    let config = UsageProfile::Secure(
        vec![Upstream::doh(
            "cloudflare-doh",
            "https://cloudflare-dns.com/dns-query",
            None,
        )],
        DnssecState::ReportOnly,
    )
    .config();

    let core = ArachneaDnsCore::new(config)?;

    // Get arguments from command line
    let domain = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "www.google.lu".to_owned());

    // Resolve IP addresses
    println!("Resolve IP {domain} using secure and DoH:");
    for ip in core.resolve_ip(&domain).await? {
        println!(" - {ip}");
    }
    println!("");

    // Resolve A records
    let answer = core
        .resolve(QueryRequest::new(&domain, RecordType::A))
        .await?;
    println!(
        "Resolve A {domain} using secure and DoH, DNSSEC: {:?}:",
        answer.metadata.dnssec
    );

    for record in answer.records {
        println!(" - {record:?}");
    }

    Ok(())
}
