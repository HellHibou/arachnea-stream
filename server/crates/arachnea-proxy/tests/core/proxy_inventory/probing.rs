use super::*;

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
        use crate::core::http::response_classifier::{
            inspect_proxy_response_with_transport_policy, ProxyResponseClassifierConfig,
        };
        struct OptedInContent(bool);
        impl ProxyResponseClassifier for OptedInContent {
            fn allows_redirect_transport_reuse(&self) -> bool { true }
            fn body_requirement(&self) -> ProxyBodyRequirement { ProxyBodyRequirement::Prefix(3) }
            fn classify(&self, view: &ProxyResponseView<'_>) -> Result<ProxyResponseDecision> {
                ContentPolicy(self.0).classify(view)
            }
        }
        struct ScopedContent;
        impl ProxyResponseClassifier for ScopedContent {
            fn body_requirement(&self) -> ProxyBodyRequirement { ProxyBodyRequirement::Prefix(3) }
            fn body_requirement_for_response(&self, view: &ProxyResponseView<'_>) -> ProxyBodyRequirement {
                if view.requested_url.contains("metadata.example") || view.effective_url.contains("metadata.example") {
                    self.body_requirement()
                } else { ProxyBodyRequirement::None }
            }
            fn classify(&self, view: &ProxyResponseView<'_>) -> Result<ProxyResponseDecision> {
                if self.body_requirement_for_response(view) == ProxyBodyRequirement::None {
                    assert!(view.body.is_empty());
                    assert_eq!(view.body_state, ProxyBodyState::Unavailable);
                    return Ok(ProxyResponseDecision::Accepted);
                }
                ContentPolicy(false).classify(view)
            }
        }
        for (requested, effective, inspect) in [
            ("https://metadata.example/a", "https://cdn.example/b", true),
            ("https://cdn.example/a", "https://metadata.example/b", true),
            ("https://cdn.example/a", "https://cdn.example/b", false),
        ] {
            let view = ProxyResponseView { method: "GET", requested_url: requested, effective_url: effective,
                redirect_urls: &[], status: 200, headers: &headers, selection: None, body: &[], body_state: ProxyBodyState::Unavailable };
            assert_eq!(ContentPolicy(false).body_requirement_for_response(&view), ProxyBodyRequirement::Prefix(3));
            let policy = ProxyResponsePolicy(Arc::new(ScopedContent));
            let payload = bytes::Bytes::from_static(b"no!media");
            let stream = Box::pin(futures::stream::iter(vec![Ok(payload.clone())]));
            let (decision, mut replay) = inspect_proxy_response(Some(&policy), view, stream, Duration::from_millis(10)).await;
            assert_eq!(matches!(decision, ProxyResponseDecision::RejectedForDestination { .. }), inspect);
            let mut body = Vec::new();
            while let Some(chunk) = replay.next().await { body.extend_from_slice(&chunk?); }
            assert_eq!(body, payload);
        }
        let policy = ProxyResponsePolicy(Arc::new(ScopedContent));
        let view = ProxyResponseView { method: "GET", requested_url: "https://cdn.example/a", effective_url: "https://cdn.example/a",
            redirect_urls: &[], status: 200, headers: &headers, selection: None, body: &[], body_state: ProxyBodyState::Unavailable };
        let (decision, _) = tokio::time::timeout(Duration::from_millis(10),
            inspect_proxy_response(Some(&policy), view, Box::pin(futures::stream::pending()), Duration::from_secs(1))).await?;
        assert_eq!(decision, ProxyResponseDecision::Accepted, "metadata classification must not wait for CDN bytes");
        assert!(!ContentPolicy(false).allows_redirect_transport_reuse());
        for (status, policy, expected) in [
            (302, None, true), (200, None, false), (304, None, false),
            (302, Some(ProxyResponsePolicy(Arc::new(ContentPolicy(false)))), false),
            (302, Some(ProxyResponsePolicy(Arc::new(OptedInContent(true)))), false),
            (302, Some(ProxyResponseClassifierConfig::Statuses {
                rejected_statuses: vec![302], accepted_statuses: vec![200], cooldown_seconds: None
            }.build()?), false),
        ] {
            let view = ProxyResponseView { method: "GET", requested_url: url, effective_url: url,
                redirect_urls: &[], status, headers: &headers, selection: None, body: &[], body_state: ProxyBodyState::Unavailable };
            let (_, _, allowed) = inspect_proxy_response_with_transport_policy(
                policy.as_ref(), view, Box::pin(futures::stream::empty()), Duration::from_millis(10)
            ).await;
            assert_eq!(allowed, expected);
        }
        for stream in [
            Box::pin(futures::stream::pending()) as crate::core::http::response_classifier::ProxyResponseStream,
            Box::pin(futures::stream::iter(vec![Err(std::io::Error::other("fixture inspection failure"))])),
        ] {
            let policy = ProxyResponsePolicy(Arc::new(OptedInContent(false)));
            let view = ProxyResponseView { method: "GET", requested_url: url, effective_url: url,
                redirect_urls: &[], status: 302, headers: &headers, selection: None, body: &[], body_state: ProxyBodyState::Unavailable };
            let (_, _, allowed) = inspect_proxy_response_with_transport_policy(
                Some(&policy), view, stream, Duration::from_millis(5)
            ).await;
            assert!(!allowed, "failed inspection must veto even an opted-in policy");
        }
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
    let mut previous = proxy.clone();
    first.prepare_context(&mut previous);
    previous.http_forwarding = ProxyCapabilityStatus::Available;
    previous.https_tunnel = ProxyCapabilityStatus::Available;
    previous.destination_tls = ProxyCapabilityStatus::Available;
    previous.last_checked = Some(SystemTime::now());
    previous.probe_attempts.insert("tcp".into(), 17);
    previous.probe_attempts.insert("http".into(), 19);
    let mut current = previous.clone();
    second.prepare_context(&mut current);
    current.http_forwarding = ProxyCapabilityStatus::Available;
    current.last_checked = Some(SystemTime::now());
    let mut records = [
        previous,
        current.clone(),
        record(ProxyRuntimeStatus::Unknown),
    ];
    let mut expected = records.clone();
    for candidate in &mut expected {
        second.prepare_context(candidate);
    }
    second.prepare_contexts(records.iter_mut());
    assert_eq!(records, expected);
    assert_eq!(records[0].http_forwarding, ProxyCapabilityStatus::Unknown);
    assert_eq!(records[0].https_tunnel, ProxyCapabilityStatus::Unknown);
    assert_eq!(records[0].destination_tls, ProxyCapabilityStatus::Unknown);
    assert!(records[0].last_checked.is_none());
    assert_eq!(records[0].status, ProxyRuntimeStatus::Ok);
    assert_eq!(records[0].probe_attempts.get("tcp"), Some(&17));
    assert!(!records[0].probe_attempts.contains_key("http"));
    assert_eq!(records[0].health_evidence.last_positive_at, positive);
    assert_eq!(records[1], current);
    let unchanged = records.clone();
    second.prepare_contexts(records.iter_mut());
    assert_eq!(records, unchanged);
    for candidate in &mut expected {
        first.prepare_context(candidate);
    }
    first.prepare_contexts(records.iter_mut());
    assert_eq!(records, expected);
    assert_eq!(records[0].health_evidence.last_positive_at, positive);
    second.prepare_contexts(std::iter::empty::<&mut ProxyRecord>());
}
