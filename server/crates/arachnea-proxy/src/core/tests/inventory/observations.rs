use super::*;

#[test]
fn default_destination_failure_cooldown_is_24_hours() {
    assert_eq!(
        InventoryConfig::default().destination_failure_cooldown,
        PROXY_DESTINATION_FAILURE_COOLDOWN
    );
    assert_eq!(
        PROXY_DESTINATION_FAILURE_COOLDOWN,
        Duration::from_secs(24 * 60 * 60)
    );
    let now = SystemTime::now();
    let destination = Destination::host_port("transient.example", 443)
        .with_protocol(crate::core::ApplicationProtocol::Https);
    let mut proxy = record(ProxyRuntimeStatus::Ok);
    proxy
        .destination_failures
        .push(crate::core::ProxyDestinationFailure {
            scheme: "https".into(),
            host: "transient.example".into(),
            port: 443,
            reason: crate::core::ProxyDestinationFailureReason::Other,
            failure_count: 1,
            last_failed: now - Duration::from_secs(31),
            cooldown_until: Some(now + PROXY_DESTINATION_FAILURE_COOLDOWN),
        });
    assert!(
        !has_active_destination_cooldown(&proxy, &destination, now),
        "legacy ambiguous exclusions must use the short policy without resetting the cache"
    );
    proxy.destination_failures[0].last_failed = now;
    assert!(has_active_destination_cooldown(&proxy, &destination, now));
    proxy.destination_failures[0].last_failed = now - Duration::from_secs(31);
    proxy.destination_failures[0].reason =
        crate::core::ProxyDestinationFailureReason::BlockedByOrigin;
    assert!(has_active_destination_cooldown(&proxy, &destination, now));
}

#[tokio::test]
async fn blocked_by_origin_remains_scoped_to_its_destination() {
    let mut provider_record = record(ProxyRuntimeStatus::Ok);
    provider_record.host = "replacement.example".to_string();
    let provider = Arc::new(CountingProvider {
        records: vec![provider_record.clone()],
        calls: AtomicUsize::new(0),
        delay: Duration::ZERO,
    });
    let inventory = ProxyInventory::new(
        InventoryConfig {
            max_destination_failures_before_ko: 1,
            ..InventoryConfig::default()
        },
        Some(provider.clone()),
        None,
    );
    let cached = record(ProxyRuntimeStatus::Ok);
    let authority = cached.authority();
    inventory.add_or_update(vec![cached]).await;
    let blocked_destination = Destination::host_port("blocked.example", 443)
        .with_protocol(crate::core::ApplicationProtocol::Https);
    let other_destination = Destination::host_port("allowed.example", 443)
        .with_protocol(crate::core::ApplicationProtocol::Https);

    let marked_globally_ko = inventory
        .record_destination_failure(
            &authority,
            &blocked_destination,
            crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
        )
        .await;

    assert!(!marked_globally_ko);
    assert_eq!(
        inventory
            .select_for_destination("BE", true, Some(&blocked_destination))
            .await
            .unwrap()
            .authority(),
        provider_record.authority()
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        inventory
            .select_for_destination("BE", true, Some(&other_destination))
            .await
            .unwrap()
            .authority(),
        authority
    );
}

#[cfg(feature = "persistence")]
#[tokio::test]
async fn blocked_by_origin_is_restored_from_persistence() -> AnyResult<()> {
    let store = memory_proxy_store()?;
    let repository = Arc::new(TypedProxyRepository::new(Arc::clone(&store)));
    let preferences = Arc::new(crate::core::TypedProxyPreferenceRepository::new(
        crate::core::proxy_preference_repository::memory_proxy_preference_store()?,
    ));
    let initial = ProxyInventory::new(InventoryConfig::default(), None, None)
        .with_proxy_repository(repository.clone())
        .with_preference_repository(preferences.clone());
    let mut blocked = record(ProxyRuntimeStatus::Ok);
    blocked.host = "blocked-proxy.example".to_string();
    blocked.latency_ms = Some(5);
    let mut fallback = record(ProxyRuntimeStatus::Ok);
    fallback.host = "fallback-proxy.example".to_string();
    fallback.latency_ms = Some(50);
    let blocked_authority = blocked.authority();
    initial
        .add_or_update(vec![blocked.clone(), fallback.clone()])
        .await;
    initial.persist_records(&[blocked, fallback.clone()]).await;
    let destination = Destination::host_port("origin.example", 443)
        .with_protocol(crate::core::ApplicationProtocol::Https);
    let redirect_destination = Destination::host_port("redirect.example", 443)
        .with_protocol(crate::core::ApplicationProtocol::Https);
    let redirect_token = crate::core::ProxySelectionToken::new(
        Arc::new(initial.clone()),
        fallback.authority(),
        redirect_destination.clone(),
    );
    redirect_token.record_redirect(302).await;

    initial
        .record_destination_failure(
            &blocked_authority,
            &destination,
            crate::core::ProxyDestinationFailureReason::Other,
        )
        .await;
    initial
        .record_validated_response(&blocked_authority, &destination)
        .await;
    let mut legacy = repository
        .get(&initial.inner.read().await.records[&blocked_authority].key())
        .await?
        .unwrap();
    assert!(
        legacy.destination_failures.is_empty(),
        "accepted responses must durably clear older ambiguous failures"
    );
    let accepted_at = preferences
        .get(&crate::core::ProxyDestinationPreference::destination_key(
            &destination,
        ))
        .await?
        .unwrap()
        .endpoints[0]
        .accepted_at;
    let old_failure = crate::core::ProxyDestinationFailure {
        scheme: "https".into(),
        host: "origin.example".into(),
        port: 443,
        reason: crate::core::ProxyDestinationFailureReason::Other,
        failure_count: 1,
        last_failed: accepted_at - Duration::from_secs(1),
        cooldown_until: Some(accepted_at + Duration::from_secs(86400)),
    };
    legacy.destination_failures.push(old_failure.clone());
    for (scheme, host, port) in [
        ("http", "origin.example", 443),
        ("https", "other.example", 443),
        ("https", "origin.example", 8443),
    ] {
        let mut unrelated = old_failure.clone();
        unrelated.scheme = scheme.into();
        unrelated.host = host.into();
        unrelated.port = port;
        legacy.destination_failures.push(unrelated);
    }
    repository.save_many(&[legacy.clone()]).await?;
    let provider = Arc::new(CountingProvider {
        records: Vec::new(),
        calls: AtomicUsize::new(0),
        delay: Duration::ZERO,
    });
    let restore_started = Instant::now();
    let restarted = ProxyInventory::new(InventoryConfig::default(), Some(provider.clone()), None)
        .with_proxy_repository(repository.clone())
        .with_preference_repository(preferences.clone());
    assert_eq!(
        restarted
            .select_for_destination("BE", true, Some(&destination))
            .await?
            .authority(),
        blocked_authority
    );
    let restore_us = restore_started.elapsed().as_micros();
    let redirect_search = ProxySearchContext::default();
    let redirected = redirect_search
        .scope(restarted.select_for_destination("BE", true, Some(&redirect_destination)))
        .await?;
    assert_eq!(redirected.authority(), fallback.authority());
    assert_eq!(
        redirect_search.selection_provenance(&redirected.authority(), &redirect_destination),
        crate::core::ProxySelectionProvenance::RedirectDestination
    );
    let repaired = repository.get(&legacy.key()).await?.unwrap();
    assert_eq!(repaired.destination_failures.len(), 3);
    assert!(!has_active_destination_cooldown(
        &repaired,
        &destination,
        SystemTime::now()
    ));
    for (reason, last_failed) in [
        (
            crate::core::ProxyDestinationFailureReason::Other,
            accepted_at,
        ),
        (
            crate::core::ProxyDestinationFailureReason::Other,
            accepted_at + Duration::from_secs(1),
        ),
        (
            crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
            accepted_at - Duration::from_secs(1),
        ),
    ] {
        let mut protected = repaired.clone();
        let mut failure = old_failure.clone();
        failure.reason = reason;
        failure.last_failed = last_failed;
        protected.destination_failures.push(failure);
        store.put(&protected).await?;
        repository
            .clear_superseded_destination_failures(&legacy.key(), &destination, accepted_at)
            .await?;
        let protected_inventory = ProxyInventory::new(InventoryConfig::default(), None, None)
            .with_proxy_repository(repository.clone())
            .with_preference_repository(preferences.clone());
        assert_eq!(
            protected_inventory
                .select_for_destination("BE", true, Some(&destination))
                .await?
                .authority(),
            fallback.authority(),
            "restored successes must preserve explicit, equal-date and newer exclusions"
        );
        assert_eq!(
            repository
                .get(&legacy.key())
                .await?
                .unwrap()
                .destination_failures
                .len(),
            4
        );
    }
    store.put(&repaired).await?;
    let warm_started = Instant::now();
    for countries in [
        vec!["FR".into(), "BE".into()],
        vec!["BE".into(), "FR".into()],
    ] {
        assert_eq!(
            restarted
                .select_any_for_destination(&countries, true, Some(&destination))
                .await?
                .authority(),
            blocked_authority
        );
    }
    assert!(restarted
        .select_for_destination("FR", true, Some(&destination))
        .await
        .is_err());
    assert_eq!(
        restarted
            .select_for_destination("BE", true, Some(&destination))
            .await?
            .authority(),
        blocked_authority
    );
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        1,
        "only the disallowed-country search should consult the provider"
    );
    eprintln!("lot7 restored preference: first_select_us={restore_us}, warm_and_country_checks_us={}, preferred_provider_calls=0", warm_started.elapsed().as_micros());
    initial.record_ok(&blocked_authority).await;

    initial
        .record_destination_failure(
            &blocked_authority,
            &destination,
            crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
        )
        .await;
    initial.record_ok(&blocked_authority).await;

    let restored = ProxyInventory::new(InventoryConfig::default(), None, None)
        .with_proxy_repository(repository)
        .with_preference_repository(preferences.clone());
    assert_eq!(
        restored
            .select_for_destination("BE", true, Some(&destination))
            .await?
            .authority(),
        fallback.authority()
    );
    let history = preferences
        .get(&crate::core::ProxyDestinationPreference::destination_key(
            &destination,
        ))
        .await?
        .unwrap();
    assert_eq!(history.endpoints[0].endpoint.host, "blocked-proxy.example");
    restored
        .record_validated_response(&fallback.authority(), &destination)
        .await;
    assert_eq!(
        preferences
            .get(&history.destination)
            .await?
            .unwrap()
            .endpoints
            .len(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn endpoint_failure_retention_uses_positive_evidence_not_attempt_date() {
    let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
    let now = SystemTime::now();
    let mut recent = record(ProxyRuntimeStatus::Ok);
    recent.host = "recent.example".into();
    recent.health_evidence.last_positive_at = Some(now - Duration::from_secs(23 * 3600));
    let mut stale = recent.clone();
    stale.host = "stale.example".into();
    stale.health_evidence.last_positive_at = Some(now - Duration::from_secs(25 * 3600));
    stale.last_checked = Some(now);
    let mut never = stale.clone();
    never.host = "never.example".into();
    never.health_evidence.last_positive_at = None;
    never.health_evidence.first_discovered_at = Some(now - Duration::from_secs(25 * 3600));
    inventory
        .add_or_update(vec![recent.clone(), stale.clone(), never.clone()])
        .await;
    for proxy in [&recent, &stale, &never] {
        inventory.record_global_failure(&proxy.authority()).await;
    }
    let records = inventory.all_records().await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].authority(), recent.authority());
    assert_eq!(
        records[0].health_evidence.last_positive_at,
        recent.health_evidence.last_positive_at
    );
    assert!(records[0].cooldown_until.unwrap() > now);
    assert_eq!(inventory.config.ko_cooldown, Duration::from_secs(15 * 60));
}

#[tokio::test]
async fn cooldown_expiry_allows_endpoint_revalidation_and_preserves_discovery() -> AnyResult<()> {
    let (mut proxy, server) = local_http_endpoint(
        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        Arc::new(tokio::sync::Barrier::new(1)),
        1,
    )
    .await;
    proxy.status = ProxyRuntimeStatus::Unknown;
    proxy.health_evidence.first_discovered_at = Some(SystemTime::now() - Duration::from_secs(3600));
    let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
    inventory.add_or_update(vec![proxy.clone()]).await;
    inventory.record_global_failure(&proxy.authority()).await;
    assert!(inventory.select("BE", false).await.is_err());
    {
        let mut inner = inventory.inner.write().await;
        inner
            .records
            .get_mut(&proxy.authority())
            .unwrap()
            .cooldown_until = Some(SystemTime::now() - Duration::from_secs(1));
    }
    let candidate = inventory.inner.read().await.records[&proxy.authority()].clone();
    assert!(is_selection_candidate(
        &candidate,
        "BE",
        false,
        None,
        SystemTime::now(),
        ProbeMode::Relaxed
    ));
    let probe = ProxyProbe::new(crate::core::ProbeConfig::default());
    probe.precheck_endpoint(&candidate).await?;
    inventory.record_ok(&candidate.authority()).await;
    let restored = inventory.all_records().await.pop().unwrap();
    assert_eq!(
        restored.health_evidence.first_discovered_at,
        proxy.health_evidence.first_discovered_at
    );
    assert!(restored.health_evidence.last_positive_at.is_some());
    assert!(restored.cooldown_until.is_none());
    // The precheck sends no request, so this fixture is no longer needed.
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    Ok(())
}

#[tokio::test]
async fn endpoint_eviction_during_delayed_http_response_does_not_resurrect_history() -> AnyResult<()>
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tokio::time::timeout(Duration::from_secs(5), async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (received_tx, received_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                head.push(stream.read_u8().await?);
                anyhow::ensure!(head.len() <= 8192, "fixture header limit");
            }
            received_tx
                .send(())
                .map_err(|_| anyhow::anyhow!("receiver dropped"))?;
            release_rx.await?;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await?;
            stream.shutdown().await?;
            Ok::<_, anyhow::Error>(())
        });
        let mut proxy = record(ProxyRuntimeStatus::Ok);
        proxy.host = address.ip().to_string();
        proxy.port = address.port();
        proxy.protocol = Some(ProxyProtocol::Http);
        proxy.declarations.clear();
        let core = local_routing_core(ProxyInventory::new(InventoryConfig::default(), None, None));
        let inventory = core.proxy_inventory().unwrap();
        inventory.add_or_update(vec![proxy.clone()]).await;
        let destination = Destination::host_port("origin.example", 80)
            .with_protocol(crate::core::ApplicationProtocol::Http);
        let selected = select_local_route(&core, &destination, "BE").await?;
        let token = selected.selection_token.clone().unwrap();
        let request_destination = destination.clone();
        let exchange =
            tokio::spawn(
                async move { pinned_http_exchange(&selected, &request_destination).await },
            );
        received_rx.await?;
        // Connection succeeded earlier; arrange a subsequently observed stale failure.
        inventory
            .inner
            .write()
            .await
            .records
            .get_mut(&proxy.authority())
            .unwrap()
            .health_evidence
            .last_positive_at = Some(SystemTime::now() - Duration::from_secs(25 * 3600));
        inventory.record_global_failure(&proxy.authority()).await;
        assert!(inventory.is_empty().await);
        release_tx
            .send(())
            .map_err(|_| anyhow::anyhow!("server dropped"))?;
        assert_eq!(exchange.await??, 200);
        token.record_accepted().await;
        assert!(inventory.is_empty().await);
        assert!(inventory
            .inner
            .read()
            .await
            .preferred_by_destination
            .is_empty());
        server.await??;
        Ok(())
    })
    .await?
}
