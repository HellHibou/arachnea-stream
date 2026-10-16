#[cfg(feature = "persistence")]
struct RejectingPreferenceStore {
    writes: AtomicUsize,
}

#[cfg(feature = "persistence")]
#[derive(Clone)]
struct CapturedLogs(Arc<std::sync::Mutex<Vec<u8>>>);

#[cfg(feature = "persistence")]
impl std::io::Write for CapturedLogs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(feature = "persistence")]
#[async_trait]
impl arachnea_core::persistence::TypedEntityStore<crate::core::ProxyDestinationPreference>
    for RejectingPreferenceStore
{
    async fn get(&self, _: &String) -> AnyResult<Option<crate::core::ProxyDestinationPreference>> {
        Ok(None)
    }

    async fn put(&self, _: &crate::core::ProxyDestinationPreference) -> AnyResult<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!("injected preference write failure")
    }

    async fn put_all(&self, _: &[crate::core::ProxyDestinationPreference]) -> AnyResult<()> {
        anyhow::bail!("unexpected batch write")
    }

    async fn delete(&self, _: &String) -> AnyResult<()> {
        Ok(())
    }

    async fn find(
        &self,
        _: &arachnea_core::persistence::EntityQuery<crate::core::ProxyDestinationPreference>,
    ) -> AnyResult<Vec<crate::core::ProxyDestinationPreference>> {
        Ok(Vec::new())
    }
}

async fn local_http_endpoint(
    response: &'static [u8],
    barrier: Arc<tokio::sync::Barrier>,
    requests: usize,
) -> (ProxyRecord, tokio::task::JoinHandle<AnyResult<usize>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut proxy = record(ProxyRuntimeStatus::Ok);
    proxy.host = address.ip().to_string();
    proxy.port = address.port();
    proxy.protocol = Some(ProxyProtocol::Http);
    proxy.declarations.clear();
    let task = tokio::spawn(async move {
        for _ in 0..requests {
            let (mut stream, _) = listener.accept().await?;
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                let byte = stream.read_u8().await?;
                head.push(byte);
                anyhow::ensure!(head.len() <= 8192, "request headers exceed fixture limit");
            }
            barrier.wait().await;
            stream.write_all(response).await?;
            stream.shutdown().await?;
        }
        Ok(requests)
    });
    (proxy, task)
}

async fn pinned_http_exchange(
    selected: &crate::core::SelectedProxy,
    destination: &Destination,
) -> AnyResult<u16> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut outgoing = selected
        .core
        .connect_http_request(crate::core::ConnectRequest::new(destination.clone()))
        .await?;
    outgoing.stream.write_all(
        b"GET http://origin.example/ HTTP/1.1\r\nHost: origin.example\r\nConnection: close\r\n\r\n",
    ).await?;
    let mut bytes = Vec::new();
    outgoing.stream.read_to_end(&mut bytes).await?;
    let status = std::str::from_utf8(&bytes)?
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("missing response status"))?
        .parse()?;
    selected
        .selection_token
        .as_ref()
        .unwrap()
        .record_http_response(status)
        .await;
    Ok(status)
}

fn local_routing_core(inventory: ProxyInventory) -> crate::core::ArachneaProxyCore {
    crate::core::ArachneaProxyCore::new(crate::core::ProxyConfig::default())
        .unwrap()
        .with_proxy_inventory(inventory)
        .with_parameter_handler(crate::core::DynamicCountryRoutingProxyHandler::new(
            CoexistencePolicy::DynamicOnly,
        ))
}

async fn select_local_route(
    core: &crate::core::ArachneaProxyCore,
    destination: &Destination,
    country: &str,
) -> AnyResult<crate::core::SelectedProxy> {
    let mut context = crate::core::ClientContext::new();
    context.insert(
        crate::core::PROXY_PARAMETER_COUNTRIES,
        crate::core::ClientParameter::String(format!("[\"{country}\"]")),
    );
    Ok(core
        .select_for_request(
            crate::core::ConnectRequest::new(destination.clone()).with_client_context(context),
        )
        .await?)
}

#[tokio::test]
async fn concurrent_http_responses_keep_endpoint_tokens_and_pinned_reconnects() -> AnyResult<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let (accepted, accepted_task) = local_http_endpoint(
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            barrier.clone(),
            2,
        )
        .await;
        let (mut rejected, rejected_task) = local_http_endpoint(
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            barrier,
            2,
        )
        .await;
        rejected.country = Some("FR".into());
        let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
        inventory
            .add_or_update(vec![accepted.clone(), rejected.clone()])
            .await;
        let core = local_routing_core(inventory);
        let inventory = core.proxy_inventory().unwrap();
        let destination = Destination::host_port("origin.example", 80)
            .with_protocol(crate::core::ApplicationProtocol::Http);
        let initial_started = Instant::now();
        let first = select_local_route(&core, &destination, "BE").await?;
        let second = select_local_route(&core, &destination, "FR").await?;
        assert_ne!(first.selection_token, second.selection_token);
        for round in 0..2 {
            let (a, b) = tokio::join!(
                pinned_http_exchange(&first, &destination),
                pinned_http_exchange(&second, &destination),
            );
            assert_eq!(a?, 200);
            assert_eq!(b?, 403);
            if round == 0 {
                first
                    .selection_token
                    .as_ref()
                    .unwrap()
                    .record_accepted()
                    .await;
                let first_accept_us = initial_started.elapsed().as_micros();
                let warm_started = Instant::now();
                let warm = select_local_route(&core, &destination, "BE").await?;
                assert_eq!(
                    warm.selection_token.as_ref().unwrap().endpoint(),
                    first.selection_token.as_ref().unwrap().endpoint()
                );
                eprintln!(
                    "lot7 local HTTP: first_accept_us={first_accept_us}, preferred_select_us={}",
                    warm_started.elapsed().as_micros()
                );
            }
        }
        let ((), ()) = tokio::join!(
            first.selection_token.as_ref().unwrap().record_accepted(),
            second
                .selection_token
                .as_ref()
                .unwrap()
                .record_failure(crate::core::ProxyDestinationFailureReason::BlockedByOrigin,),
        );
        let records = inventory.all_records().await;
        let good = records
            .iter()
            .find(|r| r.authority() == accepted.authority())
            .unwrap();
        let bad = records
            .iter()
            .find(|r| r.authority() == rejected.authority())
            .unwrap();
        assert!(good.destination_failures.is_empty());
        assert_eq!(bad.destination_failures.len(), 1);
        assert_eq!(bad.status, ProxyRuntimeStatus::Ok);
        assert_eq!(
            inventory
                .select_any_for_destination(&["FR".into(), "BE".into()], false, Some(&destination),)
                .await?
                .authority(),
            accepted.authority()
        );
        assert_eq!(accepted_task.await??, 2);
        assert_eq!(rejected_task.await??, 2);
        Ok(())
    })
    .await?
}

#[cfg(feature = "persistence")]
#[tokio::test]
async fn preference_write_failure_preserves_http_success_and_does_not_replay() -> AnyResult<()> {
    use tracing::instrument::WithSubscriber;
    let captured = CapturedLogs(Arc::new(std::sync::Mutex::new(Vec::new())));
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::ERROR)
        .with_writer(move || writer.clone())
        .finish();
    tokio::time::timeout(Duration::from_secs(5), async {
        let (proxy, server) = local_http_endpoint(
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            Arc::new(tokio::sync::Barrier::new(1)),
            1,
        )
        .await;
        let store = Arc::new(RejectingPreferenceStore {
            writes: AtomicUsize::new(0),
        });
        let inventory = ProxyInventory::new(InventoryConfig::default(), None, None)
            .with_preference_repository(Arc::new(
                crate::core::TypedProxyPreferenceRepository::new(store.clone()),
            ));
        inventory.add_or_update(vec![proxy.clone()]).await;
        let core = local_routing_core(inventory);
        let destination = Destination::host_port("origin.example", 80)
            .with_protocol(crate::core::ApplicationProtocol::Http);
        let selected = select_local_route(&core, &destination, "BE").await?;
        let status = pinned_http_exchange(&selected, &destination).await?;
        selected
            .selection_token
            .as_ref()
            .unwrap()
            .record_accepted()
            .with_subscriber(subscriber)
            .await;
        assert_eq!(status, 200);
        assert_eq!(server.await??, 1);
        assert_eq!(store.writes.load(Ordering::SeqCst), 1);
        let inventory = core.proxy_inventory().unwrap();
        assert_eq!(inventory.preference_persistence_failures(), 1);
        let log = String::from_utf8(captured.0.lock().unwrap().clone())?;
        assert!(log.contains("failed to persist accepted proxy preference"));
        assert!(log.contains("injected preference write failure"));
        assert!(log.contains("origin.example"));
        assert_eq!(
            inventory
                .select_for_destination("BE", false, Some(&destination))
                .await?
                .authority(),
            proxy.authority()
        );
        Ok(())
    })
    .await?
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
async fn unavailable_http_probe_target_keeps_endpoint_positive_evidence() -> AnyResult<()> {
    use crate::core::http::response_classifier::{
        ProxyBodyRequirement, ProxyBodyState, ProxyResponseClassifier, ProxyResponseDecision,
        ProxyResponsePolicy, ProxyResponseView,
    };
    struct ContentPolicy(bool);
    impl ProxyResponseClassifier for ContentPolicy {
        fn body_requirement(&self) -> ProxyBodyRequirement {
            ProxyBodyRequirement::Prefix(3)
        }
        fn classify(&self, view: &ProxyResponseView<'_>) -> Result<ProxyResponseDecision> {
            assert_eq!(view.method, "GET");
            if self.0 {
                return Err(ProxyError::Protocol("injected classifier failure".into()));
            }
            if view.body == b"no!" {
                return Ok(ProxyResponseDecision::RejectedForDestination {
                    cooldown: None,
                    reason: "fixture block content".into(),
                });
            }
            Ok(
                if view.status == 200
                    && view.body == b"yes"
                    && view.body_state == ProxyBodyState::Limited
                {
                    ProxyResponseDecision::Accepted
                } else {
                    ProxyResponseDecision::Inconclusive
                },
            )
        }
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        let url = "http://unavailable.example/";
        let probe = ProxyProbe::new(crate::core::ProbeConfig {
            http_probe_url: Some(url.into()),
            https_probe_url: None,
            http_policy: crate::core::ProbeTargetPolicy {
                identity: "fixture-content-v1".into(),
                response_policy: Some(ProxyResponsePolicy(Arc::new(ContentPolicy(false)))),
                suspension: crate::core::ProbeTargetSuspension {
                    backoff: [Duration::from_millis(50), Duration::from_millis(100), Duration::from_millis(200)],
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        });
        for _ in 0..3 {
            let (mut proxy, server) = local_http_endpoint(
                b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                Arc::new(tokio::sync::Barrier::new(1)), 1,
            ).await;
            let _result = probe.probe(&mut proxy).await;
            assert_eq!(server.await??, 1);
            assert_ne!(proxy.status, ProxyRuntimeStatus::Ko);
            assert!(proxy.health_evidence.last_positive_at.is_some());
            assert!(proxy.observations.iter().any(|item| {
                item.context.phase == crate::core::ProxyObservationPhase::DestinationHttp
                    && item.outcome == crate::core::ProxyObservationOutcome::Succeeded
            }));
            assert!(proxy.destination_failures.is_empty());
        }
        assert!(probe.target_diagnostics(url).suspected);
        assert!(!probe.target_diagnostics(url).suspended_for.is_zero());
        let (mut proxy, server) = local_http_endpoint(
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n",
            Arc::new(tokio::sync::Barrier::new(1)), 1,
        ).await;
        let _result = probe.probe(&mut proxy).await;
        assert!(proxy.health_evidence.last_positive_at.is_some());
        assert!(!proxy.observations.iter().any(|item| {
            item.context.phase == crate::core::ProxyObservationPhase::DestinationHttp
        }));
        assert!(server.await?.is_err());
        tokio::time::sleep(Duration::from_millis(60)).await;
        let (mut proxy, server) = local_http_endpoint(
            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n",
            Arc::new(tokio::sync::Barrier::new(1)), 1,
        ).await;
        let _result = probe.probe(&mut proxy).await;
        assert_eq!(server.await??, 1);
        assert!(probe.target_diagnostics(url).suspended_for > Duration::from_millis(50));
        tokio::time::sleep(Duration::from_millis(110)).await;
        let (mut proxy, server) = local_http_endpoint(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nyes\r\n0\r\n\r\n",
            Arc::new(tokio::sync::Barrier::new(1)), 1,
        ).await;
        probe.probe(&mut proxy).await?;
        assert_eq!(server.await??, 1);
        assert!(!probe.target_diagnostics(url).suspected);
        assert!(proxy.observations.iter().any(|item| {
            item.context.phase == crate::core::ProxyObservationPhase::ProbeTargetAccepted
                && item.outcome == crate::core::ProxyObservationOutcome::Succeeded
        }));
        use crate::core::http::response_classifier::inspect_proxy_response;
        use futures::StreamExt;
        let headers = http::HeaderMap::new();
        for (payload, fail) in [(b"no!media".as_slice(), false), (b"y".as_slice(), false), (b"yesmedia".as_slice(), false), (b"yesmedia".as_slice(), true)] {
            let policy = ProxyResponsePolicy(Arc::new(ContentPolicy(fail)));
            let stream = Box::pin(futures::stream::iter(vec![Ok(bytes::Bytes::copy_from_slice(payload))]));
            let view = ProxyResponseView { method: "GET", requested_url: url, effective_url: url,
                redirect_urls: &[], status: 200, headers: &headers, selection: None, body: &[], body_state: ProxyBodyState::Unavailable };
            let (decision, mut restored) = inspect_proxy_response(Some(&policy), view, stream, Duration::from_millis(20)).await;
            if fail || payload.len() < 3 { assert_eq!(decision, ProxyResponseDecision::Inconclusive); }
            else if payload.starts_with(b"no!") { assert!(matches!(decision, ProxyResponseDecision::RejectedForDestination { .. })); }
            else { assert_eq!(decision, ProxyResponseDecision::Accepted); }
            let mut received = Vec::new();
            while let Some(bytes) = restored.next().await { received.extend_from_slice(&bytes?); }
            assert_eq!(received, payload, "classification must preserve every media byte");
        }
        let policy = ProxyResponsePolicy(Arc::new(ContentPolicy(false)));
        let pending = futures::stream::iter(vec![Ok(bytes::Bytes::from_static(b"y"))])
            .chain(futures::stream::pending());
        let view = ProxyResponseView { method: "GET", requested_url: url, effective_url: url,
            redirect_urls: &[], status: 200, headers: &headers, selection: None, body: &[], body_state: ProxyBodyState::Unavailable };
        let (decision, mut restored) = inspect_proxy_response(Some(&policy), view, Box::pin(pending), Duration::from_millis(5)).await;
        assert_eq!(decision, ProxyResponseDecision::Inconclusive);
        assert_eq!(restored.next().await.unwrap()?, bytes::Bytes::from_static(b"y"));
        drop(restored);
        Ok(())
    })
    .await?
}

#[tokio::test]
async fn probe_target_change_invalidates_aggregates_without_erasing_positive_facts() {
    let first = ProxyProbe::new(crate::core::ProbeConfig {
        http_probe_url: Some("http://first.example/".into()),
        ..Default::default()
    });
    let second = ProxyProbe::new(crate::core::ProbeConfig {
        http_probe_url: Some("http://second.example/".into()),
        ..Default::default()
    });
    let mut proxy = record(ProxyRuntimeStatus::Ok);
    first.prepare_context(&mut proxy);
    proxy.http_forwarding = ProxyCapabilityStatus::Available;
    proxy.probe_attempts.insert("http".into(), epoch_millis());
    proxy.health_evidence.last_positive_at = Some(SystemTime::now());
    let positive = proxy.health_evidence.last_positive_at;
    second.prepare_context(&mut proxy);
    assert_ne!(first.configuration_id(), second.configuration_id());
    assert_eq!(proxy.http_forwarding, ProxyCapabilityStatus::Unknown);
    assert!(!proxy.probe_attempts.contains_key("http"));
    assert_eq!(proxy.health_evidence.last_positive_at, positive);
}

#[tokio::test]
async fn redirect_response_token_belongs_to_final_origin_only() -> AnyResult<()> {
    use crate::core::http::{ProxiedHttpRequest, ProxiedResponseBody};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tokio::time::timeout(Duration::from_secs(5), async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let mut targets = Vec::new();
            for response in [
                &b"HTTP/1.1 302 Found\r\nLocation: http://final.example/content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"[..],
                &b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"[..],
            ] {
                let (mut stream, _) = listener.accept().await?;
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    head.push(stream.read_u8().await?);
                    anyhow::ensure!(head.len() <= 8192, "fixture header limit");
                }
                targets.push(String::from_utf8(head)?);
                stream.write_all(response).await?;
                stream.shutdown().await?;
            }
            Ok::<_, anyhow::Error>(targets)
        });
        let mut proxy = record(ProxyRuntimeStatus::Ok);
        proxy.host = address.ip().to_string();
        proxy.port = address.port();
        proxy.protocol = Some(ProxyProtocol::Http);
        proxy.declarations.clear();
        let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
        inventory.add_or_update(vec![proxy]).await;
        let core = local_routing_core(inventory);
        let mut context = crate::core::ClientContext::new();
        context.insert(crate::core::PROXY_PARAMETER_COUNTRIES,
            crate::core::ClientParameter::String("[\"BE\"]".into()));
        let response = core.http_client().request_proxied(ProxiedHttpRequest {
            response_classifier: None,
            url: "http://initial.example/start".into(), method: "GET".into(),
            headers: Default::default(), cookies: Default::default(), body: Vec::new(),
            client_context: context, post_actions: Vec::new(), buffer_response_body: true,
            headers_only: false, context: Default::default(), verify_tls: true,
            follow_redirects: Some(1),
        }).await?;
        assert_eq!(response.status, 200);
        let token = response.selection_token.unwrap();
        assert_eq!(token.destination().host_for_protocol(), "final.example");
        use crate::core::http::response_classifier::ProxyResponseDecision;
        assert_eq!(response.response_decision, ProxyResponseDecision::Accepted);
        token.record_response_decision(&response.response_decision).await;
        let inner = core.proxy_inventory().unwrap().inner.read().await;
        assert_eq!(inner.preferred_by_destination.len(), 1);
        assert!(inner.preferred_by_destination.keys().all(|key| key.contains("final.example")));
        drop(inner);
        token.record_response_decision(&ProxyResponseDecision::RejectedForDestination {
            reason: "fixture rejection".into(), cooldown: Some(Duration::from_secs(120)),
        }).await;
        token.record_response_decision(&ProxyResponseDecision::Inconclusive).await;
        let inner = core.proxy_inventory().unwrap().inner.read().await;
        assert_eq!(inner.preferred_by_destination.len(), 1);
        let record = inner.records.get(token.endpoint()).unwrap();
        assert_eq!(record.destination_failures.len(), 1);
        let remaining = record.destination_failures[0].cooldown_until.unwrap().duration_since(SystemTime::now())?;
        assert!(remaining <= Duration::from_secs(120) && remaining > Duration::from_secs(100));
        assert!(matches!(response.body, ProxiedResponseBody::Buffered(ref bytes) if bytes == b"ok"));
        let requests = server.await??;
        assert!(requests[0].starts_with("GET http://initial.example/start "));
        assert!(requests[1].starts_with("GET http://final.example/content "));
        Ok(())
    }).await?
}

#[tokio::test]
async fn late_concurrent_token_results_do_not_resurrect_evicted_endpoint() -> AnyResult<()> {
    let inventory = Arc::new(ProxyInventory::new(InventoryConfig::default(), None, None));
    let mut proxy = record(ProxyRuntimeStatus::Ok);
    proxy.health_evidence.last_positive_at =
        Some(SystemTime::now() - Duration::from_secs(25 * 3600));
    inventory.add_or_update(vec![proxy.clone()]).await;
    let destination = Destination::host_port("origin.example", 80)
        .with_protocol(crate::core::ApplicationProtocol::Http);
    let token =
        crate::core::ProxySelectionToken::new(inventory.clone(), proxy.authority(), destination);
    inventory.record_global_failure(&proxy.authority()).await;
    let (_, _) = tokio::join!(token.record_accepted(), token.record_http_response(200));
    assert!(inventory.is_empty().await);
    assert!(inventory
        .inner
        .read()
        .await
        .preferred_by_destination
        .is_empty());
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
