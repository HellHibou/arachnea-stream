use super::*;

/// Verifies that proxy profiles select their matching DNS profile defaults.
#[cfg(feature = "arachnea-dns")]
#[test]
fn arachnea_dns_profile_matches_proxy_profile() {
    let privacy_dns =
        ArachneaProxyCore::arachnea_dns_config_for_proxy_profile(&ProxyProfile::Privacy);
    let censorship_dns = ArachneaProxyCore::arachnea_dns_config_for_proxy_profile(
        &ProxyProfile::CensorshipResistance,
    );
    let advanced_dns =
        ArachneaProxyCore::arachnea_dns_config_for_proxy_profile(&ProxyProfile::Advanced);

    assert!(matches!(
        privacy_dns.profile,
        arachnea_dns::core::UsageProfile::Privacy(_)
    ));
    assert!(matches!(
        censorship_dns.profile,
        arachnea_dns::core::UsageProfile::Resilience(_)
    ));
    assert!(matches!(
        advanced_dns.profile,
        arachnea_dns::core::UsageProfile::Advanced
    ));
    assert!(privacy_dns.resolve_effective_config().is_ok());
    assert!(censorship_dns.resolve_effective_config().is_ok());
    assert!(advanced_dns.resolve_effective_config().is_ok());
}

/// Verifies that the default constructor installs a DNS core when the feature is active.
#[cfg(feature = "arachnea-dns")]
#[test]
fn new_installs_default_arachnea_dns_core() {
    let core = ArachneaProxyCore::new(ProxyConfig::default()).unwrap();

    assert!(format!("{core:?}").contains("arachnea_dns_enabled: true"));
}

/// Verifies that the optional DNS core can resolve direct proxy destinations.
#[cfg(feature = "arachnea-dns")]
#[tokio::test]
async fn arachnea_dns_core_resolves_direct_destination() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let accept = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.unwrap();
    });
    let dns_core = arachnea_dns::core::ArachneaDnsCore::new(arachnea_dns::core::CoreConfig {
        local_records: vec![arachnea_dns::core::LocalRecord {
            name: "proxy-target.test".to_string(),
            records: vec![arachnea_dns::core::Record {
                name: "proxy-target.test".to_string(),
                ttl: 60,
                data: arachnea_dns::core::RecordData::A {
                    address: "127.0.0.1".parse().unwrap(),
                },
            }],
            wildcard: false,
            priority_over_upstream: true,
        }],
        ..arachnea_dns::core::UsageProfile::SystemRelay.config()
    })
    .unwrap();
    let core = ArachneaProxyCore::new_with_arachnea_dns(ProxyConfig::default(), dns_core).unwrap();

    let stream = core
        .connect(Destination::host_port("proxy-target.test", port))
        .await;

    assert!(stream.is_ok());
    accept.await.unwrap();
}
