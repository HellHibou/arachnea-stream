use crate::core::*;

/// Verifies that blocklists take precedence over local records.
#[tokio::test]
async fn blocklist_wins_over_local_records() {
    let config = CoreConfig {
        blocklists: vec![BlockRule {
            name: "ads".to_owned(),
            pattern: DomainPattern::new("ads.example"),
            action: BlockAction::NoData,
            enabled: true,
        }],
        local_records: vec![LocalRecord {
            name: "ads.example".to_owned(),
            wildcard: false,
            priority_over_upstream: true,
            records: vec![Record {
                name: "ads.example".to_owned(),
                ttl: 60,
                data: RecordData::A {
                    address: "192.0.2.10".parse().unwrap(),
                },
            }],
        }],
        ..UsageProfile::SystemRelay.config()
    };
    let core = ArachneaDnsCore::new(config).unwrap();

    let answer = core
        .resolve(QueryRequest::new("ads.example", RecordType::A))
        .await
        .unwrap();

    assert!(answer.records.is_empty());
    assert_eq!(
        answer.metadata.policy.unwrap().source,
        PolicySource::Blocklist
    );
}

/// Verifies that local records take precedence over Smart DNS answers.
#[tokio::test]
async fn local_records_win_over_smart_dns() {
    let config = CoreConfig {
        local_records: vec![LocalRecord {
            name: "media.example".to_owned(),
            wildcard: false,
            priority_over_upstream: true,
            records: vec![Record {
                name: "media.example".to_owned(),
                ttl: 60,
                data: RecordData::A {
                    address: "192.0.2.10".parse().unwrap(),
                },
            }],
        }],
        smart_dns: SmartDnsConfig {
            rules: vec![SmartDnsRule {
                name: "media".to_owned(),
                pattern: DomainPattern::new("media.example"),
                action: SmartDnsAction::Answer {
                    records: vec![Record {
                        name: "media.example".to_owned(),
                        ttl: 60,
                        data: RecordData::A {
                            address: "198.51.100.10".parse().unwrap(),
                        },
                    }],
                },
            }],
        },
        ..UsageProfile::SystemRelay.config()
    };
    let core = ArachneaDnsCore::new(config).unwrap();

    let answer = core
        .resolve(QueryRequest::new("media.example", RecordType::A))
        .await
        .unwrap();

    assert_eq!(
        answer.records[0].data,
        RecordData::A {
            address: "192.0.2.10".parse().unwrap()
        }
    );
}

/// Verifies that cache keys include privacy-affecting policy dimensions.
#[test]
fn cache_key_tracks_privacy_affecting_policy() {
    let core = ArachneaDnsCore::new(UsageProfile::SystemRelay.config()).unwrap();
    let query = QueryRequest::new("Example.COM.", RecordType::A);
    let key = core.cache_key(&query);

    assert_eq!(key.name, "example.com");
    assert_eq!(key.ecs, EcsPolicy::Disabled);
}

/// Verifies that `resolve_ip` preserves transport errors instead of masking them.
#[cfg(all(feature = "dnssec", feature = "doh"))]
#[tokio::test]
async fn resolve_ip_preserves_non_negative_errors() {
    let config = UsageProfile::Secure(
        vec![Upstream::doh(
            "strict-doh",
            "https://example.invalid/dns-query",
            None,
        )],
        DnssecState::Strict,
    )
    .config();
    let core = ArachneaDnsCore::new(config).unwrap();

    let error = core.resolve_ip("example.com").await.unwrap_err();

    assert!(matches!(error, DnsError::UnsupportedFeature(_)));
}
