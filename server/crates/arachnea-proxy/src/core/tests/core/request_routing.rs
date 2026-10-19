use super::*;

/// Verifies that configured parameter handlers contribute extraction definitions.
#[test]
fn configured_parameter_handlers_contribute_definitions() {
    let core = ArachneaProxyCore::new(ProxyConfig {
        parameter_handlers: vec![ParameterHandlerConfig {
            kind: ParameterHandlerKind::SmartDns,
            parameter_name: Some("dns_hint".to_string()),
            http_header: Some("Arachnea-Proxy-Dns-Hint".to_string()),
            forward_header: false,
            stop_on_match: true,
            routes: Vec::new(),
        }],
        ..ProxyConfig::default()
    })
    .unwrap();

    let definitions = core.parameter_definitions();
    assert!(definitions
        .iter()
        .any(|definition| definition.name == PROXY_PARAMETER_COUNTRIES));
    assert!(definitions
        .iter()
        .any(|definition| definition.name == "dns_hint"));
}

/// Verifies that country routing handlers select a request-local extra proxy.
#[test]
fn country_parameter_handler_selects_extra_proxy() {
    let handler = CountryRoutingProxyHandler::new()
        .with_proxy_for_country("us", ProxyNode::socks5("us", "127.0.0.1:9050"))
        .unwrap()
        .with_proxy_for_country("ca", ProxyNode::socks5("ca", "127.0.0.1:9051"))
        .unwrap();
    let dynamic = crate::core::DynamicCountryRoutingProxyHandler::new(
        crate::core::CoexistencePolicy::DynamicOnly,
    );
    for countries in [r#"["CA", "US", "US"]"#, r#"["us", " ca "]"#] {
        let mut context = ClientContext::new();
        context.insert(
            PROXY_PARAMETER_COUNTRIES,
            ClientParameter::String(countries.to_string()),
        );
        let decision = handler.proxy_from_parameters(&context).unwrap();
        assert!(decision.stop);
        assert_eq!(decision.extra_proxy.unwrap().name, "us");
        assert_eq!(
            dynamic
                .proxy_from_parameters(&context)
                .unwrap()
                .extra_proxy
                .unwrap()
                .name,
            r#"dynamic-countries:["CA","US"]"#
        );
        assert_eq!(
            crate::core::normalize_parameter_value(PROXY_PARAMETER_COUNTRIES, countries),
            r#"["CA","US"]"#
        );
    }
}

/// Verifies request-local proxies and explicit direct-fallback precedence.
#[tokio::test]
async fn parameter_handler_appends_request_local_proxy() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let accept = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut data = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            stream.read_exact(&mut byte).await.unwrap();
            data.push(byte[0]);
            if data.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        stream
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .unwrap();
        String::from_utf8(data).unwrap()
    });
    let core = ArachneaProxyCore::new(ProxyConfig {
        parameter_handlers: vec![ParameterHandlerConfig {
            kind: ParameterHandlerKind::CountryRouting,
            parameter_name: None,
            http_header: None,
            forward_header: false,
            stop_on_match: true,
            routes: vec![ParameterProxyRoute {
                value: "US".to_string(),
                proxy: ProxyNode::http("us-http", endpoint),
            }],
        }],
        ..ProxyConfig::default()
    })
    .unwrap();
    let mut context = ClientContext::new();
    context.insert(
        PROXY_PARAMETER_COUNTRIES,
        ClientParameter::String(r#"["CA", "US"]"#.to_string()),
    );

    let stream = core
        .connect_request(
            ConnectRequest::new(Destination::host_port("example.com", 443))
                .with_client_context(context),
        )
        .await;

    assert!(stream.is_ok());
    let request = accept.await.unwrap();
    assert!(request.starts_with("CONNECT example.com:443 "));

    use crate::core::{
        CoexistencePolicy, DynamicCountryRoutingProxyHandler, PrivacyPolicy,
        ProxyFallbackDiagnostics, ProxySearchContext, PROXY_PARAMETER_FALLBACK_DIRECT,
    };
    let direct_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let destination =
        Destination::host_port("127.0.0.1", direct_listener.local_addr().unwrap().port());
    for scoped in [false, true] {
        for (default, override_value, prevent, allow, expected) in [
            (false, None, false, true, false),
            (true, None, false, true, true),
            (false, Some(true), false, true, true),
            (true, Some(false), false, true, false),
            (true, Some(true), true, true, false),
            (true, Some(true), false, false, false),
        ] {
            let core = ArachneaProxyCore::new(ProxyConfig {
                fallback_direct: default,
                chains: vec![ProxyChain::fixed(
                    "remote",
                    vec![ProxyNode::http("unused", "127.0.0.1:1")],
                )],
                routing: RoutePolicy {
                    default_chain: Some("remote".into()),
                    allow_direct_fallback: allow,
                    rules: Vec::new(),
                },
                privacy: PrivacyPolicy {
                    prevent_direct_fallback: prevent,
                    ..PrivacyPolicy::default()
                },
                ..ProxyConfig::default()
            })
            .unwrap()
            .with_parameter_handler(DynamicCountryRoutingProxyHandler::new(
                CoexistencePolicy::DynamicOnly,
            ));
            let mut context = ClientContext::new();
            context.insert(
                PROXY_PARAMETER_COUNTRIES,
                ClientParameter::String(r#"["BE"]"#.into()),
            );
            if let Some(value) = override_value {
                context.insert(
                    PROXY_PARAMETER_FALLBACK_DIRECT,
                    ClientParameter::String(value.to_string()),
                );
            }
            let request = ConnectRequest::new(destination.clone()).with_client_context(context);
            let diagnostics = ProxyFallbackDiagnostics::default();
            let execute = async {
                if scoped {
                    ProxySearchContext::default()
                        .scope(core.select_for_request(request.clone()))
                        .await
                } else {
                    core.select_for_request(request.clone()).await
                }
            };
            let selected = diagnostics.scope(execute).await;
            assert_eq!(
                selected.is_ok(),
                expected,
                "default={default}, override={override_value:?}, scoped={scoped}"
            );
            assert_eq!(diagnostics.messages().len(), usize::from(expected));
            if let Ok(selected) = selected {
                assert!(selected.selection_token.is_none());
                let stream = selected.core.connect_request(request).await.unwrap();
                let accepted = tokio::time::timeout(
                    std::time::Duration::from_secs(1),
                    direct_listener.accept(),
                )
                .await
                .unwrap()
                .unwrap();
                drop((stream, accepted));
            }
        }
    }
}

/// Verifies that routing rules can match client context parameters.
#[test]
fn routing_rule_uses_client_context() {
    let direct = ProxyChain::direct();
    let premium = ProxyChain::fixed("premium", vec![ProxyNode::direct("direct")]);
    let config = ProxyConfig {
        chains: vec![direct, premium],
        routing: RoutePolicy {
            default_chain: Some("direct".to_string()),
            allow_direct_fallback: true,
            rules: vec![RouteRule {
                match_host_suffix: None,
                match_port: None,
                match_protocol: None,
                match_client_parameter: Some(PROXY_PARAMETER_COUNTRIES.to_string()),
                match_client_value: Some(ClientParameter::String(r#"["US"]"#.to_string())),
                chain: "premium".to_string(),
            }],
        },
        ..ProxyConfig::default()
    }
    .resolve()
    .unwrap();
    let mut context = ClientContext::new();
    context.insert(
        PROXY_PARAMETER_COUNTRIES,
        ClientParameter::String(r#"["US"]"#.to_string()),
    );
    let request = ConnectRequest::new(Destination::host_port("example.com", 443))
        .with_client_context(context);
    let decision = config
        .as_config()
        .routing
        .decide(&config, &request)
        .unwrap();
    assert_eq!(decision.selected_chain, "premium");
}

/// Verifies that a Smart DNS route hint selects a precomputed proxy chain.
#[test]
fn smart_dns_route_hint_selects_chain() {
    let config = ProxyConfig {
        chains: vec![
            ProxyChain::direct(),
            ProxyChain::fixed("smart", vec![ProxyNode::direct("direct")]),
        ],
        routing: RoutePolicy {
            default_chain: Some("direct".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    }
    .resolve()
    .unwrap();
    let mut context = ClientContext::new();
    SmartDnsRouteHint::new(Some("smart".to_string()), None).insert_into_context(&mut context);
    let request = ConnectRequest::new(Destination::host_port("example.com", 443))
        .with_client_context(context);

    let decision = config
        .as_config()
        .routing
        .decide(&config, &request)
        .unwrap();

    assert_eq!(decision.selected_chain, "smart");
}

/// Verifies that a Smart DNS egress-pool hint creates a pooled route.
#[test]
fn smart_dns_route_hint_selects_egress_pool() {
    let config = ProxyConfig {
        upstreams: vec![ProxyNode::http("http-a", "127.0.0.1:8080")],
        egress_pools: vec![EgressPool::new(
            "smart-pool",
            vec![ProxyNode::http("http-a", "127.0.0.1:8080")],
            None,
        )],
        chains: vec![ProxyChain::direct()],
        routing: RoutePolicy {
            default_chain: Some("direct".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    }
    .resolve()
    .unwrap();
    let mut context = ClientContext::new();
    SmartDnsRouteHint::new(None, Some("smart-pool".to_string())).insert_into_context(&mut context);
    let request = ConnectRequest::new(Destination::host_port("example.com", 443))
        .with_client_context(context);

    let decision = config
        .as_config()
        .routing
        .decide(&config, &request)
        .unwrap();

    assert_eq!(decision.selected_chain, "egress_pool:smart-pool");
    assert_eq!(decision.chain.nodes[0].kind, TransportKind::ProxyPool);
    assert_eq!(decision.chain.nodes[0].name, "smart-pool");
}
