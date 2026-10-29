use super::*;

#[cfg(feature = "persistence")]
#[tokio::test]
async fn provider_reload_persists_post_merge_runtime_exclusion() -> AnyResult<()> {
    let store = memory_proxy_store()?;
    let repository = Arc::new(TypedProxyRepository::new(Arc::clone(&store)));
    let provider = Arc::new(StaticProvider {
        records: vec![record(ProxyRuntimeStatus::Ok)],
    });
    let inventory = ProxyInventory::new(InventoryConfig::default(), Some(provider), None)
        .with_proxy_repository(repository);

    let mut retained = record(ProxyRuntimeStatus::Ko);
    retained.health_evidence.first_discovered_at = Some(SystemTime::now());
    retained.failure_count = 3;
    retained.authentication_required = Some(true);
    retained.last_validated_at = Some(SystemTime::now() - PROXY_CACHE_TTL - Duration::from_secs(1));
    retained.cooldown_until = Some(SystemTime::now() + Duration::from_secs(60));
    retained.destination_failures = vec![ProxyDestinationFailure {
        scheme: "https".to_string(),
        host: "origin.example".to_string(),
        port: 443,
        reason: crate::core::ProxyDestinationFailureReason::BlockedByOrigin,
        failure_count: 2,
        last_failed: SystemTime::now(),
        cooldown_until: None,
    }];
    let key = retained.key();
    inventory.add_or_update(vec![retained.clone()]).await;

    inventory.load_if_needed("BE").await;

    assert_eq!(store.get(&key).await?, Some(retained));
    Ok(())
}

#[tokio::test]
async fn concurrent_country_loads_wait_for_one_provider_refresh() -> AnyResult<()> {
    let provider = Arc::new(CountingProvider {
        records: vec![record(ProxyRuntimeStatus::Ok)],
        calls: AtomicUsize::new(0),
        delay: Duration::from_millis(50),
    });
    let inventory = Arc::new(ProxyInventory::new(
        InventoryConfig {
            reserve_timeout: Duration::from_millis(200),
            ..InventoryConfig::default()
        },
        Some(provider.clone()),
        None,
    ));

    let first = {
        let inventory = Arc::clone(&inventory);
        tokio::spawn(async move {
            let search = ProxySearchContext::new(crate::core::ProxySearchLimits {
                target: Duration::from_millis(10),
                extension: Duration::ZERO,
            });
            search.scope(inventory.select("BE", true)).await
        })
    };
    let second = {
        let inventory = Arc::clone(&inventory);
        tokio::spawn(async move { inventory.select("BE", true).await })
    };

    let started = Instant::now();
    assert!(first.await?.is_err());
    assert!(second.await??.supports_https.unwrap_or(false));
    eprintln!(
        "lot7 shared provider: survivor_wait_us={}, provider_calls={}",
        started.elapsed().as_micros(),
        provider.calls.load(Ordering::SeqCst)
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

    let limits = crate::core::ProxySearchLimits::default();
    assert_eq!(limits.target, Duration::from_secs(120));
    assert_eq!(limits.extension, Duration::from_secs(15));
    let search = ProxySearchContext::new(limits);
    let hop = search.for_url("https://redirect.example/path");
    assert_eq!(hop.id(), search.id());
    for index in 0..201 {
        assert!(hop.reserve(false, &format!("tcp:{index}")));
    }
    assert!(!search.reserve(false, "tcp:0"));
    for index in 0..201 {
        assert!(search.reserve(true, &format!("validate:{index}")));
    }
    assert!(!hop.reserve(true, "validate:0"));
    let destination = Destination::host_port("origin.example", 443);
    for index in 0..201 {
        search.start_origin(&format!("endpoint:{index}"), &destination)?;
        search.end_attempt();
    }
    assert!(search.start_origin("endpoint:0", &destination).is_err());
    assert!(search.attempted("endpoint:200", Some(&destination)));
    assert!(!search.attempted("endpoint:201", Some(&destination)));
    hop.start_origin("endpoint:0", &destination)?;
    hop.end_attempt();
    assert!(hop.start_origin("endpoint:0", &destination).is_err());
    assert!(search.can_discover());
    let configured = crate::core::TimeoutConfig::default();
    let localized = search.connection_timeouts("endpoint:0", &configured);
    assert_eq!(localized.connect_ms, 3_000);
    assert_eq!(localized.handshake_ms, 5_000);
    assert_eq!(localized.idle_ms, configured.idle_ms);
    assert_eq!(
        search.connection_timeouts("ordinary", &configured),
        configured
    );
    let shorter = crate::core::TimeoutConfig {
        connect_ms: 100,
        handshake_ms: 200,
        idle_ms: 300,
    };
    assert_eq!(hop.connection_timeouts("endpoint:0", &shorter), shorter);
    let known = ProxySearchContext::default();
    known.mark_selection(
        "known:0",
        &destination,
        crate::core::ProxySelectionProvenance::AcceptedDestination,
    );
    known.start_origin("known:0", &destination)?;
    let reused = known.connection_timeouts("known:0", &configured);
    assert_eq!(reused.connect_ms, 30_000);
    assert_eq!(reused.handshake_ms, 30_000);
    assert_eq!(reused.idle_ms, configured.idle_ms);
    assert_eq!(known.connection_timeouts("known:0", &shorter), shorter);
    assert_eq!(known.connection_timeout(), Duration::from_secs(60));
    assert_eq!(search.connection_timeout(), Duration::from_secs(15));
    known.end_attempt();
    known.start_origin("fresh:0", &destination)?;
    assert_eq!(known.connection_timeout(), Duration::from_secs(15));
    for validation in [false, true] {
        let caller_task = tokio::task::try_id();
        assert!(search
            .scope(
                inventory.shared_operation(
                    format!("isolated:{validation}"),
                    validation,
                    async move {
                        assert_ne!(
                            tokio::task::try_id(),
                            caller_task,
                            "probe work must not extend the caller's polling stack"
                        );
                        assert!(ProxySearchContext::current().is_none());
                        Some(record(ProxyRuntimeStatus::Ok))
                    }
                    .boxed(),
                )
            )
            .await
            .is_some());
        for reserve_size in [0, 1, 2] {
            let inventory = Arc::new(ProxyInventory::new(
                InventoryConfig {
                    reserve_size,
                    reserve_timeout: Duration::from_millis(200),
                    ..InventoryConfig::default()
                },
                None,
                None,
            ));
            inventory
                .add_or_update(vec![record(ProxyRuntimeStatus::Ok)])
                .await;
            for retained in [false, true] {
                let work_search = if retained {
                    ProxySearchContext::default()
                } else {
                    search.clone()
                };
                let owner = work_search.continuation.as_ref().unwrap();
                owner.set_route(
                    vec!["BE".into()],
                    true,
                    retained.then(|| destination.clone()),
                );
                let key = format!("cancelled:{validation}:{retained}:{reserve_size}");
                let mut acceptance_us = None;
                let mut accepted_at = None;
                let slots = if validation {
                    inventory.validation_slots.clone()
                } else {
                    inventory.tcp_slots.clone()
                };
                let available = slots.available_permits();
                let operation_slots = slots.clone();
                let (started, running) = tokio::sync::oneshot::channel();
                {
                    let pending = work_search.scope(
                        inventory.shared_operation(
                            key.clone(),
                            validation,
                            async move {
                                let _slot = operation_slots.acquire().await.ok()?;
                                let _ = started.send(());
                                futures::future::pending().await
                            }
                            .boxed(),
                        ),
                    );
                    tokio::pin!(pending);
                    tokio::select! {
                        _ = &mut pending => panic!("fixture work must remain pending"),
                        result = running => result.expect("owned probe task must start"),
                    }
                    assert!(!work_search.reserve(validation, &key));
                }
                if retained {
                    assert_eq!(
                        slots.available_permits(),
                        available - 1,
                        "engaged work survives selection return"
                    );
                    assert!(!work_search.reserve(validation, &key));
                    let token = work_search
                        .scope(async {
                            crate::core::ProxySelectionToken::new(
                                inventory.clone(),
                                record(ProxyRuntimeStatus::Ok).authority(),
                                destination.clone(),
                            )
                        })
                        .await;
                    let acceptance_started = Instant::now();
                    tokio::time::timeout(Duration::from_secs(1), token.record_accepted())
                        .await
                        .expect("acceptance must not await pending reserve work");
                    acceptance_us = Some(acceptance_started.elapsed().as_micros());
                    accepted_at = Some(Instant::now());
                    owner.cancel();
                    tokio::task::yield_now().await;
                    if reserve_size > 0 {
                        assert_eq!(
                            slots.available_permits(),
                            available - 1,
                            "acceptance transfers subscriptions away from the request owner"
                        );
                    }
                }
                tokio::time::timeout(Duration::from_secs(1), async {
                    while slots.available_permits() != available {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("last-subscriber cancellation must release probe slots");
                if let Some(accepted_at) = accepted_at {
                    eprintln!(
                    "lot1 retained subscription: reserve_size={reserve_size}, validation={validation}, acceptance_us={}, slot_release_after_acceptance_us={}",
                    acceptance_us.unwrap(),
                    accepted_at.elapsed().as_micros()
                );
                }
                assert!(work_search.reserve(validation, &key));
                owner.set_route(Vec::new(), true, None);
            }
        }
    }
    for validation in [false, true] {
        for cancel_last in [false, true] {
            let inventory = Arc::new(ProxyInventory::new(InventoryConfig::default(), None, None));
            let slots = if validation {
                inventory.validation_slots.clone()
            } else {
                inventory.tcp_slots.clone()
            };
            let available = slots.available_permits();
            let executions = Arc::new(AtomicUsize::new(0));
            let executed = executions.clone();
            let release = Arc::new(tokio::sync::Notify::new());
            let worker_release = release.clone();
            let worker_inventory = inventory.clone();
            let (started, running) = tokio::sync::oneshot::channel();
            let first_search = ProxySearchContext::default();
            let second_search = ProxySearchContext::default();
            let key = format!("shared-cancel:{validation}:{cancel_last}");
            let mut first = Box::pin(
                first_search.scope(
                    inventory.shared_operation(
                        key.clone(),
                        validation,
                        async move {
                            let _permit = worker_inventory
                                .acquire_probe_slot(validation, false)
                                .await?;
                            executed.fetch_add(1, Ordering::SeqCst);
                            let _ = started.send(());
                            worker_release.notified().await;
                            Some(record(ProxyRuntimeStatus::Ok))
                        }
                        .boxed(),
                    ),
                ),
            );
            tokio::select! {
                _ = &mut first => panic!("shared probe must stay suspended"),
                result = running => result.expect("shared probe must acquire its permit"),
            }
            let mut second = Box::pin(second_search.scope(inventory.shared_operation(
                key,
                validation,
                async { panic!("a second subscriber must not execute another probe") }.boxed(),
            )));
            assert!(futures::poll!(&mut second).is_pending());
            drop(first);
            tokio::task::yield_now().await;
            assert_eq!(executions.load(Ordering::SeqCst), 1);
            assert_eq!(slots.available_permits(), available - 1);
            if cancel_last {
                drop(second);
            } else {
                release.notify_one();
                assert!(second.await.is_some());
            }
            tokio::time::timeout(Duration::from_secs(1), async {
                while slots.available_permits() != available {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("the last subscriber must release the shared network permit");
            eprintln!("lot1 probe cancellation: validation={validation}, cancel_last={cancel_last}, executions=1, permits_before={available}, permits_after_first_drop={}, permits_after_last={}", available - 1, slots.available_permits());
        }
    }
    for (validation, slots, spare) in [
        (false, inventory.tcp_slots.clone(), 8),
        (true, inventory.validation_slots.clone(), 2),
    ] {
        let held = slots.clone().try_acquire_many_owned(spare)?;
        assert!(
            inventory
                .acquire_probe_slot(validation, true)
                .await
                .is_none(),
            "speculative work must leave capacity for active searches"
        );
        drop(held);
        let available = slots.available_permits();
        let admitted = inventory
            .acquire_probe_slot(validation, true)
            .await
            .unwrap();
        assert_eq!(slots.available_permits(), available - 1);
        drop(admitted);
        let mut speculative = Vec::new();
        while let Some(permit) = inventory.acquire_probe_slot(validation, true).await {
            speculative.push(permit);
        }
        assert_eq!(speculative.len(), available - spare as usize);
        assert_eq!(slots.available_permits(), spare as usize);
        let mut active = Vec::new();
        for _ in 0..spare {
            active.push(
                inventory
                    .acquire_probe_slot(validation, false)
                    .await
                    .unwrap(),
            );
        }
        assert_eq!(slots.available_permits(), 0);
        let mut queued = Box::pin(inventory.acquire_probe_slot(validation, false));
        assert!(futures::poll!(&mut queued).is_pending());
        assert!(inventory
            .acquire_probe_slot(validation, true)
            .await
            .is_none());
        drop(active.pop());
        let resumed = tokio::time::timeout(Duration::from_secs(1), queued)
            .await
            .expect("an active search must resume when a saturated slot is freed")
            .expect("the inventory semaphore stays open");
        assert_eq!(slots.available_permits(), 0);
        drop(resumed);
        drop(active);
        let speculative_peak = speculative.len();
        drop(speculative);
        assert_eq!(slots.available_permits(), available);
        eprintln!("lot1 slot saturation: validation={validation}, speculative_peak={speculative_peak}, active_spare={spare}, total_peak={available}, queued_active_resumed=true, permits_after={}", slots.available_permits());
    }
    let tcp = search.tcp_slots.try_acquire_many(8)?;
    assert!(hop.tcp_slots.try_acquire().is_err());
    drop(tcp);
    let validation = search.validation_slots.try_acquire_many(3)?;
    assert!(hop.validation_slots.try_acquire().is_err());
    drop(validation);
    {
        let inventory = Arc::new(ProxyInventory::new(InventoryConfig::default(), None, None));
        let first_search = ProxySearchContext::default();
        let second_search = ProxySearchContext::default();
        let (started, mut running) = tokio::sync::mpsc::unbounded_channel();
        let mut first = Box::pin(futures::future::join_all((0..4).map(|index| {
            let inventory = inventory.clone();
            let worker_inventory = inventory.clone();
            let started = started.clone();
            first_search.scope(async move {
                inventory
                    .shared_operation(
                        format!("local-validation-limit:{index}"),
                        true,
                        async move {
                            let _permit = worker_inventory.acquire_probe_slot(true, false).await?;
                            let _ = started.send(index);
                            futures::future::pending().await
                        }
                        .boxed(),
                    )
                    .await
            })
        })));
        tokio::time::timeout(Duration::from_secs(1), async {
            for _ in 0..3 {
                tokio::select! {
                    _ = &mut first => panic!("validation work must stay suspended"),
                    result = running.recv() => assert!(result.is_some()),
                }
            }
        })
        .await
        .expect("three validations must start without waiting for another to finish");
        assert_eq!(first_search.validation_slots.available_permits(), 0);
        assert_eq!(inventory.validation_slots.available_permits(), 1);
        assert!(
            running.try_recv().is_err(),
            "a fourth local validation must wait"
        );

        let worker_inventory = inventory.clone();
        let (started, running) = tokio::sync::oneshot::channel();
        let mut second = Box::pin(
            second_search.scope(
                inventory.shared_operation(
                    "other-search-validation".into(),
                    true,
                    async move {
                        let _permit = worker_inventory.acquire_probe_slot(true, false).await?;
                        let _ = started.send(());
                        futures::future::pending().await
                    }
                    .boxed(),
                ),
            ),
        );
        tokio::time::timeout(Duration::from_secs(1), async {
            tokio::select! {
                _ = &mut second => panic!("the other search must stay suspended"),
                result = running => result.expect("the other search must use the remaining shared slot"),
            }
        })
        .await?;
        assert_eq!(inventory.validation_slots.available_permits(), 0);
        let mut queued = Box::pin(inventory.acquire_probe_slot(true, false));
        assert!(futures::poll!(&mut queued).is_pending());
        drop(first);
        let resumed = tokio::time::timeout(Duration::from_secs(1), queued)
            .await
            .expect("cancelling a saturated search must unblock shared validation")
            .expect("the inventory semaphore stays open");
        drop(resumed);
        drop(second);
        tokio::time::timeout(Duration::from_secs(1), async {
            while inventory.validation_slots.available_permits() != 4
                || first_search.validation_slots.available_permits() != 3
                || second_search.validation_slots.available_permits() != 3
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cancellation must release local and shared validation permits");
    }
    let country = search.provider_slots.try_acquire_many(2)?;
    assert!(hop.provider_slots.try_acquire().is_err());
    drop(country);
    let queued = search.tcp_slots.acquire_many(8).await?;
    let waiter = hop.tcp_slots.acquire();
    let wait_started = Instant::now();
    let (permit, ()) = tokio::join!(waiter, async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(queued);
    });
    drop(permit?);
    eprintln!("lot7 slot wait: wait_us={}, tcp_limit=8, validation_limit=3, country_limit=2, prechecks=201, validations=201, endpoints=201, work_count_limit=none", wait_started.elapsed().as_micros());

    for engaged in [false, true] {
        let timed = ProxySearchContext::new(crate::core::ProxySearchLimits {
            target: Duration::from_millis(30),
            extension: Duration::from_millis(60),
        });
        if engaged {
            timed.start_origin("endpoint", &destination)?;
        }
        let deadline_started = Instant::now();
        let result: Result<()> = tokio::time::timeout(Duration::from_secs(1), async {
            if engaged {
                timed.origin(futures::future::pending()).await
            } else {
                timed.technical(futures::future::pending()).await
            }
        })
        .await
        .expect("search deadlines must stop pending work");
        assert!(deadline_started.elapsed() >= Duration::from_millis(if engaged { 90 } else { 30 }));
        assert!(matches!(result, Err(ProxyError::Timeout(_))));
        assert!(!timed.can_start());
        assert!(timed.start_origin("new-endpoint", &destination).is_err());
        assert!(timed.technical(async {}).await.is_err());
    }
    let margin = ProxySearchContext::new(crate::core::ProxySearchLimits {
        target: Duration::from_millis(30),
        extension: Duration::from_millis(60),
    });
    margin.start_origin("endpoint", &destination)?;
    margin
        .origin(async {
            tokio::time::sleep(Duration::from_millis(40)).await;
            assert!(!margin.can_start());
            assert!(margin.start_origin("new-endpoint", &destination).is_err());
            assert!(margin.technical(async {}).await.is_err());
        })
        .await?;
    let accepted = ProxySearchContext::new(crate::core::ProxySearchLimits {
        target: Duration::from_millis(30),
        extension: Duration::from_millis(60),
    });
    accepted.start_origin("endpoint", &destination)?;
    accepted
        .origin(async {
            accepted.accept();
            tokio::time::sleep(Duration::from_millis(110)).await;
        })
        .await?;
    assert!(!accepted.can_start());
    use crate::core::{proxy_retry_allowed, ProxyRequestDelivery};
    for method in ["GET", "HEAD", "POST"] {
        assert!(proxy_retry_allowed(
            method,
            false,
            ProxyRequestDelivery::NotSent,
            false
        ));
        assert_eq!(
            proxy_retry_allowed(method, false, ProxyRequestDelivery::PossiblySent, false),
            method != "POST"
        );
        assert!(!proxy_retry_allowed(
            method,
            true,
            ProxyRequestDelivery::ResponseReceived,
            false
        ));
        assert!(proxy_retry_allowed(
            method,
            true,
            ProxyRequestDelivery::ResponseReceived,
            true
        ));
    }
    Ok(())
}

#[tokio::test]
async fn concurrent_country_loads_share_one_provider_failure() -> AnyResult<()> {
    let provider = Arc::new(FailingProvider {
        calls: AtomicUsize::new(0),
        delay: Duration::from_millis(50),
    });
    let inventory = Arc::new(ProxyInventory::new(
        InventoryConfig::default(),
        Some(provider.clone()),
        None,
    ));

    let first = {
        let inventory = Arc::clone(&inventory);
        tokio::spawn(async move { inventory.select("BE", true).await })
    };
    let second = {
        let inventory = Arc::clone(&inventory);
        tokio::spawn(async move { inventory.select("BE", true).await })
    };

    assert!(first.await?.is_err());
    assert!(second.await?.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(inventory.select("BE", true).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

    struct PartialFailure(AtomicUsize);
    #[async_trait]
    impl ProxyDataProvider for PartialFailure {
        async fn load_proxies(&self, _: ProxyLoadRequest) -> Result<Vec<ProxyRecord>> {
            unreachable!("partial batches use the progressive adapter")
        }
        async fn load_proxies_progressively(
            &self,
            _: ProxyLoadRequest,
            sink: Arc<dyn ProxyBatchSink>,
        ) -> Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            sink.publish(vec![record(ProxyRuntimeStatus::Ok)]).await?;
            Err(ProxyError::Config(
                "fixture failure after publication".into(),
            ))
        }
    }
    let partial = Arc::new(PartialFailure(AtomicUsize::new(0)));
    let inventory = ProxyInventory::new(InventoryConfig::default(), Some(partial.clone()), None);
    inventory.load_if_needed("BE").await;
    assert_eq!(
        inventory.records_for_country("BE").await.len(),
        1,
        "a failed refresh must retain records published before its error"
    );
    inventory.load_if_needed("BE").await;
    assert_eq!(
        partial.0.load(Ordering::SeqCst),
        1,
        "partial errors must preserve the existing refresh-attempt cooldown"
    );
    Ok(())
}

#[tokio::test]
async fn successive_selections_share_provider_refresh_cooldown() {
    let mut http_only = record(ProxyRuntimeStatus::Ok);
    http_only.protocol = Some(ProxyProtocol::Http);
    http_only.supports_https = Some(false);
    http_only.https_tunnel = ProxyCapabilityStatus::Unavailable;
    let provider = Arc::new(CountingProvider {
        records: vec![http_only],
        calls: AtomicUsize::new(0),
        delay: Duration::ZERO,
    });
    let inventory = ProxyInventory::new(
        InventoryConfig {
            provider_refresh_cooldown: Duration::from_secs(60),
            ..InventoryConfig::default()
        },
        Some(provider.clone()),
        None,
    );

    assert!(inventory.select("BE", true).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(inventory.select("BE", true).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

    let destination = Destination::host_port("origin.example", 443)
        .with_protocol(crate::core::ApplicationProtocol::Https);
    let search = ProxySearchContext::default();
    let origin = search.for_refresh_destination(Some(&destination));
    let exploration = Duration::from_secs(60);
    let (deadline, ready) = origin.provider_refresh_progress(exploration, 8);
    assert!(!ready);
    for index in 0..8 {
        let retry = search
            .for_url(&format!("https://origin.example/path/{index}"))
            .for_refresh_destination(Some(&destination));
        assert!(retry
            .scope(inventory.shared_operation(
                format!("refresh-failure:{index}"),
                true,
                async { None }.boxed(),
            ))
            .await
            .is_none());
        assert_eq!(
            retry.provider_refresh_progress(exploration, 8),
            (deadline, index == 7),
            "retries must retain both the origin clock and completed validation failures"
        );
    }
    for other in [
        Destination::host_port("redirect.example", 443)
            .with_protocol(crate::core::ApplicationProtocol::Https),
        destination
            .clone()
            .with_protocol(crate::core::ApplicationProtocol::Http),
        Destination::host_port("origin.example", 8443)
            .with_protocol(crate::core::ApplicationProtocol::Https),
    ] {
        let other = search.for_refresh_destination(Some(&other));
        assert!(!other.provider_refresh_progress(exploration, 8).1);
    }
    let independent = ProxySearchContext::default().for_refresh_destination(Some(&destination));
    assert!(!independent.provider_refresh_progress(exploration, 8).1);
    let originless = search.for_refresh_destination(None);
    assert!(!originless.provider_refresh_progress(exploration, 8).1);
    originless.allow_provider_refresh();
    assert!(
        search
            .for_refresh_destination(None)
            .provider_refresh_progress(exploration, 8)
            .1
    );

    let timed = independent;
    let (timed_deadline, ready) = timed.provider_refresh_progress(Duration::from_millis(10), 8);
    assert!(!ready);
    tokio::time::sleep_until(timed_deadline).await;
    assert_eq!(
        timed
            .for_url("https://origin.example/another-path")
            .for_refresh_destination(Some(&destination))
            .provider_refresh_progress(Duration::from_millis(10), 8),
        (timed_deadline, true)
    );

    let provider = Arc::new(CountingProvider {
        records: Vec::new(),
        calls: AtomicUsize::new(0),
        delay: Duration::from_secs(60),
    });
    let probe = Arc::new(ProxyProbe::new(crate::core::ProbeConfig::default()));
    let inventory = ProxyInventory::new(
        InventoryConfig {
            exploration_before_refresh: exploration,
            ..InventoryConfig::default()
        },
        Some(provider.clone()),
        Some(probe.clone()),
    );
    let mut unknown = record(ProxyRuntimeStatus::Unknown);
    unknown.host = "127.0.0.1".into();
    unknown.port = 9;
    unknown.last_checked = None;
    unknown.http_forwarding = ProxyCapabilityStatus::Unknown;
    unknown.https_tunnel = ProxyCapabilityStatus::Unknown;
    probe.prepare_context(&mut unknown);
    inventory.add_or_update(vec![unknown]).await;
    let slots = inventory.tcp_slots.clone();
    let available = slots.available_permits();
    let _blocked = slots.acquire_many_owned(available as u32).await.unwrap();
    let countries = ["BE".into()];
    let selection =
        search.scope(inventory.select_any_for_destination(&countries, true, Some(&destination)));
    tokio::pin!(selection);
    tokio::time::timeout(Duration::from_secs(1), async {
        tokio::select! {
            _ = &mut selection => panic!("blocked discovery must remain pending"),
            _ = async {
                while provider.calls.load(Ordering::SeqCst) == 0 {
                    tokio::task::yield_now().await;
                }
            } => {}
        }
    })
    .await
    .expect("retained failures must start providers before a new exploration window");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn provider_refresh_resumes_after_cooldown() {
    let provider = Arc::new(CountingProvider {
        records: Vec::new(),
        calls: AtomicUsize::new(0),
        delay: Duration::ZERO,
    });
    let inventory = ProxyInventory::new(
        InventoryConfig {
            provider_refresh_cooldown: Duration::from_millis(20),
            provider_min_refresh_interval: Duration::from_millis(10),
            ..InventoryConfig::default()
        },
        Some(provider.clone()),
        None,
    );

    assert!(inventory.select("BE", true).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(inventory.select("BE", true).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
}
