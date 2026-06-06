use anyhow::Result;
use arachnea_dns::core::{
    ArachneaDnsCore, LocalRecord, QueryRequest, Record, RecordData, RecordType, UsageProfile,
};

// cargo run -p arachnea-dns --example advanced -- example.internal
/// Runs the advanced DNS configuration example.
///
/// # Errors
///
/// Returns an error when the DNS core cannot be created or resolution fails.
#[tokio::main]
async fn main() -> Result<()> {
    let domain = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "example.internal".to_owned());

    let mut config = UsageProfile::Advanced.config();

    config.local_records = vec![LocalRecord {
        name: domain.clone(),
        records: vec![Record {
            name: domain.clone(),
            ttl: 300,
            data: RecordData::A {
                address: "192.168.42.10".parse()?,
            },
        }],
        wildcard: false,
        priority_over_upstream: true,
    }];

    let core = ArachneaDnsCore::new(config)?;
    let answer = core
        .resolve(QueryRequest::new(&domain, RecordType::A))
        .await?;

    println!("Mode advanced avec entree locale pour {domain}:");
    for record in answer.records {
        println!("{record:?}");
    }

    Ok(())
}
