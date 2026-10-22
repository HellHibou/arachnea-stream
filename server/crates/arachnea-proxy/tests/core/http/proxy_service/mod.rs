use super::options::{encode_proxy_opts, parse_opts_value, ProxyHttpOpts};
use super::proxied_url_with_countries;
use super::redirects::redirect_opts_encoded;
use super::urls::{encode_proxy_path, entry_point_location_prefix};
use crate::core::http::actions::{
    parse_proxy_action_header_value, ParsedProxyActionHeader, ProxyHttpRedirectActionConfig,
    ProxyHttpRemoveHeaderConfig, RemoveHeader, REMOVE_HEADER_ACTION_HEADER,
};
use crate::core::PROXY_HEADER_PARAMETER_COUNTRIES;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use std::collections::HashMap;
use url::Url;

fn decode_opts(encoded: &str) -> serde_json::Value {
    let bytes = URL_SAFE_NO_PAD.decode(encoded).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn parses_remove_header_json_action() {
    let action = parse_proxy_action_header_value(
        REMOVE_HEADER_ACTION_HEADER,
        r#"{"type":"onHttp302","headers":["Arachnea-Proxy-Countries"]}"#,
    )
    .unwrap();

    match action {
        Some(ParsedProxyActionHeader::RedirectAction(
            ProxyHttpRedirectActionConfig::RemoveHeader(action),
        )) => {
            assert_eq!(action.trigger, "onHttp302");
            assert_eq!(action.headers, vec!["Arachnea-Proxy-Countries".to_string()]);
        }
        _ => panic!("expected RemoveHeader action"),
    }
}

#[test]
fn proxied_url_encodes_remove_header_action() {
    let url = proxied_url_with_countries(
        "https://example.test/manifest.mpd",
        Some("/proxy"),
        &["fr".to_string(), " be ".to_string(), "FR".to_string()],
        &[RemoveHeader::on_http302([
            PROXY_HEADER_PARAMETER_COUNTRIES,
            REMOVE_HEADER_ACTION_HEADER,
        ])],
        &[],
    );

    let opts_encoded = url
        .split("/opts_")
        .nth(1)
        .and_then(|value| value.split('/').next())
        .expect("proxied URL should contain an opts segment");
    let opts = decode_opts(opts_encoded);
    let headers = opts
        .get("headers")
        .and_then(serde_json::Value::as_array)
        .expect("opts should contain headers");

    assert_eq!(headers.len(), 2);
    let country_header = headers
        .iter()
        .find(|header| header[0] == PROXY_HEADER_PARAMETER_COUNTRIES)
        .expect("country header should be present");
    let action_header = headers
        .iter()
        .find(|header| header[0] == REMOVE_HEADER_ACTION_HEADER)
        .expect("action header should be present");
    assert_eq!(country_header[1], r#"["BE","FR"]"#);

    let action_value: serde_json::Value = serde_json::from_str(action_header[1].as_str().unwrap())
        .expect("RemoveHeader value should be JSON");
    assert_eq!(action_value.get("type").unwrap(), "onHttp302");
    assert_eq!(
        action_value.get("headers").unwrap(),
        &serde_json::json!([
            PROXY_HEADER_PARAMETER_COUNTRIES,
            REMOVE_HEADER_ACTION_HEADER
        ])
    );
}

#[tokio::test]
async fn remove_header_updates_redirect_opts_on_http_302() {
    let opts = ProxyHttpOpts {
            headers: vec![
                (
                    "Arachnea-Proxy-Countries".to_string(),
                    r#"["FR"]"#.to_string(),
                ),
                (
                    REMOVE_HEADER_ACTION_HEADER.to_string(),
                    r#"{"type":"onHttp302","headers":["Arachnea-Proxy-Countries","Arachnea-Proxy-RemoveHeader"]}"#.to_string(),
                ),
                ("X-Keep".to_string(), "1".to_string()),
            ],
            cookies: HashMap::new(),
            proxy: serde_json::Value::Null,
            insecure_tls: false,
        fallback_direct: Some(false),
            follow_redirects: None,
            proxy_rejection_statuses: Vec::new(),
            proxy_retry_replayable: true,
            proxy_response_classifier: Some(crate::core::http::response_classifier::ProxyResponseClassifierConfig::Http403),
        };
    let encoded = encode_proxy_opts(&opts);
    let redirect_actions = vec![ProxyHttpRedirectActionConfig::RemoveHeader(
        ProxyHttpRemoveHeaderConfig {
            trigger: "onHttp302".to_string(),
            headers: vec![
                "Arachnea-Proxy-Countries".to_string(),
                REMOVE_HEADER_ACTION_HEADER.to_string(),
            ],
        },
    )];

    let next_encoded = redirect_opts_encoded(302, &encoded, Some(&opts), &redirect_actions);
    let next = decode_opts(&next_encoded);
    let headers = next
        .get("headers")
        .and_then(serde_json::Value::as_array)
        .unwrap();

    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0][0], "X-Keep");
    assert_eq!(headers[0][1], "1");
    assert_eq!(next["proxy_response_classifier"]["type"], "http403");
    assert_eq!(next["proxy_retry_replayable"], true);
    assert_eq!(next["fallback_direct"], false);
    assert_eq!(
        parse_opts_value(&next).unwrap().fallback_direct,
        Some(false)
    );
    assert_eq!(
        parse_opts_value(&serde_json::json!({"fallback_direct": true}))
            .unwrap()
            .fallback_direct,
        Some(true)
    );
    assert_eq!(
        parse_opts_value(&serde_json::Value::Null)
            .unwrap()
            .fallback_direct,
        None
    );
    for value in [
        serde_json::json!("true"),
        serde_json::json!(null),
        serde_json::json!(1),
    ] {
        assert!(parse_opts_value(&serde_json::json!({"fallback_direct": value})).is_err());
    }
    assert!(parse_opts_value(&serde_json::json!({ "proxy_retry_replayable": "yes" })).is_err());
    let parsed = parse_opts_value(&next).unwrap();
    assert!(parsed.proxy_response_classifier.is_some());
    use crate::core::http::response_classifier::*;
    for value in [
        serde_json::json!({"type": "unknown"}),
        serde_json::json!({"type": "statuses", "rejected_statuses": [99]}),
        serde_json::json!({"type": "statuses", "rejected_statuses": [200]}),
        serde_json::json!({"type": "statuses", "cooldown_seconds": 0}),
    ] {
        assert!(
            parse_opts_value(&serde_json::json!({ "proxy_response_classifier": value })).is_err()
        );
    }
    let policy =
        resolve_response_policy(parsed.proxy_response_classifier.as_ref(), &[500]).unwrap();
    let empty_headers = http::HeaderMap::new();
    for (status, expected) in [
        (200, "accepted"),
        (403, "rejected"),
        (451, "inconclusive"),
        (500, "inconclusive"),
    ] {
        let view = ProxyResponseView {
            method: "GET",
            requested_url: "https://example.test/",
            effective_url: "https://example.test/",
            redirect_urls: &[],
            status,
            headers: &empty_headers,
            selection: None,
            body: &[],
            body_state: ProxyBodyState::Unavailable,
        };
        let actual = match policy.0.classify(&view).unwrap() {
            ProxyResponseDecision::Accepted => "accepted",
            ProxyResponseDecision::RejectedForDestination { .. } => "rejected",
            ProxyResponseDecision::Inconclusive => "inconclusive",
        };
        assert_eq!(actual, expected);
    }

    use crate::core::{ArachneaProxyCore, ProxyConfig, ProxySearchContext, ProxySearchLimits};
    use arachnea_core::controler::{ControlerStreamInput, ResponseBody};
    use futures::StreamExt;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[derive(Debug)]
    struct CaptureContext(tokio::sync::mpsc::UnboundedSender<(tokio::task::Id, usize)>);
    impl crate::core::ProxyParameterHandler for CaptureContext {
        fn parameter_definitions(&self) -> Vec<crate::core::ParameterDefinition> {
            Vec::new()
        }
        fn proxy_from_parameters(
            &self,
            _: &crate::core::ClientContext,
        ) -> crate::core::Result<crate::core::ParameterHandlerDecision> {
            let current = ProxySearchContext::current().expect("inherited search context");
            let _ = self.0.send((tokio::task::id(), current.id()));
            Ok(crate::core::ParameterHandlerDecision::continue_without_proxy())
        }
    }

    for mode in [
        "redirect",
        "stream",
        "buffer",
        "cancel",
        "deadline",
        "drop_stream",
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release, ready) = tokio::sync::oneshot::channel::<()>();
        let (request_seen, on_request) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }
            request_seen.send(()).unwrap();
            if mode == "cancel" || mode == "deadline" {
                assert_eq!(socket.read(&mut [0u8; 1]).await.unwrap(), 0);
            } else if mode == "redirect" {
                socket.write_all(b"HTTP/1.1 302 Found\r\nLocation: https://media.example/next\r\nContent-Length: 0\r\n\r\n").await.unwrap();
            } else {
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: 4\r\n\r\n").await.unwrap();
                if mode == "drop_stream" {
                    assert_eq!(socket.read(&mut [0u8; 1]).await.unwrap(), 0);
                } else {
                    if mode == "stream" {
                        ready.await.unwrap();
                    }
                    socket.write_all(b"body").await.unwrap();
                }
            }
        });
        let search = ProxySearchContext::new(ProxySearchLimits {
            target: if mode == "deadline" {
                Duration::from_millis(150)
            } else {
                Duration::from_secs(5)
            },
            extension: Duration::ZERO,
        });
        let search_id = search.id();
        let (seen, mut contexts) = tokio::sync::mpsc::unbounded_channel();
        let core = Arc::new(
            ArachneaProxyCore::new(ProxyConfig::default())
                .unwrap()
                .with_parameter_handler(CaptureContext(seen)),
        );
        let input = ControlerStreamInput {
            path: format!("http:{}/127.0.0.1/start", address.port()),
            query: String::new(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: Vec::new(),
            entry_point: "/api/proxy".into(),
            force_buffer_response: mode == "buffer",
        };
        let parent = tokio::spawn(async move {
            let id = tokio::task::id();
            let response = search
                .scope(Box::pin(super::handle_proxy_http(core, input, true)))
                .await;
            (id, response)
        });
        let (child_id, observed_search) =
            tokio::time::timeout(Duration::from_secs(2), contexts.recv())
                .await
                .unwrap()
                .unwrap();
        assert_eq!(observed_search, search_id);
        tokio::time::timeout(Duration::from_secs(2), on_request)
            .await
            .unwrap()
            .unwrap();
        if mode == "cancel" {
            assert_ne!(child_id, parent.id());
            parent.abort();
            assert!(matches!(parent.await, Err(error) if error.is_cancelled()));
        } else {
            let (parent_id, response) = tokio::time::timeout(Duration::from_secs(2), parent)
                .await
                .unwrap()
                .unwrap();
            assert_ne!(child_id, parent_id);
            let response = response.unwrap();
            assert_eq!(
                response.status,
                if mode == "redirect" {
                    302
                } else if mode == "deadline" {
                    502
                } else {
                    200
                }
            );
            if mode == "redirect" {
                assert!(response
                    .headers
                    .iter()
                    .any(|(key, value)| key.eq_ignore_ascii_case("location")
                        && value.starts_with("/api/proxy/")
                        && value.contains("media.example/next")));
            } else if mode == "stream" {
                release.send(()).unwrap();
                let ResponseBody::Streamed(mut body) = response.body else {
                    panic!("stream must outlive its request task")
                };
                let mut bytes = Vec::new();
                while let Some(chunk) = body.next().await {
                    bytes.extend_from_slice(&chunk.unwrap());
                }
                assert_eq!(bytes, b"body");
            } else if mode == "drop_stream" {
                assert!(matches!(&response.body, ResponseBody::Streamed(_)));
                drop(response);
            } else if mode == "buffer" {
                assert!(
                    matches!(response.body, ResponseBody::Buffered(ref bytes) if bytes == b"body")
                );
            }
        }
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }
}

#[test]
fn encode_proxy_path_http_entry_point_is_root_relative() {
    let url = Url::parse("https://th2-edge-02.cdn.test/m6web/a.mpd").unwrap();
    assert_eq!(
        encode_proxy_path("/api/proxy", &url, "ABCD"),
        "/api/proxy/opts_ABCD/https://th2-edge-02.cdn.test/m6web/a.mpd"
    );
    assert_eq!(
        encode_proxy_path("/api/proxy", &url, ""),
        "/api/proxy/https://th2-edge-02.cdn.test/m6web/a.mpd"
    );
}

#[test]
fn entry_point_location_prefix_reduces_http_to_path() {
    assert_eq!(
        entry_point_location_prefix("http://0.0.0.0:8080/api/proxy"),
        "/api/proxy"
    );
    assert_eq!(
        entry_point_location_prefix("http://192.168.178.34:8080/api/proxy"),
        "/api/proxy"
    );
    assert_eq!(entry_point_location_prefix("http://host:8080"), "");
    // Custom scheme entry points stay absolute.
    assert_eq!(
        entry_point_location_prefix("arachnea://api/proxy"),
        "arachnea://api/proxy"
    );
}

#[test]
fn remove_header_keeps_redirect_opts_for_non_302() {
    let opts = ProxyHttpOpts {
        headers: vec![(
            "Arachnea-Proxy-Countries".to_string(),
            r#"["FR"]"#.to_string(),
        )],
        cookies: HashMap::new(),
        proxy: serde_json::Value::Null,
        insecure_tls: false,
        fallback_direct: Some(false),
        follow_redirects: None,
        proxy_rejection_statuses: Vec::new(),
        proxy_retry_replayable: false,
        proxy_response_classifier: None,
    };
    let encoded = encode_proxy_opts(&opts);
    let redirect_actions = vec![ProxyHttpRedirectActionConfig::RemoveHeader(
        ProxyHttpRemoveHeaderConfig {
            trigger: "onHttp302".to_string(),
            headers: vec!["Arachnea-Proxy-Countries".to_string()],
        },
    )];

    let next_encoded = redirect_opts_encoded(301, &encoded, Some(&opts), &redirect_actions);

    assert_eq!(next_encoded, encoded);
}
