use super::*;

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
        let initial = Destination::host_port("initial.example", 80).with_protocol(crate::core::ApplicationProtocol::Http);
        assert!(inner.records.get(token.endpoint()).unwrap().observations.iter().any(|observation|
            observation.context.configuration == "business-redirect-v1"
                && observation.context.target.as_deref() == Some(initial.to_string().as_str())
                && observation.http_status == Some(302)));
        drop(inner);
        let search = ProxySearchContext::default();
        let inventory = core.proxy_inventory().unwrap();
        let selected = search.scope(inventory.select_any_for_destination(&["BE".into()], false, Some(&initial))).await?;
        assert_eq!(selected.authority(), token.endpoint());
        assert_eq!(search.selection_provenance(token.endpoint(), &initial), crate::core::ProxySelectionProvenance::RedirectDestination);
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
