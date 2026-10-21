use super::*;

#[tokio::test]
async fn multi_country_selection_checks_all_cached_countries_before_loading() {
    for require_https in [false, true] {
        let provider = Arc::new(CountingProvider {
            records: Vec::new(),
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
        });
        let probe = Arc::new(ProxyProbe::new(crate::core::ProbeConfig::default()));
        let inventory = ProxyInventory::new(
            InventoryConfig::default(),
            Some(provider.clone()),
            Some(probe.clone()),
        );
        let mut cached = record(ProxyRuntimeStatus::Ok);
        cached.host = "fr-proxy.example".to_string();
        cached.country = Some("FR".to_string());
        probe.prepare_context(&mut cached);
        cached.http_forwarding = ProxyCapabilityStatus::Available;
        cached.https_tunnel = ProxyCapabilityStatus::Available;
        cached.destination_tls = ProxyCapabilityStatus::Available;
        cached.last_checked = Some(SystemTime::now());
        cached.latency_ms = Some(100);
        let mut unknown = record(ProxyRuntimeStatus::Unknown);
        unknown.host = "127.0.0.1".into();
        unknown.port = 9;
        unknown.country = Some("AD".into());
        unknown.latency_ms = Some(1);
        inventory.add_or_update(vec![unknown.clone()]).await;
        #[cfg(feature = "persistence")]
        let inventory = {
            let repository = Arc::new(TypedProxyRepository::new(memory_proxy_store().unwrap()));
            repository.save_many(&[cached.clone()]).await.unwrap();
            inventory.with_proxy_repository(repository)
        };
        #[cfg(not(feature = "persistence"))]
        inventory.add_or_update(vec![cached.clone()]).await;

        for countries in [
            vec!["ad".into(), " FR ".into(), "AD".into()],
            vec!["FR".into(), "AD".into()],
        ] {
            let selected = inventory
                .select_any(&countries, require_https)
                .await
                .unwrap();
            assert_eq!(selected.authority(), cached.authority());
            assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
            let inner = inventory.inner.read().await;
            let unknown = inner.records.get(&unknown.authority()).unwrap();
            assert!(!unknown.probe_attempts.contains_key("tcp"));
            assert!(!unknown.probe_attempts.contains_key("http"));
            assert!(!unknown.probe_attempts.contains_key("https"));
        }
        let mut faster = cached.clone();
        faster.host = "ad-ready.example".into();
        faster.country = Some("AD".into());
        faster.latency_ms = Some(5);
        inventory.add_or_update(vec![faster.clone()]).await;
        for countries in [
            vec!["FR".into(), "AD".into()],
            vec!["AD".into(), "FR".into()],
        ] {
            assert_eq!(
                inventory
                    .select_any(&countries, require_https)
                    .await
                    .unwrap()
                    .authority(),
                faster.authority()
            );
            assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        }
    }
}

#[tokio::test]
async fn multi_country_selection_loads_each_country_at_most_once_per_call() {
    let provider = Arc::new(CountingProvider {
        records: Vec::new(),
        calls: AtomicUsize::new(0),
        delay: Duration::ZERO,
    });
    let inventory = ProxyInventory::new(InventoryConfig::default(), Some(provider.clone()), None);
    let countries = vec!["AD".to_string(), "FR".to_string(), "WF".to_string()];

    assert!(inventory.select_any(&countries, true).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), countries.len());
    inventory
        .allow_provider_refresh_after_observed_failure(&countries)
        .await;
    assert!(inventory.select_any(&countries, true).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), countries.len());
    {
        let mut inner = inventory.inner.write().await;
        for at in inner.country_last_refresh_attempts.values_mut() {
            *at = Instant::now() - Duration::from_secs(11);
        }
    }
    assert!(inventory.select_any(&countries, true).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), countries.len() * 2);

    struct ProgressiveProvider(Arc<AtomicUsize>, Option<tokio::task::Id>);
    struct PendingLoad(Arc<AtomicUsize>);
    impl Drop for PendingLoad {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[async_trait]
    impl ProxyDataProvider for ProgressiveProvider {
        async fn load_proxies(&self, _: ProxyLoadRequest) -> Result<Vec<ProxyRecord>> {
            panic!("progressive selection must not wait for the list adapter");
        }
        async fn load_proxies_progressively(
            &self,
            request: ProxyLoadRequest,
            sink: Arc<dyn ProxyBatchSink>,
        ) -> Result<()> {
            assert_ne!(
                tokio::task::try_id(),
                self.1,
                "provider refresh must not extend the caller's polling stack"
            );
            assert!(
                ProxySearchContext::current().is_none(),
                "provider requests must not inherit business acceptance state"
            );
            let _pending = PendingLoad(self.0.clone());
            let mut candidate = record(ProxyRuntimeStatus::Ok);
            candidate.country = Some(request.countries[0].clone());
            sink.publish(vec![candidate]).await?;
            futures::future::pending().await
        }
    }
    let cancelled = Arc::new(AtomicUsize::new(0));
    let inventory = ProxyInventory::new(
        InventoryConfig::default(),
        Some(Arc::new(ProgressiveProvider(
            cancelled.clone(),
            tokio::task::try_id(),
        ))),
        None,
    );
    let discovery_started = Instant::now();
    let selected = tokio::time::timeout(
        Duration::from_secs(1),
        inventory.select_any(&countries, true),
    )
    .await
    .expect("published candidates must precede provider completion")
    .unwrap();
    let first_candidate_elapsed = discovery_started.elapsed();
    assert!(countries.contains(selected.country.as_ref().unwrap()));
    tokio::time::timeout(Duration::from_secs(1), async {
        while cancelled.load(Ordering::SeqCst) == 0
            || inventory.provider_slots.available_permits() != 2
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("last-subscriber cancellation must stop owned provider tasks and release slots");
    eprintln!(
        "lot7 progressive discovery: first_candidate_us={}, cancelled_pending_loads={}",
        first_candidate_elapsed.as_micros(),
        cancelled.load(Ordering::SeqCst)
    );
    assert!(
        cancelled.load(Ordering::SeqCst) > 0,
        "pending producers must be dropped with the final subscriber"
    );

    let strict_inventory = ProxyInventory::new(
        InventoryConfig::default(),
        None,
        Some(Arc::new(ProxyProbe::new(crate::core::ProbeConfig {
            http_probe_url: None,
            https_probe_url: Some("https://probe.example/".into()),
            mode: ProbeMode::Strict,
            ..Default::default()
        }))),
    );
    #[cfg(feature = "persistence")]
    let strict_inventory = {
        struct YieldingRepository(Arc<dyn ProxyRepository>);
        #[async_trait]
        impl ProxyRepository for YieldingRepository {
            async fn get(&self, key: &crate::core::ProxyKey) -> AnyResult<Option<ProxyRecord>> {
                self.0.get(key).await
            }
            async fn get_failure(
                &self,
                key: &crate::core::ProxyKey,
            ) -> AnyResult<Option<ProxyRecord>> {
                self.0.get_failure(key).await
            }
            async fn evict(&self, record: &ProxyRecord) -> AnyResult<()> {
                self.0.evict(record).await
            }
            async fn find_by_country(&self, country: &str) -> AnyResult<Vec<ProxyRecord>> {
                self.0.find_by_country(country).await
            }
            async fn save_many(&self, records: &[ProxyRecord]) -> AnyResult<()> {
                tokio::time::sleep(Duration::from_millis(20)).await;
                self.0.save_many(records).await
            }
            async fn clear_superseded_destination_failures(
                &self,
                key: &crate::core::ProxyKey,
                destination: &Destination,
                accepted_at: SystemTime,
            ) -> AnyResult<()> {
                self.0
                    .clear_superseded_destination_failures(key, destination, accepted_at)
                    .await
            }
            async fn delete(&self, key: &crate::core::ProxyKey) -> AnyResult<()> {
                self.0.delete(key).await
            }
        }
        strict_inventory.with_proxy_repository(Arc::new(YieldingRepository(Arc::new(
            TypedProxyRepository::new(memory_proxy_store().unwrap()),
        ))))
    };
    let mut servers = Vec::new();
    for _ in 0..3 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await?;
                let Ok(first) = stream.read_u8().await else {
                    continue;
                };
                let mut head = vec![first];
                while !head.ends_with(b"\r\n\r\n") {
                    head.push(stream.read_u8().await?);
                    anyhow::ensure!(head.len() <= 8192, "fixture header limit");
                }
                stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;
                stream.shutdown().await?;
                return Ok::<_, anyhow::Error>(1);
            }
        });
        let mut candidate = record(ProxyRuntimeStatus::Unknown);
        candidate.host = address.ip().to_string();
        candidate.port = address.port();
        candidate.protocol = Some(ProxyProtocol::Http);
        candidate.declarations.clear();
        candidate.country = Some("FR".into());
        candidate.http_forwarding = ProxyCapabilityStatus::Unknown;
        candidate.https_tunnel = ProxyCapabilityStatus::Unknown;
        candidate.destination_tls = ProxyCapabilityStatus::Unknown;
        candidate.last_checked = None;
        candidate.tcp_reachable = None;
        candidate.probe_attempts.clear();
        strict_inventory.add_or_update(vec![candidate]).await;
        servers.push(server);
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        assert!(strict_inventory.select_any(&countries, true).await.is_err());
        for server in servers {
            assert_eq!(server.await.unwrap().unwrap(), 1);
        }
    })
    .await
    .expect("prechecks and validations must progress while persistence yields");
}

#[tokio::test]
async fn multi_country_failure_reports_every_attempted_country() {
    let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
    let countries = vec!["AD".to_string(), "FR".to_string(), "WF".to_string()];

    let error = inventory
        .select_any(&countries, true)
        .await
        .expect_err("selection should fail without records or provider")
        .to_string();

    assert!(error.contains("AD, FR, WF"));
}
