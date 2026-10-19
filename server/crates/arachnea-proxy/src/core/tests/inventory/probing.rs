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
