use super::*;

#[test]
fn provider_records_reuse_fresh_cache_and_probe_only_unknown_or_expired_endpoints() {
    let config = InventoryConfig::default();
    let now = SystemTime::now();

    let mut fresh = record(ProxyRuntimeStatus::Ko);
    fresh.failure_count = 3;
    fresh.authentication_required = Some(true);
    fresh.cooldown_until = Some(now + Duration::from_secs(60));
    fresh.destination_failures = vec![ProxyDestinationFailure {
        scheme: "https".to_string(),
        host: "origin.example".to_string(),
        port: 443,
        reason: crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
        failure_count: 2,
        last_failed: now,
        cooldown_until: None,
    }];

    let mut untested = record(ProxyRuntimeStatus::Unknown);
    untested.host = "untested.example".to_string();
    untested.last_checked = None;

    let mut expired = record(ProxyRuntimeStatus::Ok);
    expired.host = "expired.example".to_string();
    expired.last_checked = Some(now - config.probe_ttl - Duration::from_secs(1));

    let mut provider_fresh = record(ProxyRuntimeStatus::Unknown);
    provider_fresh.protocol = Some(ProxyProtocol::Http);
    provider_fresh.supports_https = None;
    provider_fresh.latency_ms = None;
    provider_fresh.last_checked = None;
    provider_fresh.last_validated_at = None;

    let mut provider_untested = provider_fresh.clone();
    provider_untested.host = untested.host.clone();
    let mut provider_expired = provider_fresh.clone();
    provider_expired.host = expired.host.clone();
    let mut provider_new = provider_fresh.clone();
    provider_new.host = "new.example".to_string();

    let existing = HashMap::from([
        (fresh.authority(), fresh.clone()),
        (untested.authority(), untested),
        (expired.authority(), expired),
    ]);
    let mut provider_fresh_http = provider_fresh.clone();
    provider_fresh_http.protocol = Some(ProxyProtocol::Http);
    provider_fresh_http.declarations = vec![ProxyDeclaration {
        source: "secondary-provider".to_string(),
        protocol: Some(ProxyProtocol::Http),
        supports_https: None,
    }];
    let (prepared, probe_indices, duplicate_count, cached_count) = prepare_provider_records(
        vec![
            provider_fresh.clone(),
            provider_fresh_http,
            provider_untested,
            provider_expired,
            provider_new,
        ],
        &existing,
        now,
        config.probe_ttl,
    );

    assert_eq!(prepared.len(), 4);
    let mut expected_fresh = fresh;
    expected_fresh.supports_https = None;
    expected_fresh.declarations.push(ProxyDeclaration {
        source: "secondary-provider".to_string(),
        protocol: Some(ProxyProtocol::Http),
        supports_https: None,
    });
    assert_eq!(prepared[0], expected_fresh);
    assert_eq!(probe_indices, vec![1, 2, 3]);
    assert_eq!(duplicate_count, 1);
    assert_eq!(cached_count, 1);
}
