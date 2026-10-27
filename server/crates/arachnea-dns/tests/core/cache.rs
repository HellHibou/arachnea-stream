use crate::core::*;

/// Verifies aggressive NSEC caching can synthesize negative answers.
#[tokio::test]
async fn aggressive_nsec_cache_synthesizes_negative_answers() {
    let cache = crate::core::cache::Cache::new(CacheConfig {
        enabled: true,
        aggressive_dnssec_negative_cache: true,
        ..CacheConfig::default()
    });

    cache
        .put_nsec_proofs(vec![crate::core::cache::NsecProof {
            owner: "a.example".to_owned(),
            next_owner: "z.example".to_owned(),
            types: vec![hickory_proto::rr::RecordType::A],
        }])
        .await;

    let covered = cache
        .get_aggressive_negative(&QueryRequest::new("m.example", RecordType::A))
        .await;
    assert!(matches!(covered, Some(DnsError::Nxdomain(name)) if name == "m.example"));

    let nodata = cache
        .get_aggressive_negative(&QueryRequest::new("a.example", RecordType::AAAA))
        .await;
    assert!(matches!(
        nodata,
        Some(DnsError::NoData {
            name,
            record_type: RecordType::AAAA,
        }) if name == "a.example"
    ));
}
