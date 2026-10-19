use super::*;

#[tokio::test]
async fn validated_proxy_is_preferred_only_for_its_destination() {
    let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
    let mut fastest = record(ProxyRuntimeStatus::Ok);
    fastest.host = "fastest.example".to_string();
    fastest.latency_ms = Some(5);
    let mut validated = record(ProxyRuntimeStatus::Ok);
    validated.host = "validated.example".to_string();
    validated.latency_ms = Some(100);
    inventory
        .add_or_update(vec![fastest.clone(), validated.clone()])
        .await;
    let preferred_destination = Destination::host_port("preferred.example", 443)
        .with_protocol(crate::core::ApplicationProtocol::Https);
    let other_destination = Destination::host_port("other.example", 443)
        .with_protocol(crate::core::ApplicationProtocol::Https);

    inventory
        .record_validated_response(&validated.authority(), &preferred_destination)
        .await;

    validated.last_checked =
        Some(SystemTime::now() - InventoryConfig::default().probe_ttl - Duration::from_secs(1));
    inventory.add_or_update(vec![validated.clone()]).await;
    assert_eq!(
        inventory
            .select_any_for_destination(&["BE".to_string()], true, Some(&preferred_destination))
            .await
            .unwrap()
            .authority(),
        validated.authority()
    );

    assert_eq!(
        inventory
            .select_for_destination("BE", true, Some(&preferred_destination))
            .await
            .unwrap()
            .authority(),
        validated.authority()
    );

    {
        let inventory = Arc::new(ProxyInventory::new(InventoryConfig::default(), None, None));
        inventory
            .add_or_update(vec![fastest.clone(), validated.clone()])
            .await;
        let countries = vec!["BE".to_string()];
        let leader = ProxySearchContext::default();
        let selected = leader
            .scope(inventory.select_any_for_destination_with_affinity(
                &countries,
                true,
                Some(&preferred_destination),
                Some("shared-cold-affinity"),
            ))
            .await
            .unwrap();
        let token = leader
            .scope(async {
                crate::core::ProxySelectionToken::new(
                    inventory.clone(),
                    selected.authority(),
                    preferred_destination.clone(),
                )
            })
            .await;
        assert_eq!(
            token.provenance(),
            crate::core::ProxySelectionProvenance::Discovery
        );
        let follower = ProxySearchContext::default();
        let pending = follower.scope(inventory.select_any_for_destination_with_affinity(
            &countries,
            true,
            Some(&preferred_destination),
            Some("shared-cold-affinity"),
        ));
        tokio::pin!(pending);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut pending)
                .await
                .is_err(),
            "cold follower must await the leader's business result"
        );
        let unrelated = inventory
            .select_any_for_destination(&countries, true, Some(&other_destination))
            .await
            .unwrap();
        assert_eq!(unrelated.authority(), fastest.authority());
        token.record_accepted().await;
        let selected = tokio::time::timeout(Duration::from_secs(1), &mut pending)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(selected.authority(), token.endpoint());
        assert_eq!(
            follower.selection_provenance(&selected.authority(), &preferred_destination),
            crate::core::ProxySelectionProvenance::AcceptedDestination
        );

        let cancelled = Destination::host_port("cancelled.example", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        let selected = leader
            .scope(inventory.select_any_for_destination(&countries, true, Some(&cancelled)))
            .await
            .unwrap();
        let token = leader
            .scope(async {
                crate::core::ProxySelectionToken::new(
                    inventory.clone(),
                    selected.authority(),
                    cancelled.clone(),
                )
            })
            .await;
        let abandoned = ProxySearchContext::default();
        assert!(tokio::time::timeout(
            Duration::from_millis(10),
            abandoned.scope(inventory.select_any_for_destination(
                &countries,
                true,
                Some(&cancelled)
            ))
        )
        .await
        .is_err());
        drop(token);
        assert!(
            tokio::time::timeout(
                Duration::from_secs(1),
                inventory.select_any_for_destination(&countries, true, Some(&cancelled))
            )
            .await
            .unwrap()
            .is_ok(),
            "cancelling a leader and a waiter must release discovery ownership"
        );
    }
    {
        let provider = Arc::new(CountingProvider {
            records: Vec::new(),
            calls: AtomicUsize::new(0),
            delay: Duration::from_secs(60),
        });
        let waiting_inventory =
            ProxyInventory::new(InventoryConfig::default(), Some(provider.clone()), None);
        let countries = vec!["BE".into(), "FR".into()];
        let search = ProxySearchContext::default();
        let waiting = search.scope(waiting_inventory.select_any_for_destination(
            &countries,
            true,
            Some(&preferred_destination),
        ));
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut waiting)
                .await
                .is_err()
        );
        assert!(provider.calls.load(Ordering::SeqCst) > 0);
        waiting_inventory
            .add_or_update(vec![validated.clone()])
            .await;
        waiting_inventory
            .record_validated_response(&validated.authority(), &preferred_destination)
            .await;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), &mut waiting)
                .await
                .unwrap()
                .unwrap()
                .authority(),
            validated.authority(),
            "an accepted response must wake discovery already awaiting a slow provider"
        );
    }
    assert_eq!(
        inventory
            .select_for_destination("BE", true, Some(&other_destination))
            .await
            .unwrap()
            .authority(),
        fastest.authority()
    );
    assert_eq!(
        inventory.select("BE", true).await.unwrap().authority(),
        fastest.authority()
    );

    validated.destination_tls = ProxyCapabilityStatus::Available;
    inventory.add_or_update(vec![validated.clone()]).await;
    assert_eq!(
        inventory
            .select_for_destination("BE", true, Some(&other_destination))
            .await
            .unwrap()
            .authority(),
        validated.authority()
    );
    assert_eq!(
        inventory.select("BE", false).await.unwrap().authority(),
        fastest.authority()
    );

    let mut unknown = record(ProxyRuntimeStatus::Ok);
    unknown.host = "unknown.example".to_string();
    unknown.latency_ms = Some(1);
    unknown.https_tunnel = ProxyCapabilityStatus::Unknown;
    inventory.add_or_update(vec![unknown]).await;
    assert_eq!(
        inventory.select("BE", true).await.unwrap().authority(),
        validated.authority()
    );
    let mut other_country = record(ProxyRuntimeStatus::Ok);
    other_country.host = "accepted-fr.example".into();
    other_country.country = Some("FR".into());
    inventory.add_or_update(vec![other_country.clone()]).await;
    inventory
        .record_validated_response(&other_country.authority(), &preferred_destination)
        .await;
    for countries in [
        vec!["BE".into(), "FR".into()],
        vec!["FR".into(), "BE".into()],
    ] {
        assert_eq!(
            inventory
                .select_any_for_destination(&countries, true, Some(&preferred_destination))
                .await
                .unwrap()
                .authority(),
            other_country.authority()
        );
    }
    assert_eq!(
        inventory
            .select_for_destination("BE", true, Some(&preferred_destination))
            .await
            .unwrap()
            .authority(),
        validated.authority()
    );
}

#[tokio::test]
async fn validated_proxy_preference_never_bypasses_eligibility() {
    let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
    let mut fallback = record(ProxyRuntimeStatus::Ok);
    fallback.host = "fallback.example".to_string();
    fallback.latency_ms = Some(50);
    let mut validated = record(ProxyRuntimeStatus::Ok);
    validated.host = "validated.example".to_string();
    validated.latency_ms = Some(5);
    let validated_authority = validated.authority();
    inventory
        .add_or_update(vec![fallback.clone(), validated])
        .await;
    let destination = Destination::host_port("blocked.example", 443)
        .with_protocol(crate::core::ApplicationProtocol::Https);
    inventory
        .record_validated_response(&validated_authority, &destination)
        .await;
    for certificate in [
        ProxyCapabilityStatus::Unavailable,
        ProxyCapabilityStatus::Available,
    ] {
        inventory
            .inner
            .write()
            .await
            .records
            .get_mut(&validated_authority)
            .unwrap()
            .proxy_tls_certificate = certificate;
        let selected = inventory
            .select_for_destination("BE", true, Some(&destination))
            .await
            .unwrap();
        assert_eq!(
            selected.authority(),
            if certificate == ProxyCapabilityStatus::Unavailable {
                fallback.authority()
            } else {
                validated_authority.clone()
            }
        );
    }
    let other_destination = Destination::host_port("other.example", 443)
        .with_protocol(crate::core::ApplicationProtocol::Https);
    for target in [&destination, &other_destination] {
        inventory
            .record_destination_failure(
                &validated_authority,
                target,
                crate::core::ProxyDestinationFailureReason::Other,
            )
            .await;
    }
    assert_eq!(
        inventory
            .select_for_destination("BE", true, Some(&destination))
            .await
            .unwrap()
            .authority(),
        fallback.authority()
    );
    inventory
        .record_validated_response(&validated_authority, &destination)
        .await;
    assert_eq!(
        inventory
            .select_for_destination("BE", true, Some(&destination))
            .await
            .unwrap()
            .authority(),
        validated_authority
    );
    assert_eq!(
        inventory
            .select_for_destination("BE", true, Some(&other_destination))
            .await
            .unwrap()
            .authority(),
        fallback.authority()
    );
    inventory
        .record_destination_failure(
            &validated_authority,
            &destination,
            crate::core::ProxyDestinationFailureReason::Other,
        )
        .await;
    {
        let mut inner = inventory.inner.write().await;
        let record = inner.records.get_mut(&validated_authority).unwrap();
        record
            .destination_failures
            .iter_mut()
            .find(|failure| failure.host == "blocked.example")
            .unwrap()
            .last_failed = SystemTime::now() + Duration::from_secs(60);
    }
    inventory
        .record_validated_response(&validated_authority, &destination)
        .await;
    assert_eq!(
        inventory
            .select_for_destination("BE", true, Some(&destination))
            .await
            .unwrap()
            .authority(),
        fallback.authority(),
        "acceptance must preserve newer failures"
    );
    inventory
        .record_destination_failure(
            &validated_authority,
            &destination,
            crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
        )
        .await;
    inventory
        .record_validated_response(&validated_authority, &destination)
        .await;

    assert_eq!(
        inventory
            .select_for_destination("BE", true, Some(&destination))
            .await
            .unwrap()
            .authority(),
        fallback.authority()
    );

    inventory.record_global_failure(&validated_authority).await;
    inventory
        .record_validated_response(&validated_authority, &destination)
        .await;
    assert_eq!(
        inventory.select("BE", true).await.unwrap().authority(),
        fallback.authority()
    );
}
