use super::*;

/// Verifies that the resilience profile falls back after NXDOMAIN responses.
#[test]
fn profile_resilience_falls_back_on_nxdomain() {
    let config = UsageProfile::Resilience(Vec::new()).config();

    assert_eq!(config.resolver.mode, ResolutionMode::Recursive);
    assert_eq!(
        config.resolver.nxdomain_strategy,
        NxdomainStrategy::FallbackOnNxdomain
    );
}

/// Verifies that embedded root hints are loaded by default.
#[test]
fn embedded_root_hints_are_loaded_by_default() {
    let config = UsageProfile::SystemRelay
        .config()
        .resolve_effective_config()
        .unwrap();

    assert!(config
        .config
        .resolver
        .root_hints
        .contains(&"198.41.0.4".to_owned()));
    assert!(config
        .config
        .resolver
        .root_hints
        .contains(&"2001:503:ba3e::2:30".to_owned()));
}

/// Verifies that configured root hints override embedded defaults.
#[test]
fn direct_root_hints_override_embedded_defaults() {
    let mut config = UsageProfile::SystemRelay.config();
    config.resolver.root_hints = vec!["203.0.113.53".to_owned()];

    let effective = config.resolve_effective_config().unwrap();

    assert_eq!(effective.config.resolver.root_hints, vec!["203.0.113.53"]);
}

/// Verifies that root hints can be loaded from an external file.
#[test]
fn root_hints_can_be_loaded_from_external_file() {
    let path = std::env::temp_dir().join(format!("arachnea-root-hints-{}.txt", std::process::id()));
    std::fs::write(&path, "# test root\n192.0.2.53\n2001:db8::53\n").unwrap();

    let mut config = UsageProfile::SystemRelay.config();
    config.resolver.root_hints_file = Some(path.clone());

    let effective = config.resolve_effective_config().unwrap();
    let _ = std::fs::remove_file(path);

    assert_eq!(
        effective.config.resolver.root_hints,
        vec!["192.0.2.53", "2001:db8::53"]
    );
}

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
