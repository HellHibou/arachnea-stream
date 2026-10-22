use super::*;

#[tokio::test]
async fn validated_proxy_is_preferred_only_for_its_destination() {
    {
        let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
        let mut near = record(ProxyRuntimeStatus::Ok);
        near.host = "near.example".into();
        near.destination_tls = ProxyCapabilityStatus::Available;
        let mut far = near.clone();
        far.host = "far.example".into();
        let mut grouped = near.clone();
        grouped.host = "grouped.example".into();
        inventory
            .add_or_update(vec![near.clone(), far.clone(), grouped.clone()])
            .await;
        let target = Destination::host_port("abcd.mediainfo.tf1.fr", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        let near_origin = Destination::host_port("mediainfo.tf1.fr", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        let far_origin = Destination::host_port("www.tf1.fr", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        let group_origin = Destination::host_port("android.middleware.6play.fr", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        inventory
            .record_validated_response(&near.authority(), &near_origin)
            .await;
        tokio::time::sleep(Duration::from_millis(2)).await;
        inventory
            .record_validated_response(&far.authority(), &far_origin)
            .await;
        inventory
            .record_validated_response(&grouped.authority(), &group_origin)
            .await;
        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&target))
                .await
                .unwrap()
                .authority(),
            near.authority(),
            "nearest suffix precedes a more recent broad suffix"
        );
        let group = vec!["tf1.fr".into(), "6play.fr".into()];
        assert_eq!(
            inventory
                .select_any_for_destination_with_domain_group(
                    &["BE".into()],
                    true,
                    Some(&target),
                    None,
                    &group
                )
                .await
                .unwrap()
                .authority(),
            grouped.authority(),
            "configured group precedes hierarchy"
        );
        let unrelated_group = vec!["m6.fr".into(), "6play.fr".into()];
        assert_eq!(
            inventory
                .select_any_for_destination_with_domain_group(
                    &["BE".into()],
                    true,
                    Some(&target),
                    None,
                    &unrelated_group
                )
                .await
                .unwrap()
                .authority(),
            near.authority(),
            "a group must contain the target domain"
        );
        inventory
            .record_destination_failure(
                &near.authority(),
                &target,
                crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
            )
            .await;
        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&target))
                .await
                .unwrap()
                .authority(),
            far.authority(),
            "ineligible nearer candidates allow broader suffixes"
        );
        inventory
            .record_destination_failure(
                &grouped.authority(),
                &target,
                crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
            )
            .await;
        assert_eq!(
            inventory
                .select_any_for_destination_with_domain_group(
                    &["BE".into()],
                    true,
                    Some(&target),
                    None,
                    &group
                )
                .await
                .unwrap()
                .authority(),
            far.authority(),
            "group exclusions preserve hierarchical fallback"
        );
        let exact_target = Destination::host_port("second.mediainfo.tf1.fr", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        inventory
            .record_validated_response(&near.authority(), &exact_target)
            .await;
        assert_eq!(
            inventory
                .select_any_for_destination_with_domain_group(
                    &["BE".into()],
                    true,
                    Some(&exact_target),
                    None,
                    &group
                )
                .await
                .unwrap()
                .authority(),
            near.authority(),
            "exact acceptance keeps priority over the group"
        );
        let other = Destination::host_port("api.bedrock.tech", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        let m6_group = vec![
            "m6.fr".into(),
            "6play.fr".into(),
            "6cloud.fr".into(),
            "bedrock.tech".into(),
        ];
        assert_eq!(
            inventory
                .select_related_origin(&["BE".into()], true, Some(&other), &m6_group)
                .await
                .unwrap()
                .0
                .authority(),
            grouped.authority()
        );
        assert!(inventory
            .select_related_origin(&["FR".into()], true, Some(&other), &m6_group)
            .await
            .is_none());
        let deceptive = Destination::host_port("evilbedrock.tech", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        assert!(inventory
            .select_related_origin(&["BE".into()], true, Some(&deceptive), &m6_group)
            .await
            .is_none());
        let http = other
            .clone()
            .with_protocol(crate::core::ApplicationProtocol::Http);
        assert!(inventory
            .select_related_origin(&["BE".into()], false, Some(&http), &m6_group)
            .await
            .is_none());
    }
    for (source, target, related) in [
        ("www.tf1.fr", "mediainfo.tf1.fr", true),
        ("WWW.TF1.FR.", "compte.tf1.fr", true),
        ("front-auth.6cloud.fr", "drm.6cloud.fr", true),
        ("android.middleware.6play.fr", "www.6play.fr", true),
        ("a.example.co.uk", "b.example.co.uk", true),
        ("a.example.co.uk", "b.other.co.uk", true),
        ("alice.github.io", "bob.github.io", true),
        ("one.alice.github.io", "two.alice.github.io", true),
        ("www.6play.fr", "front-auth.6cloud.fr", false),
        ("www.m6.fr", "www.6play.fr", false),
        ("a.example", "b.example", false),
        ("a.localhost", "b.localhost", false),
        ("127.0.0.1", "127.0.0.2", false),
        ("co.uk", "example.co.uk", true),
        ("-invalid.tf1.fr", "www.tf1.fr", false),
        ("a.bücher.de", "b.xn--bcher-kva.de", true),
        ("a.project.internal", "b.project.internal", true),
        ("a.tf1.fr", "b.othertf1.fr", false),
    ] {
        let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
        let mut ordinary = record(ProxyRuntimeStatus::Ok);
        ordinary.host = "ordinary.example".into();
        ordinary.latency_ms = Some(1);
        ordinary.destination_tls = ProxyCapabilityStatus::Available;
        let mut accepted = ordinary.clone();
        accepted.host = "accepted.example".into();
        accepted.latency_ms = Some(100);
        inventory
            .add_or_update(vec![ordinary.clone(), accepted.clone()])
            .await;
        let source = Destination::host_port(source, 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        let target = Destination::host_port(target, 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        inventory
            .record_validated_response(&accepted.authority(), &source)
            .await;
        let search = ProxySearchContext::default();
        let selected = search
            .scope(inventory.select_for_destination("BE", true, Some(&target)))
            .await
            .unwrap();
        assert_eq!(
            selected.authority(),
            if related {
                accepted.authority()
            } else {
                ordinary.authority()
            },
            "{source} -> {target}"
        );
        assert_eq!(
            search.selection_provenance(&selected.authority(), &target),
            crate::core::ProxySelectionProvenance::Discovery
        );
        assert!(!inventory
            .inner
            .read()
            .await
            .preferred_by_destination
            .contains_key(&crate::core::ProxyDestinationPreference::destination_key(
                &target
            )));
        if related {
            accepted.last_checked = Some(
                SystemTime::now() - InventoryConfig::default().probe_ttl - Duration::from_secs(1),
            );
            inventory.add_or_update(vec![accepted.clone()]).await;
            assert_eq!(
                inventory
                    .select_for_destination("BE", true, Some(&target))
                    .await
                    .unwrap()
                    .authority(),
                accepted.authority(),
                "probe age alone must not invalidate a related-origin hint"
            );
            for separated in [
                Destination::host_port(&target.host_for_protocol(), 8443)
                    .with_protocol(crate::core::ApplicationProtocol::Https),
                Destination::host_port(&target.host_for_protocol(), 443)
                    .with_protocol(crate::core::ApplicationProtocol::Http),
            ] {
                assert!(inventory
                    .select_related_origin(&["BE".into()], true, Some(&separated), &[])
                    .await
                    .is_none());
            }
            assert!(inventory
                .select_related_origin(&["FR".into()], true, Some(&target), &[])
                .await
                .is_none());
            inventory
                .record_destination_failure(
                    &accepted.authority(),
                    &target,
                    crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
                )
                .await;
            inventory
                .record_validated_response(&accepted.authority(), &source)
                .await;
            assert!(
                inventory
                    .select_related_origin(&["BE".into()], true, Some(&target), &[])
                    .await
                    .is_none(),
                "sibling success must not clear target exclusion"
            );
        }
    }
    {
        let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
        let mut older = record(ProxyRuntimeStatus::Ok);
        older.host = "older.example".into();
        older.destination_tls = ProxyCapabilityStatus::Available;
        let mut newer = older.clone();
        newer.host = "newer.example".into();
        inventory
            .add_or_update(vec![older.clone(), newer.clone()])
            .await;
        let source = Destination::host_port("www.tf1.fr", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        let target = Destination::host_port("mediainfo.tf1.fr", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https);
        inventory
            .record_validated_response(&older.authority(), &source)
            .await;
        tokio::time::sleep(Duration::from_millis(2)).await;
        inventory
            .record_validated_response(&newer.authority(), &source)
            .await;
        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&target))
                .await
                .unwrap()
                .authority(),
            newer.authority()
        );
        let search = ProxySearchContext::default();
        search.start_origin(&newer.authority(), &target).unwrap();
        assert_eq!(
            search
                .scope(inventory.select_for_destination("BE", true, Some(&target)))
                .await
                .unwrap()
                .authority(),
            older.authority(),
            "already tried sibling candidates must rotate"
        );
        inventory
            .bind_affinity(Some("explicit-priority"), &older)
            .await;
        assert_eq!(
            inventory
                .select_any_for_destination_with_affinity(
                    &["BE".into()],
                    true,
                    Some(&target),
                    Some("explicit-priority")
                )
                .await
                .unwrap()
                .authority(),
            older.authority()
        );
        let token = crate::core::ProxySelectionToken::new(
            Arc::new(inventory.clone()),
            older.authority(),
            target.clone(),
        );
        token.record_redirect(302).await;
        assert_eq!(
            inventory
                .select_for_destination("BE", true, Some(&target))
                .await
                .unwrap()
                .authority(),
            older.authority(),
            "exact redirect evidence precedes related history"
        );
        inventory
            .record_validated_response(&older.authority(), &target)
            .await;
        let exact = ProxySearchContext::default();
        let selected = exact
            .scope(inventory.select_for_destination("BE", true, Some(&target)))
            .await
            .unwrap();
        assert_eq!(
            selected.authority(),
            older.authority(),
            "exact preference precedes newer related history"
        );
        assert_eq!(
            exact.selection_provenance(&selected.authority(), &target),
            crate::core::ProxySelectionProvenance::AcceptedDestination
        );
        for state in [
            "unknown_capability",
            "global_cooldown",
            "authentication",
            "unavailable_tls",
        ] {
            let mut ineligible = newer.clone();
            match state {
                "unknown_capability" => ineligible.destination_tls = ProxyCapabilityStatus::Unknown,
                "global_cooldown" => {
                    ineligible.cooldown_until = Some(SystemTime::now() + Duration::from_secs(30))
                }
                "authentication" => ineligible.authentication_required = Some(true),
                _ => ineligible.destination_tls = ProxyCapabilityStatus::Unavailable,
            }
            let mut inner = inventory.inner.write().await;
            inner.records.insert(newer.authority(), ineligible);
            drop(inner);
            let candidate = inventory
                .select_related_origin(&["BE".into()], true, Some(&target), &[])
                .await
                .unwrap();
            assert_eq!(candidate.0.authority(), older.authority(), "{state}");
        }
    }
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
    let preferred_started = Instant::now();
    assert_eq!(
        inventory
            .select_any_for_destination(&["BE".to_string()], true, Some(&preferred_destination))
            .await
            .unwrap()
            .authority(),
        validated.authority()
    );
    eprintln!("lot0 preferred memory: select_us={}, inventory_records=2, provider_calls=0, probe_configured=false", preferred_started.elapsed().as_micros());

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
        let select_started = Instant::now();
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
        eprintln!(
            "lot0 eligibility selection: select_us={}, preferred_rejected={}",
            select_started.elapsed().as_micros(),
            certificate == ProxyCapabilityStatus::Unavailable
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
