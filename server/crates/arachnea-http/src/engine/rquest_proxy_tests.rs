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
    tokio::time::timeout(Duration::from_secs(10), async {
        let cert = include_bytes!("../../../arachnea-proxy/src/core/tests/fixtures/localhost-cert.der");
        let key = include_bytes!("../../../arachnea-proxy/src/core/tests/fixtures/localhost-key.der");
        let ca = include_bytes!("../../../arachnea-proxy/src/core/tests/fixtures/test-ca.der");
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
                tls.write_all(response).await?;
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
        let loopback = arachnea_proxy::connectors::ArachneaRquestLoopback::start(selected.core).await?;
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
        let first = engine.send_for_proxy(request(), Some(token.clone())).await?;
        assert_eq!(first.status.as_u16(), 200);
        token.record_accepted().await;
        let independent = core.select_for_request(select("FR")).await?;
        independent.selection_token.unwrap().record_failure(ProxyDestinationFailureReason::BlockedByOrigin).await;
        let second = engine.send_for_proxy(request(), Some(token.clone())).await?;
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
        Ok(())
    }).await?
}
