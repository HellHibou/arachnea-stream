use super::*;
use arachnea_proxy::core::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

async fn read_head(stream: &mut (impl AsyncRead + Unpin)) -> anyhow::Result<String> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        bytes.push(stream.read_u8().await?);
        anyhow::ensure!(bytes.len() <= 8192, "fixture header limit");
    }
    Ok(String::from_utf8(bytes)?)
}

#[tokio::test]
async fn keep_alive_tls_503_keeps_exact_selection_and_positive_phases() -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(25), async {
        let shared_runtime = Arc::new(arachnea_proxy::server::LoopbackProxyRuntime::default());
        struct CountryProvider {
            client: crate::client::ArachneaHttpClient,
            url: String,
        }
        #[async_trait::async_trait]
        impl IpCountryDataProvider for CountryProvider {
            async fn load_ip_countries(
                &self,
            ) -> arachnea_proxy::core::Result<Vec<IpCountryRecord>> {
                Ok(Vec::new())
            }
            async fn resolve_current_country(
                &self,
            ) -> arachnea_proxy::core::Result<Option<String>> {
                let response = self.client.get(&self.url).send().await.map_err(|error| {
                    ProxyError::Config(format!("country fixture request: {error}"))
                })?;
                let country = response.text().await.map_err(|error| {
                    ProxyError::Config(format!("country fixture body: {error}"))
                })?;
                Ok(Some(country))
            }
        }
        let country_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let country_address = country_listener.local_addr()?;
        let country_server = tokio::spawn(async move {
            let (mut stream, _) = country_listener.accept().await?;
            read_head(&mut stream).await?;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nLU")
                .await?;
            stream.shutdown().await?;
            Ok::<_, anyhow::Error>(())
        });
        let country_client = crate::client::ArachneaHttpClient::new(
            ArachneaHttpConfig::builder()
                .proxy_runtime(shared_runtime.clone())
                .proxy(crate::config::HttpProxyConfig::Arachnea(
                    ArachneaProxyCore::new(UsageProfile::SystemRelay.config())?,
                ))
                .build()?,
        )
        .await?;
        let resolver = IpCountryResolver::new(
            IpCountryResolverConfig::default(),
            Some(Arc::new(CountryProvider {
                client: country_client,
                url: format!("http://{country_address}/country"),
            })),
            None,
        );
        let business_search = ProxySearchContext::default();
        business_search
            .scope(async {
                assert_eq!(resolver.resolve_current_country().await?, Some("LU".into()));
                assert!(
                    business_search.can_start(),
                    "country detection must not accept the pending business request"
                );
                business_search.technical(async {}).await?;
                Ok::<_, anyhow::Error>(())
            })
            .await?;
        country_server.await??;
        #[derive(Clone, Copy, PartialEq)]
        enum DeliveryFixture {
            ConnectRejected,
            BodySent,
            UntrustedTls,
            RedirectConnect,
            ConnectTimeout,
        }
        for stage in [
            DeliveryFixture::ConnectRejected,
            DeliveryFixture::BodySent,
            DeliveryFixture::UntrustedTls,
            DeliveryFixture::RedirectConnect,
            DeliveryFixture::ConnectTimeout,
        ] {
            let url = if stage == DeliveryFixture::BodySent {
                "http://origin.example/login"
            } else {
                "https://origin.example/login"
            };
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let address = listener.local_addr()?;
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await?;
                let head = read_head(&mut stream).await?;
                if stage != DeliveryFixture::BodySent {
                    anyhow::ensure!(
                        head.starts_with("CONNECT "),
                        "expected tunnel setup before POST"
                    );
                    if stage == DeliveryFixture::ConnectTimeout {
                        let mut byte = [0; 1];
                        anyhow::ensure!(
                            stream.read(&mut byte).await? == 0,
                            "timed out CONNECT must not receive POST bytes"
                        );
                        return Ok::<_, anyhow::Error>(());
                    }
                    if stage == DeliveryFixture::UntrustedTls {
                        stream
                            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                            .await?;
                        let tls = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
                                    .with_safe_default_protocol_versions()?.with_no_client_auth().with_single_cert(
                                        vec![rustls::pki_types::CertificateDer::from(include_bytes!("../../../arachnea-proxy/tests/fixtures/localhost-cert.der").to_vec())],
                                        rustls::pki_types::PrivatePkcs8KeyDer::from(include_bytes!("../../../arachnea-proxy/tests/fixtures/localhost-key.der").to_vec()).into(),
                                    )?;
                        if let Ok(mut tls) = tokio_rustls::TlsAcceptor::from(Arc::new(tls))
                            .accept(stream)
                            .await
                        {
                            let mut byte = [0; 1];
                            anyhow::ensure!(
                                !matches!(tls.read(&mut byte).await, Ok(count) if count > 0),
                                "untrusted TLS must not receive a POST"
                            );
                        }
                        return Ok::<_, anyhow::Error>(());
                    }
                    stream.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;
                } else {
                    anyhow::ensure!(head.starts_with("POST "), "expected business POST");
                    let mut body = [0; 3];
                    stream.read_exact(&mut body).await?;
                    anyhow::ensure!(&body == b"abc", "expected complete business body");
                }
                stream.shutdown().await?;
                Ok::<_, anyhow::Error>(())
            });
            let attempt_started = tokio::time::Instant::now();
            let search = ProxySearchContext::default();
            let engine = search
                .scope(async {
                    RquestEngine::new(
                        &ArachneaHttpConfig::builder()
                            .request_timeout(Duration::from_secs(25))
                            .build()?,
                        Some(&format!("http://{address}")),
                        None,
                    )
                })
                .await?;
            let redirects = if stage == DeliveryFixture::RedirectConnect {
                vec!["https://prior.example/login".to_string()]
            } else {
                Vec::new()
            };
            let error = engine
                .send_for_proxy(
                    EngineRequest {
                        method: ::http::Method::POST,
                        url: url.into(),
                        headers: Default::default(),
                        body: Some(bytes::Bytes::from_static(b"abc")),
                    },
                    None,
                    None,
                    url,
                    &redirects,
                )
                .await
                .err()
                .expect("fixture must reject setup or close before headers");
            let ArachneaHttpError::ProxyAttempt {
                delivery, source, ..
            } = error
            else {
                anyhow::bail!("expected attributed proxy error");
            };
            let not_sent = matches!(
                stage,
                DeliveryFixture::ConnectRejected
                    | DeliveryFixture::UntrustedTls
                    | DeliveryFixture::ConnectTimeout
            );
            assert_eq!(
                delivery,
                if not_sent {
                    ProxyRequestDelivery::NotSent
                } else {
                    ProxyRequestDelivery::PossiblySent
                }
            );
            assert_eq!(
                proxy_retry_allowed("POST", false, delivery, false),
                not_sent
            );
            if stage == DeliveryFixture::UntrustedTls {
                assert!(source.to_string().contains("tls_certificate"));
            }
            if stage == DeliveryFixture::ConnectTimeout {
                assert!(attempt_started.elapsed() >= Duration::from_secs(14));
                assert!(attempt_started.elapsed() < Duration::from_secs(20));
            }
            server.await??;
        }
        let cert = include_bytes!("../../../arachnea-proxy/tests/fixtures/localhost-cert.der");
        let key = include_bytes!("../../../arachnea-proxy/tests/fixtures/localhost-key.der");
        let ca = include_bytes!("../../../arachnea-proxy/tests/fixtures/test-ca.der");
        let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        )).with_safe_default_protocol_versions()?.with_no_client_auth().with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(cert.to_vec())],
            rustls::pki_types::PrivatePkcs8KeyDer::from(key.to_vec()).into(),
        )?;
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let connect = read_head(&mut stream).await?;
            anyhow::ensure!(connect.starts_with("CONNECT localhost:443 "), "expected CONNECT");
            stream.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;
            let mut tls = acceptor.accept(stream).await?;
            let mut requests = Vec::new();
            for response in [
                &b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\nok"[..],
                &b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndown"[..],
            ] {
                requests.push(read_head(&mut tls).await?);
                if response.ends_with(b"ok") {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    tls.write_all(&response[..response.len() - 2]).await?;
                    tokio::time::sleep(Duration::from_millis(450)).await;
                    tls.write_all(b"ok").await?;
                } else {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    tls.write_all(response).await?;
                }
            }
            tls.shutdown().await?;
            // Both requests above were read from the same accepted TCP/TLS stream.
            Ok::<_, anyhow::Error>(requests)
        });
        let proxy: ProxyRecord = serde_json::from_value(serde_json::json!({
            "host": "127.0.0.1", "port": address.port(), "country": "BE",
            "protocol": "Http", "status": "ok", "supports_https": true,
            "http_forwarding": "available", "https_tunnel": "available"
        }))?;
        let mut other = proxy.clone();
        other.host = "other.example".into();
        other.country = Some("FR".into());
        let authority = proxy.authority();
        let other_authority = other.authority();
        let inventory = ProxyInventory::new(InventoryConfig::default(), None, None);
        inventory.add_or_update(vec![proxy, other]).await;
        let core = ArachneaProxyCore::new(ProxyConfig::default())?
            .with_proxy_inventory(inventory)
            .with_parameter_handler(DynamicCountryRoutingProxyHandler::new(CoexistencePolicy::DynamicOnly));
        let destination = Destination::host_port("localhost", 443).with_protocol(ApplicationProtocol::Https);
        let select = |country: &str| {
            let mut context = ClientContext::new();
            context.insert(PROXY_PARAMETER_COUNTRIES, ClientParameter::String(format!("[\"{country}\"]")));
            ConnectRequest::new(destination.clone()).with_client_context(context)
        };
        let selected = core.select_for_request(select("BE")).await?;
        let token = selected.selection_token.clone().unwrap();
        let loopback = arachnea_proxy::connectors::ArachneaRquestLoopback::start_on_runtime(
            shared_runtime.clone(), selected.core.clone(), selected.core.parameter_definitions(),
            ClientContext::new(), false,
        ).await?;
        assert_eq!(url::Url::parse(loopback.proxy_url())?.port(), shared_runtime.local_addr().map(|addr| addr.port()));
        let trusted = rquest::tls::CertStore::from_der_certs([ca.as_slice()])?;
        let client = rquest::Client::builder().cert_store(trusted)
            .proxy(rquest::Proxy::all(loopback.proxy_url())?)
            .redirect(rquest::redirect::Policy::none()).build()?;
        let engine = RquestEngine::new(
            &ArachneaHttpConfig::default(), Some(loopback.proxy_url()), Some(client),
        )?;
        let request = || EngineRequest {
            method: ::http::Method::GET, url: "https://localhost/content".into(),
            headers: Default::default(), body: None,
        };
        let search = ProxySearchContext::new(ProxySearchLimits {
            target: Duration::from_millis(100), extension: Duration::from_millis(300), ..Default::default()
        });
        search.start_origin(token.endpoint(), token.destination())?;
        let (first, first_decision) = search.scope(search.origin(engine.send_for_proxy(request(), Some(token.clone()), None, "https://localhost/content", &[]))).await??;
        assert!(!search.can_start());
        assert!(search.remaining().is_zero());
        assert!(!proxy_retry_allowed("POST", false, ProxyRequestDelivery::PossiblySent, false));
        assert!(!proxy_retry_allowed("POST", false, ProxyRequestDelivery::ResponseReceived, true));
        assert!(proxy_retry_allowed("POST", false, ProxyRequestDelivery::NotSent, false));
        assert!(proxy_retry_allowed("POST", true, ProxyRequestDelivery::ResponseReceived, true));
        assert!(!proxy_retry_allowed("GET", false, ProxyRequestDelivery::ResponseReceived, false));
        assert_eq!(first_decision, arachnea_proxy::core::http::response_classifier::ProxyResponseDecision::Accepted);
        assert_eq!(first.status.as_u16(), 200);
        token.record_accepted().await;
        let independent = core.select_for_request(select("FR")).await?;
        independent.selection_token.unwrap().record_failure(ProxyDestinationFailureReason::BlockedByOrigin).await;
        struct PrefixPolicy;
        impl arachnea_proxy::core::http::response_classifier::ProxyResponseClassifier for PrefixPolicy {
            fn body_requirement(&self) -> arachnea_proxy::core::http::response_classifier::ProxyBodyRequirement {
                arachnea_proxy::core::http::response_classifier::ProxyBodyRequirement::Prefix(2)
            }
            fn classify(&self, view: &arachnea_proxy::core::http::response_classifier::ProxyResponseView<'_>) -> arachnea_proxy::core::Result<arachnea_proxy::core::http::response_classifier::ProxyResponseDecision> {
                use arachnea_proxy::core::http::response_classifier::*;
                assert_eq!(view.body, b"do");
                assert_eq!(view.body_state, ProxyBodyState::Limited);
                assert_eq!(view.method, "GET");
                assert!(view.selection.is_some());
                HttpStatusProxyResponseClassifier::default().classify(view)
            }
        }
        let policy = arachnea_proxy::core::http::response_classifier::ProxyResponsePolicy(std::sync::Arc::new(PrefixPolicy));
        let reuse_search = ProxySearchContext::default();
        let reused = reuse_search.scope(core.select_for_request(select("BE"))).await?;
        let reused_token = reused.selection_token.unwrap();
        assert!(reused_token.reuses_destination());
        assert_eq!(reused_token.provenance(), arachnea_proxy::core::ProxySelectionProvenance::AcceptedDestination);
        let short_engine = RquestEngine { request_timeout: Duration::from_millis(50), ..engine.clone() };
        let (second, second_decision) = reuse_search.scope(short_engine.send_for_proxy(request(), Some(reused_token), Some(&policy), "https://localhost/content", &[])).await?;
        assert_eq!(second_decision, arachnea_proxy::core::http::response_classifier::ProxyResponseDecision::Inconclusive);
        assert_eq!(second.status.as_u16(), 503);
        assert_eq!(second.body.as_ref(), b"down");
        let records = core.proxy_inventory().unwrap().all_records().await;
        let actual = records.iter().find(|record| record.authority() == authority).unwrap();
        assert_eq!(actual.status, ProxyRuntimeStatus::Ok);
        assert!(actual.destination_failures.is_empty());
        assert!(actual.observations.iter().any(|item| {
            item.context.phase == ProxyObservationPhase::Tunnel
                && item.outcome == ProxyObservationOutcome::Succeeded
        }));
        let tls = actual.observations.iter().find(|item| item.context.phase == ProxyObservationPhase::DestinationTls).unwrap();
        assert_eq!(tls.outcome, ProxyObservationOutcome::Succeeded);
        assert!(tls.last_succeeded_at.is_some());
        let http = actual.observations.iter().find(|item| item.context.phase == ProxyObservationPhase::DestinationHttp).unwrap();
        assert_eq!(http.http_status, Some(503));
        assert_eq!(http.outcome, ProxyObservationOutcome::Succeeded);
        assert_eq!(records.iter().find(|r| r.authority() == other_authority).unwrap().destination_failures.len(), 1);
        assert_eq!(server.await??.len(), 2);
        drop((engine, loopback));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            read_head(&mut stream).await?;
            stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndo").await?;
            stream.shutdown().await?;
            Ok::<_, anyhow::Error>(())
        });
        let proxy: ProxyRecord = serde_json::from_value(serde_json::json!({
            "host": "127.0.0.1", "port": address.port(), "country": "US",
            "protocol": "Http", "status": "ok", "http_forwarding": "available"
        }))?;
        core.proxy_inventory().unwrap().add_or_update(vec![proxy]).await;
        let body_destination = Destination::host_port("body.example", 80).with_protocol(ApplicationProtocol::Http);
        let body_request = || {
            let mut context = ClientContext::new();
            context.insert(PROXY_PARAMETER_COUNTRIES, ClientParameter::String("[\"US\"]".into()));
            ConnectRequest::new(body_destination.clone()).with_client_context(context)
        };
        let body_search = ProxySearchContext::default();
        let selected = body_search.scope(core.select_for_request(body_request())).await?;
        let token = selected.selection_token.unwrap();
        let loopback = arachnea_proxy::connectors::ArachneaRquestLoopback::start_on_runtime(
            shared_runtime.clone(), selected.core.clone(), selected.core.parameter_definitions(),
            ClientContext::new(), false,
        ).await?;
        assert_eq!(url::Url::parse(loopback.proxy_url())?.port(), shared_runtime.local_addr().map(|addr| addr.port()));
        let engine = RquestEngine::new(&ArachneaHttpConfig::default(), Some(loopback.proxy_url()), Some(loopback.client()?))?;
        let error = body_search.scope(engine.send_for_proxy(EngineRequest {
            method: ::http::Method::GET, url: "http://body.example/content".into(),
            headers: Default::default(), body: None,
        }, Some(token.clone()), None, "http://body.example/content", &[])).await.err().expect("truncated body must fail");
        assert!(matches!(&error, ArachneaHttpError::ProxyAttempt { delivery: ProxyRequestDelivery::ResponseReceived, .. }));
        assert!(!proxy_retry_allowed("GET", false, ProxyRequestDelivery::ResponseReceived, false));
        let next_search = ProxySearchContext::default();
        let next = tokio::time::timeout(Duration::from_secs(1), next_search.scope(core.select_for_request(body_request()))).await??;
        assert_eq!(next.selection_token.as_ref().unwrap().endpoint(), token.endpoint(), "retaining a body error must not retain discovery ownership");
        server.await??;
        drop((error, token, next, engine, loopback));
        Ok(())
    }).await?
}
