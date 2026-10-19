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

/// Verifies that a parameter handler can append a proxy for one request only.
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
