pub(crate) use super::test_support::configure_test_application_data_dir;
use super::test_support::*;
#[cfg(feature = "arachnea-proxy")]
#[tokio::test]
async fn replacement_preserves_shared_proxy_history_during_active_response() -> anyhow::Result<()> {
    configure_test_application_data_dir();
    use arachnea_proxy::core::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let store = memory_proxy_store()?;
            let session_store = memory_session_store()?;
            let old = ScraperAgregator::new_with_proxy_handle_and_typed_stores(
                SharedProxyConfigHandle::new(), store.clone(), session_store.clone(),
            );
            let mut replacement = ScraperAgregator::new_with_proxy_handle_and_typed_stores(
                SharedProxyConfigHandle::with_runtime(old.proxy_handle.proxy_runtime()), store, session_store,
            );
            replacement.set_proxy_repositories(old.proxy_repository.clone(), old.proxy_preferences.clone());
            let snapshot = old.provider_snapshot(&[]);
            let runtime = old.proxy_handle.proxy_runtime();
            assert!(Arc::ptr_eq(&runtime, &snapshot.proxy_handle.proxy_runtime()));
            assert!(Arc::ptr_eq(&runtime, &replacement.proxy_handle.proxy_runtime()));
            let client = old.create_http_client(ScraperHttpConfig::default());
            assert!(Arc::ptr_eq(&runtime, &client.proxy_handle().proxy_runtime()));
            assert!(Arc::ptr_eq(&snapshot.proxy_preferences, &old.proxy_preferences));
            assert!(Arc::ptr_eq(&replacement.proxy_repository, &old.proxy_repository));
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
                received_tx.send(()).map_err(|_| anyhow::anyhow!("receiver dropped"))?;
                release_rx.await?;
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").await?;
                stream.shutdown().await?;
                Ok::<_, anyhow::Error>(())
            });
            let proxy: ProxyRecord = serde_json::from_value(serde_json::json!({
                "host": "127.0.0.1", "port": address.port(), "country": "BE", "protocol": "Http",
                "status": "ok", "http_forwarding": "available", "https_tunnel": "available"
            }))?;
            old.proxy_repository.save_many(&[proxy.clone()]).await?;
            let inventory = ProxyInventory::new(InventoryConfig::default(), None, None)
                .with_proxy_repository(old.proxy_repository.clone())
                .with_preference_repository(old.proxy_preferences.clone());
            inventory.add_or_update(vec![proxy.clone()]).await;
            let core = ArachneaProxyCore::new(ProxyConfig::default())?.with_proxy_inventory(inventory)
                .with_parameter_handler(DynamicCountryRoutingProxyHandler::new(CoexistencePolicy::DynamicOnly));
            let mut context = ClientContext::new();
            context.insert(PROXY_PARAMETER_COUNTRIES, ClientParameter::String("[\"BE\"]".into()));
            let destination = Destination::host_port("origin.example", 80).with_protocol(ApplicationProtocol::Http);
            let selected = core.select_for_request(ConnectRequest::new(destination.clone()).with_client_context(context)).await?;
            let token = selected.selection_token.clone().unwrap();
            let request = tokio::spawn(async move {
                let mut outgoing = selected.core.connect_http_request(ConnectRequest::new(destination)).await?;
                outgoing.stream.write_all(b"GET http://origin.example/ HTTP/1.1\r\nHost: origin.example\r\nConnection: close\r\n\r\n").await?;
                let mut response = Vec::new();
                outgoing.stream.read_to_end(&mut response).await?;
                anyhow::ensure!(response.starts_with(b"HTTP/1.1 200"), "expected success");
                token.record_http_response(200).await;
                token.record_accepted().await;
                Ok::<_, anyhow::Error>(())
            });
            received_rx.await?;
            drop(old);
            release_tx.send(()).map_err(|_| anyhow::anyhow!("server dropped"))?;
            request.await??;
            server.await??;
            let restored = ProxyInventory::new(InventoryConfig::default(), None, None)
                .with_proxy_repository(replacement.proxy_repository.clone())
                .with_preference_repository(replacement.proxy_preferences.clone());
            let destination = Destination::host_port("origin.example", 80).with_protocol(ApplicationProtocol::Http);
            assert_eq!(restored.select_for_destination("BE", false, Some(&destination)).await?.authority(), proxy.authority());
            let key = ProxyDestinationPreference::destination_key(&destination);
            assert_eq!(snapshot.proxy_preferences.get(&key).await?.unwrap().endpoints.len(), 1);
            Ok(())
        }).await?
}

use super::*;

use std::collections::HashMap;

#[tokio::test]
async fn execute_query_async_keeps_successes_in_config_order() {
    let (aggregator, dir) = setup_aggregator_sources(&[
        ("first", &static_source("first", "first")),
        ("second", &static_source("second", "second")),
    ]);

    let result = aggregator
        .execute_query_async(
            &RequestControlerContext::default(),
            QueryParameters::default(),
            "test",
            "probe",
            &HashMap::new(),
            None,
            None,
            None,
            None,
            None,
            "probe",
        )
        .await;

    assert!(result.is_ok(), "full success should not produce errors");
    assert_eq!(result.data.len(), 2);
    assert_eq!(value_from_row(&result.data[0]), Some("first"));
    assert_eq!(value_from_row(&result.data[1]), Some("second"));
    #[cfg(feature = "arachnea-proxy")]
    {
        use futures::StreamExt;
        let mut batches = aggregator.proxy_source_batches("test", "probe", HashMap::new());
        let mut values = Vec::new();
        while let Some(batch) = batches.next().await {
            for row in batch.expect("discovery source should succeed") {
                assert!(row.contains_key("proxy_source"));
                values.push(value_from_row(&row).unwrap().to_string());
            }
        }
        values.sort();
        assert_eq!(values, ["first", "second"]);
    }
    std::fs::remove_dir_all(dir).expect("test service directory should be removed");
}

#[tokio::test]
async fn execute_query_async_preserves_partial_success_and_error_code() {
    let failing_url = "not-a-valid-url";
    let (aggregator, dir) = setup_aggregator_sources(&[
        ("success", &static_source("success", "kept")),
        ("failure", &failing_html_source("failure", &failing_url)),
    ]);

    let result = aggregator
        .execute_query_async(
            &RequestControlerContext::default(),
            QueryParameters::default(),
            "test",
            "probe",
            &HashMap::new(),
            None,
            None,
            None,
            None,
            None,
            "probe",
        )
        .await;

    assert_eq!(
        result.data.len(),
        1,
        "successful source data should be kept"
    );
    assert_eq!(value_from_row(&result.data[0]), Some("kept"));
    assert_eq!(result.errors.len(), 1, "failed source should be reported");
    let error = &result.errors[0];
    assert_eq!(error.operation, "probe");
    assert_eq!(error.source.as_deref(), Some("failure"));
    assert!(matches!(&error.origin, ScraperErrorOrigin::Backend));
    assert!(error.code.to_string().starts_with("ARACHNEA_E"));
    assert!(!error.message.is_empty());
    #[cfg(feature = "arachnea-proxy")]
    {
        use arachnea_proxy::core::{
            ArachneaProxyCore, CoexistencePolicy, DynamicCountryRoutingProxyHandler, ProxyConfig,
        };
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                head.push(stream.read_u8().await.unwrap());
                assert!(head.len() <= 8192);
            }
            assert!(!String::from_utf8_lossy(&head)
                .to_ascii_lowercase()
                .contains("arachnea-proxy-fallback-direct"));
            let body = "<html><title>recovered</title></html>";
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let yaml = failing_html_source("fallback", &url).replace(
            "id: fallback",
            "id: fallback\nhttp:\n  proxy_countries: [BE]",
        );
        let (aggregator, fallback_dir) = setup_aggregator_sources(&[
            ("success", &static_source("success", "kept")),
            ("fallback", &yaml),
        ]);
        let core = ArachneaProxyCore::new(ProxyConfig::default())
            .unwrap()
            .with_fallback_direct(true)
            .with_parameter_handler(DynamicCountryRoutingProxyHandler::new(
                CoexistencePolicy::DynamicOnly,
            ));
        aggregator
            .get_proxy_handle()
            .set_proxy(HttpProxyConfig::Arachnea(core));
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            aggregator.execute_query_async(
                &RequestControlerContext::default(),
                QueryParameters::from_cache_type(CacheType::ClientCache),
                "test",
                "probe",
                &HashMap::new(),
                None,
                None,
                None,
                None,
                None,
                "get_entry",
            ),
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(result.data.len(), 2);
        assert_eq!(value_from_row(&result.data[0]), Some("kept"));
        assert_eq!(value_from_row(&result.data[1]), Some("recovered"));
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].source.as_deref(), Some("fallback"));
        assert_eq!(result.errors[0].operation, "get_entry");
        assert!(result.errors[0].message.contains("using direct fallback"));
        assert!(result.global_etag.is_none());
        std::fs::remove_dir_all(fallback_dir).unwrap();
    }
    std::fs::remove_dir_all(dir).expect("test service directory should be removed");
}

#[tokio::test]
async fn execute_query_async_reports_all_failed_sources_in_config_order() {
    let failing_url = "not-a-valid-url";
    let (aggregator, dir) = setup_aggregator_sources(&[
        ("first", &failing_html_source("first", &failing_url)),
        ("second", &failing_html_source("second", &failing_url)),
    ]);

    let result = aggregator
        .execute_query_async(
            &RequestControlerContext::default(),
            QueryParameters::default(),
            "test",
            "probe",
            &HashMap::new(),
            None,
            None,
            None,
            None,
            None,
            "probe",
        )
        .await;

    assert!(
        result.data.is_empty(),
        "all failed sources should produce no data"
    );
    assert_eq!(result.errors.len(), 2);
    assert_eq!(result.errors[0].source.as_deref(), Some("first"));
    assert_eq!(result.errors[1].source.as_deref(), Some("second"));
    assert!(result
        .errors
        .iter()
        .all(|error| error.code.to_string().starts_with("ARACHNEA_E")));
    std::fs::remove_dir_all(dir).expect("test service directory should be removed");
}

#[tokio::test]
async fn execute_query_async_reports_pre_execution_error_without_source() {
    let aggregator = ScraperAgregator::new_with_proxy_handle(SharedProxyConfigHandle::new());
    let result = aggregator
        .execute_query_async(
            &RequestControlerContext::default(),
            QueryParameters::default(),
            "missing",
            "probe",
            &HashMap::new(),
            None,
            None,
            None,
            None,
            None,
            "probe",
        )
        .await;

    assert!(result.data.is_empty());
    assert_eq!(result.errors.len(), 1);
    let error = &result.errors[0];
    assert_eq!(error.operation, "probe");
    assert!(error.source.is_none());
    assert!(error.code.to_string().starts_with("ARACHNEA_E"));
}
