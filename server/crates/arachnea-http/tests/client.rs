use super::test_support::StaticEngine;
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Verifies that a directly injected engine handles application requests.
#[tokio::test]
async fn injected_engine_executes_requests() {
    let calls = Arc::new(AtomicUsize::new(0));
    let engine = StaticEngine {
        calls: calls.clone(),
    };
    let config = ArachneaHttpConfig::builder()
        .engine_instance(engine)
        .build()
        .expect("valid config");
    let client = ArachneaHttpClient::new(config).await.expect("client");

    let response = client
        .get("https://example.com/custom")
        .send()
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.bytes().await.expect("body"),
        Bytes::from_static(b"injected")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    #[cfg(feature = "arachnea-proxy")]
    {
        use arachnea_proxy::core::*;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..37 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    head.push(stream.read_u8().await.unwrap());
                }
                assert!(head.starts_with(b"GET http://pool.example/content "));
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    )
                    .await
                    .unwrap();
                stream.shutdown().await.unwrap();
            }
        });
        let record: ProxyRecord = serde_json::from_value(serde_json::json!({
            "host": "127.0.0.1", "port": address.port(), "country": "BE",
            "protocol": "Http", "status": "ok", "http_forwarding": "available"
        }))
        .unwrap();
        let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
        inventory.add_or_update(vec![record]).await;
        let core = ArachneaProxyCore::new(ProxyConfig::default())
            .unwrap()
            .with_proxy_inventory(inventory)
            .with_parameter_handler(DynamicCountryRoutingProxyHandler::new(
                CoexistencePolicy::DynamicOnly,
            ));
        let runtime = Arc::new(arachnea_proxy::server::LoopbackProxyRuntime::new(0));
        let pool = Arc::new(ProxyTransportPool::default());
        let config = ArachneaHttpConfig::builder()
            .proxy_core(core)
            .proxy_runtime(runtime)
            .proxy_transport_pool(pool.clone())
            .proxy_parameter("countries", "[\"BE\"]")
            .build()
            .unwrap();
        let mut previous = None;
        let mut preferred_lease = None;
        for attempt in 0..3 {
            let client = ArachneaHttpClient::new(config.clone()).await.unwrap();
            let search = ProxySearchContext::new(ProxySearchLimits {
                target: std::time::Duration::from_secs(1),
                extension: std::time::Duration::ZERO,
            });
            let response = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                search.scope(client.get("http://pool.example/content").send()),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(response.text().await.unwrap(), "ok");
            let token = response.proxy_selection().unwrap();
            assert_ne!(
                previous.as_ref(),
                Some(token),
                "each call has a new selection token"
            );
            assert_eq!(token.reuses_destination(), attempt > 0);
            previous = Some(token.clone());
            if attempt > 0 {
                let idle = pool.idle.lock().await;
                let transport = idle
                    .values()
                    .flatten()
                    .max_by_key(|entry| entry.idle_since)
                    .unwrap();
                if let Some(lease) = &preferred_lease {
                    assert_eq!(
                        transport.loopback.proxy_url(),
                        lease,
                        "compatible preferred calls must reuse the idle transport across clients"
                    );
                }
                preferred_lease = Some(transport.loopback.proxy_url().to_string());
            }
            if attempt == 1 {
                tokio::time::sleep(std::time::Duration::from_millis(1010)).await;
                assert!(
                    search.origin_remaining().is_zero(),
                    "reconnect must not inherit this expired search"
                );
            }
        }
        let mut transports = Vec::new();
        for _ in 0..34 {
            let client = ArachneaHttpClient::new(config.clone()).await.unwrap();
            let response = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                client.get("http://pool.example/content").send(),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(response.text().await.unwrap(), "ok");
            let key = pool
                .idle
                .lock()
                .await
                .keys()
                .find(|key| key.starts_with("http://pool.example|") && key.contains("Some(60s)"))
                .unwrap()
                .clone();
            transports.push(pool.checkout(&key).await.0.unwrap());
        }
        server.await.unwrap();
        pool.idle.lock().await.clear();
        let mut first = transports.pop().unwrap();
        first.idle_since = std::time::Instant::now() - std::time::Duration::from_secs(10);
        let first_lease = first.loopback.proxy_url().to_string();
        let mut second = transports.pop().unwrap();
        second.idle_since = first.idle_since + std::time::Duration::from_millis(1);
        let second_lease = second.loopback.proxy_url().to_string();
        assert_ne!(first_lease, second_lease);
        let mut third = transports.pop().unwrap();
        third.idle_since = second.idle_since + std::time::Duration::from_millis(1);
        pool.retain("shared".into(), first).await;
        pool.retain("shared".into(), second).await;
        let (a, b) = tokio::join!(pool.checkout("shared"), pool.checkout("shared"));
        let a = a.0.unwrap();
        let b = b.0.unwrap();
        assert_ne!(a.loopback.proxy_url(), b.loopback.proxy_url());
        assert!(
            pool.checkout("shared").await.0.is_none(),
            "active borrowers never block checkout"
        );
        pool.retain("shared".into(), a).await;
        pool.retain("shared".into(), b).await;
        pool.retain("shared".into(), third).await;
        assert_eq!(pool.idle.lock().await["shared"].len(), 2);
        assert!(
            !pool.idle.lock().await["shared"]
                .iter()
                .any(|entry| entry.loopback.proxy_url() == first_lease),
            "oldest route entry is evicted"
        );
        assert!(pool.checkout("other-route").await.0.is_none());
        for (index, transport) in transports.into_iter().enumerate() {
            pool.retain(format!("route-{index}"), transport).await;
        }
        {
            let mut idle = pool.idle.lock().await;
            assert_eq!(idle.values().map(Vec::len).sum::<usize>(), 32);
            assert_eq!(
                idle.len(),
                32,
                "global eviction removes the oldest transport, not its entire route"
            );
            let newest = idle
                .values_mut()
                .flat_map(|entries| entries.iter_mut())
                .max_by_key(|entry| entry.idle_since)
                .unwrap();
            newest.idle_since = std::time::Instant::now() - std::time::Duration::from_secs(61);
        }
        let (_, expired, remaining) = pool.checkout("missing-route").await;
        assert_eq!(expired, 1);
        assert_eq!(remaining, 31);
        assert!(pool
            .idle
            .lock()
            .await
            .values()
            .all(|entries| !entries.is_empty()));

        use arachnea_proxy::core::http::response_classifier::*;
        struct ContentGate(bool);
        impl ProxyResponseClassifier for ContentGate {
            fn body_requirement(&self) -> ProxyBodyRequirement {
                ProxyBodyRequirement::Prefix(64)
            }
            fn classify(
                &self,
                view: &ProxyResponseView<'_>,
            ) -> arachnea_proxy::core::Result<ProxyResponseDecision> {
                if view.body == b"blocked" {
                    return Ok(ProxyResponseDecision::RejectedForDestination {
                        cooldown: None,
                        reason: "fixture content rejection".into(),
                    });
                }
                Ok(if view.status == 200 && !self.0 {
                    ProxyResponseDecision::Accepted
                } else {
                    ProxyResponseDecision::Inconclusive
                })
            }
        }
        for (case, location, redirects, callback, truncated, expected) in [
            (
                "followed",
                Some("http://final.example/end"),
                2,
                false,
                false,
                true,
            ),
            ("missing", None, 2, false, false, false),
            ("invalid", Some("http://["), 2, false, false, false),
            (
                "disabled",
                Some("http://final.example/end"),
                0,
                false,
                false,
                false,
            ),
            (
                "custom",
                Some("http://final.example/end"),
                2,
                true,
                false,
                false,
            ),
            (
                "truncated",
                Some("http://final.example/end"),
                2,
                false,
                true,
                false,
            ),
            ("content_rejected", None, 2, true, false, false),
            ("content_inconclusive", None, 2, true, false, false),
            (
                "status_rejected",
                Some("http://final.example/end"),
                2,
                false,
                false,
                false,
            ),
            (
                "quota_exceeded",
                Some("http://redirect.example/next"),
                1,
                false,
                false,
                true,
            ),
            (
                "cancelled",
                Some("http://final.example/end"),
                2,
                false,
                false,
                false,
            ),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let mut servers = tokio::task::JoinSet::new();
            let (cancel_started, cancel_received) = tokio::sync::oneshot::channel();
            let mut cancel_received = Some(cancel_received);
            servers.spawn(async move {
                let mut cancel_started = Some(cancel_started);
                loop {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let mut head = Vec::new();
                    while !head.ends_with(b"\r\n\r\n") {
                        head.push(stream.read_u8().await.unwrap());
                    }
                    let final_response = String::from_utf8(head).unwrap().lines().next().unwrap().contains("/end ");
                    let body = if case == "content_rejected" { "blocked" } else { "ok" };
                    let status = if final_response || matches!(case, "content_rejected" | "content_inconclusive") { "200 OK" } else { "302 Found" };
                    let location = if final_response { None } else { location };
                    let location = location.map(|value| format!("Location: {value}\r\n")).unwrap_or_default();
                    let response = format!("HTTP/1.1 {status}\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len() + usize::from(truncated));
                    if case == "cancelled" {
                        let headers = response.split("\r\n\r\n").next().unwrap().to_string() + "\r\n\r\n";
                        stream.write_all(headers.as_bytes()).await.unwrap();
                        let _ = cancel_started.take().unwrap().send(());
                        futures::future::pending::<()>().await;
                    }
                    stream.write_all(response.as_bytes()).await.unwrap();
                    stream.shutdown().await.unwrap();
                }
            });
            let record: ProxyRecord = serde_json::from_value(serde_json::json!({
                "host": "127.0.0.1", "port": address.port(), "country": "BE",
                "protocol": "Http", "status": "ok", "http_forwarding": "available"
            }))
            .unwrap();
            let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
            inventory.add_or_update(vec![record]).await;
            let core = ArachneaProxyCore::new(ProxyConfig::default())
                .unwrap()
                .with_proxy_inventory(inventory)
                .with_parameter_handler(DynamicCountryRoutingProxyHandler::new(
                    CoexistencePolicy::DynamicOnly,
                ));
            let pool = Arc::new(ProxyTransportPool::default());
            let mut config = ArachneaHttpConfig::builder()
                .proxy_core(core)
                .proxy_runtime(Arc::new(arachnea_proxy::server::LoopbackProxyRuntime::new(
                    0,
                )))
                .proxy_transport_pool(pool.clone())
                .proxy_parameter("countries", "[\"BE\"]")
                .max_redirects(Some(redirects))
                .build()
                .unwrap();
            if callback {
                config.proxy_response_classifier = Some(ProxyResponsePolicy(Arc::new(
                    ContentGate(case == "content_inconclusive"),
                )));
            }
            if case == "status_rejected" {
                config.proxy_response_classifier = Some(
                    ProxyResponseClassifierConfig::Statuses {
                        rejected_statuses: vec![302],
                        accepted_statuses: vec![200],
                        cooldown_seconds: None,
                    }
                    .build()
                    .unwrap(),
                );
            }
            let mut previous_lease = None;
            for attempt in 0..if expected && case != "quota_exceeded" {
                3
            } else {
                1
            } {
                let client = ArachneaHttpClient::new(config.clone()).await.unwrap();
                let search = ProxySearchContext::new(ProxySearchLimits {
                    target: std::time::Duration::from_millis(300),
                    extension: std::time::Duration::ZERO,
                });
                {
                    let request = search.scope(client.get("http://redirect.example/start").send());
                    tokio::pin!(request);
                    if case == "cancelled" {
                        tokio::time::timeout(std::time::Duration::from_secs(2), async {
                            tokio::select! {
                                _ = &mut request => panic!("body must remain pending before cancellation"),
                                _ = cancel_received.take().unwrap() => {}
                            }
                        }).await.unwrap();
                    } else {
                        let result =
                            tokio::time::timeout(std::time::Duration::from_secs(2), request)
                                .await
                                .unwrap();
                        if case == "quota_exceeded" {
                            assert!(matches!(
                                result,
                                Err(ArachneaHttpError::RedirectLimitExceeded { .. })
                            ));
                        } else if expected {
                            assert_eq!(result.unwrap().status(), StatusCode::OK);
                        }
                    }
                }
                let idle = pool.idle.lock().await;
                let retained = idle
                    .iter()
                    .filter(|(key, _)| key.starts_with("http://redirect.example|"))
                    .flat_map(|(_, entries)| entries.iter())
                    .max_by_key(|transport| transport.idle_since);
                assert_eq!(
                    retained.is_some(),
                    expected,
                    "redirect retention case {case}"
                );
                if case == "quota_exceeded" {
                    assert_eq!(
                        idle.len(),
                        1,
                        "only the followed hop can retain a transport"
                    );
                }
                if attempt > 0 {
                    let lease = retained.unwrap().loopback.proxy_url().to_string();
                    if let Some(previous) = previous_lease {
                        assert_eq!(previous, lease);
                    }
                    previous_lease = Some(lease);
                }
            }
        }
        for provenance in [
            ProxySelectionProvenance::Discovery,
            ProxySelectionProvenance::AcceptedDestination,
            ProxySelectionProvenance::RedirectDestination,
        ] {
            for method in [::http::Method::GET, ::http::Method::POST] {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let (release, released) = tokio::sync::oneshot::channel();
                let server = tokio::spawn(async move {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let mut head = Vec::new();
                    while !head.ends_with(b"\r\n\r\n") {
                        head.push(stream.read_u8().await.unwrap());
                    }
                    let _ = released.await;
                    drop(stream);
                });
                let proxy: ProxyRecord = serde_json::from_value(serde_json::json!({
                    "host": "127.0.0.1", "port": address.port(), "country": "BE",
                    "protocol": "Http", "status": "ok", "http_forwarding": "available"
                }))
                .unwrap();
                let inventory = ProxyInventory::new(
                    InventoryConfig {
                        reserve_size: 0,
                        ..Default::default()
                    },
                    None,
                    None,
                );
                inventory.add_or_update(vec![proxy]).await;
                let core = ArachneaProxyCore::new(ProxyConfig::default())
                    .unwrap()
                    .with_proxy_inventory(inventory)
                    .with_parameter_handler(DynamicCountryRoutingProxyHandler::new(
                        CoexistencePolicy::DynamicOnly,
                    ));
                let route = || {
                    let mut context = ClientContext::new();
                    context.insert(
                        PROXY_PARAMETER_COUNTRIES,
                        ClientParameter::String("[\"BE\"]".into()),
                    );
                    ConnectRequest::new(Destination::host_port("headers.example", 80))
                        .with_client_context(context)
                };
                let seed = ProxySearchContext::default();
                let selected = seed.scope(core.select_for_request(route())).await.unwrap();
                let token = selected.selection_token.unwrap();
                match provenance {
                    ProxySelectionProvenance::AcceptedDestination => token.record_accepted().await,
                    ProxySelectionProvenance::RedirectDestination => {
                        token.record_redirect(302).await
                    }
                    ProxySelectionProvenance::Discovery => token.finish_discovery(),
                }
                let search = ProxySearchContext::default();
                let selected = search
                    .scope(core.select_for_request(route()))
                    .await
                    .unwrap();
                let token = selected.selection_token.unwrap();
                assert_eq!(token.provenance(), provenance);
                let proxy_url = format!("http://{address}");
                let native = rquest::Client::builder()
                    .proxy(rquest::Proxy::all(&proxy_url).unwrap())
                    .build()
                    .unwrap();
                let timeout = std::time::Duration::from_millis(100);
                let engine = crate::engine::rquest::RquestEngine::new(
                    &ArachneaHttpConfig::builder()
                        .request_timeout(timeout)
                        .build()
                        .unwrap(),
                    Some(&proxy_url),
                    Some(native),
                )
                .unwrap();
                let started = tokio::time::Instant::now();
                let error = tokio::time::timeout(
                    std::time::Duration::from_secs(1),
                    search.scope(engine.send_for_proxy(
                        crate::engine::EngineRequest {
                            method: method.clone(),
                            url: "http://headers.example/content".into(),
                            headers: Default::default(),
                            body: None,
                        },
                        Some(token),
                        None,
                        "http://headers.example/content",
                        &[],
                    )),
                )
                .await
                .expect("the configured header timeout must apply to every provenance")
                .unwrap_err();
                assert!(started.elapsed() >= timeout);
                let ArachneaHttpError::ProxyAttempt {
                    delivery,
                    source,
                    selection,
                } = error
                else {
                    panic!("expected an attributed header timeout");
                };
                assert_eq!(delivery, ProxyRequestDelivery::PossiblySent);
                assert!(source
                    .to_string()
                    .contains("origin response headers timed out"));
                assert_eq!(selection.unwrap().provenance(), provenance);
                assert!(
                    search.can_start(),
                    "one attempt must not exhaust the global search budget"
                );
                assert_eq!(
                    proxy_retry_allowed(method.as_str(), false, delivery, false),
                    method == ::http::Method::GET
                );
                assert!(proxy_retry_allowed(method.as_str(), true, delivery, false));
                let _ = release.send(());
                server.await.unwrap();
            }
        }
    }
}
