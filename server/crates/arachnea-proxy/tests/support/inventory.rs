use super::*;

#[cfg(feature = "persistence")]
pub(super) struct StaticProvider {
    pub(super) records: Vec<ProxyRecord>,
}

#[cfg(feature = "persistence")]
#[async_trait]
impl ProxyDataProvider for StaticProvider {
    async fn load_proxies(
        &self,
        _request: crate::core::ProxyLoadRequest,
    ) -> crate::core::Result<Vec<ProxyRecord>> {
        Ok(self.records.clone())
    }
}

pub(super) struct CountingProvider {
    pub(super) records: Vec<ProxyRecord>,
    pub(super) calls: AtomicUsize,
    pub(super) delay: Duration,
}

#[async_trait]
impl ProxyDataProvider for CountingProvider {
    async fn load_proxies(
        &self,
        _request: crate::core::ProxyLoadRequest,
    ) -> crate::core::Result<Vec<ProxyRecord>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        Ok(self.records.clone())
    }
}

pub(super) struct FailingProvider {
    pub(super) calls: AtomicUsize,
    pub(super) delay: Duration,
}

#[async_trait]
impl ProxyDataProvider for FailingProvider {
    async fn load_proxies(
        &self,
        _request: crate::core::ProxyLoadRequest,
    ) -> crate::core::Result<Vec<ProxyRecord>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        Err(ProxyError::RouteUnavailable(
            "test provider unavailable".to_string(),
        ))
    }
}

pub(super) fn record(status: ProxyRuntimeStatus) -> ProxyRecord {
    ProxyRecord {
        health_evidence: Default::default(),
        observations: Vec::new(),
        probe_attempts: Default::default(),
        tcp_reachable: None,
        protocol: Some(ProxyProtocol::Socks5),
        host: "merged.example".to_string(),
        port: 1080,
        country: Some("BE".to_string()),
        supports_https: Some(true),
        declarations: vec![ProxyDeclaration {
            source: "test-provider".to_string(),
            protocol: Some(ProxyProtocol::Socks5),
            supports_https: Some(true),
        }],
        status,
        http_forwarding: ProxyCapabilityStatus::Available,
        https_tunnel: ProxyCapabilityStatus::Available,
        destination_tls: ProxyCapabilityStatus::Unknown,
        proxy_tls_certificate: ProxyCapabilityStatus::Unknown,
        latency_ms: Some(25),
        failure_count: 0,
        authentication_required: Some(false),
        availability: ProxyAvailabilityHint::High,
        destination_failures: Vec::new(),
        last_checked: Some(SystemTime::now()),
        last_validated_at: Some(SystemTime::now()),
        cooldown_until: None,
    }
}

#[cfg(feature = "persistence")]
pub(super) struct RejectingPreferenceStore {
    pub(super) writes: AtomicUsize,
}

#[cfg(feature = "persistence")]
#[derive(Clone)]
pub(super) struct CapturedLogs(pub(super) Arc<std::sync::Mutex<Vec<u8>>>);

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

pub(super) async fn local_http_endpoint(
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

pub(super) async fn pinned_http_exchange(
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

pub(super) fn local_routing_core(inventory: ProxyInventory) -> crate::core::ArachneaProxyCore {
    crate::core::ArachneaProxyCore::new(crate::core::ProxyConfig::default())
        .unwrap()
        .with_proxy_inventory(inventory)
        .with_parameter_handler(crate::core::DynamicCountryRoutingProxyHandler::new(
            CoexistencePolicy::DynamicOnly,
        ))
}

pub(super) async fn select_local_route(
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
