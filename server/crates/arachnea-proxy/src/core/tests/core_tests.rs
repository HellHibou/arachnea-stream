use crate::core::{
    ApplicationProtocol, ArachneaProxyCore, ClientContext, ClientParameter, ConnectRequest,
    CountryRoutingProxyHandler, Destination, EgressPool, HttpRequestTargetForm,
    ParameterHandlerConfig, ParameterHandlerKind, ParameterProxyRoute, ProxyChain, ProxyConfig,
    ProxyError, ProxyNameResolutionMode, ProxyNode, ProxyParameterHandler, ProxyPoolMemberStatus,
    ProxyProfile, RoutePolicy, RouteRule, SecurityPolicy, SmartDnsRouteHint, TimeoutConfig,
    TransportKind, UsageProfile, PROXY_PARAMETER_COUNTRY,
};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Verifies that the default proxy configuration resolves to the direct chain.
#[test]
fn default_config_resolves_to_direct() {
    let config = ProxyConfig::default().resolve().unwrap();
    assert!(config.chain("direct").is_some());
}

/// Verifies that new profile names serialize while legacy names still parse.
#[test]
fn proxy_profile_accepts_new_and_legacy_names() {
    let current = serde_json::to_string(&ProxyProfile::Direct).unwrap();
    let legacy: ProxyProfile = serde_json::from_str("\"direct\"").unwrap();
    let current_single: ProxyProfile = serde_json::from_str("\"single_forwarder\"").unwrap();
    let legacy_single: ProxyProfile = serde_json::from_str("\"single_proxy\"").unwrap();

    assert_eq!(current, "\"system_relay\"");
    assert_eq!(legacy, ProxyProfile::Direct);
    assert_eq!(current_single, ProxyProfile::SingleProxy);
    assert_eq!(legacy_single, ProxyProfile::SingleProxy);
}

/// Verifies that destinations preserve hostnames instead of resolving early.
#[test]
fn destination_preserves_hostname() {
    let destination =
        Destination::from_authority("example.com:443", crate::core::ApplicationProtocol::Https)
            .unwrap();
    assert_eq!(destination.authority(), "example.com:443");
}

/// Verifies that an undeclared default chain is rejected.
#[test]
fn invalid_default_chain_is_rejected() {
    let config = ProxyConfig {
        routing: RoutePolicy {
            default_chain: Some("missing".to_string()),
            allow_direct_fallback: false,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    };
    assert!(matches!(config.resolve(), Err(ProxyError::Config(_))));
}

/// Verifies that route rules cannot reference undeclared chains.
#[test]
fn route_rule_with_missing_chain_is_rejected() {
    let config = ProxyConfig {
        routing: RoutePolicy {
            default_chain: Some("direct".to_string()),
            allow_direct_fallback: true,
            rules: vec![RouteRule::to_chain("missing")],
        },
        ..ProxyConfig::default()
    };
    assert!(matches!(config.resolve(), Err(ProxyError::Config(_))));
}

/// Verifies that duplicate chain names are rejected.
#[test]
fn duplicate_chain_names_are_rejected() {
    let config = ProxyConfig {
        chains: vec![ProxyChain::direct(), ProxyChain::direct()],
        ..ProxyConfig::default()
    };
    assert!(matches!(config.resolve(), Err(ProxyError::Config(_))));
}

/// Verifies that duplicate node names inside a chain are rejected.
#[test]
fn duplicate_node_names_inside_chain_are_rejected() {
    let config = ProxyConfig {
        chains: vec![ProxyChain::fixed(
            "proxy",
            vec![
                ProxyNode::socks5("hop", "127.0.0.1:9050"),
                ProxyNode::http("hop", "127.0.0.1:8080"),
            ],
        )],
        routing: RoutePolicy {
            default_chain: Some("proxy".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    };
    assert!(matches!(config.resolve(), Err(ProxyError::Config(_))));
}

/// Verifies that `direct` cannot be mixed with proxy hops in one chain.
#[test]
fn mixed_direct_and_proxy_nodes_are_rejected() {
    let config = ProxyConfig {
        chains: vec![ProxyChain::fixed(
            "mixed",
            vec![
                ProxyNode::direct("direct"),
                ProxyNode::socks5("proxy", "127.0.0.1:9050"),
            ],
        )],
        routing: RoutePolicy {
            default_chain: Some("mixed".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    };
    assert!(matches!(config.resolve(), Err(ProxyError::Config(_))));
}

/// Verifies that malformed proxy endpoints are rejected.
#[test]
fn invalid_proxy_endpoint_is_rejected() {
    let config = ProxyConfig {
        chains: vec![ProxyChain::fixed(
            "proxy",
            vec![ProxyNode::socks5("proxy", "127.0.0.1")],
        )],
        routing: RoutePolicy {
            default_chain: Some("proxy".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    };
    assert!(matches!(config.resolve(), Err(ProxyError::Config(_))));
}

/// Verifies that global TLS verification settings control HTTPS upstream checks.
#[test]
fn disabling_global_upstream_tls_verification_allows_insecure_https_node() {
    let mut node = ProxyNode::https("https", "proxy.example:443");
    node.verify_tls = false;
    let config = ProxyConfig {
        chains: vec![ProxyChain::fixed("https", vec![node])],
        routing: RoutePolicy {
            default_chain: Some("https".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        security: SecurityPolicy {
            verify_upstream_tls: false,
            ..SecurityPolicy::default()
        },
        ..ProxyConfig::default()
    };
    assert!(config.resolve().is_ok());
}

/// Verifies that the privacy profile disables direct fallback.
#[test]
fn privacy_profile_disables_direct_fallback() {
    let config = ProxyConfig::builder()
        .profile(ProxyProfile::Privacy)
        .build()
        .unwrap();
    assert!(!config.as_config().routing.allow_direct_fallback);
}

/// Verifies that the builder creates a chain for a single upstream.
#[test]
fn builder_adds_single_upstream_chain() {
    let config = ArachneaProxyCore::builder()
        .upstream(ProxyNode::socks5("tor", "127.0.0.1:9050"))
        .build()
        .unwrap();
    assert!(config.chain("tor").is_some());
}

/// Verifies that a single-forwarder usage profile builds one default proxy chain.
#[test]
fn usage_profile_single_forwarder_builds_default_chain() {
    let config = UsageProfile::SingleForwarder(ProxyNode::socks5("tor", "127.0.0.1:9050")).config();
    assert_eq!(config.routing.default_chain.as_deref(), Some("tor"));
    assert!(!config.routing.allow_direct_fallback);
    assert_eq!(config.chains[0].nodes[0].kind, TransportKind::Socks5);
}

/// Verifies that a multi-forwarder usage profile keeps hop order.
#[test]
fn usage_profile_multi_forwarder_keeps_hop_order() {
    let config = UsageProfile::MultiForwarder(vec![
        ProxyNode::http("first", "127.0.0.1:8080"),
        ProxyNode::socks5("second", "127.0.0.1:9050"),
    ])
    .config();
    let chain = config.chains.first().unwrap();
    assert_eq!(chain.name, "multi_forwarder");
    assert_eq!(chain.nodes[0].name, "first");
    assert_eq!(chain.nodes[1].name, "second");
}

/// Verifies that a privacy usage profile disables direct fallback.
#[test]
fn usage_profile_privacy_disables_direct_fallback() {
    let config = UsageProfile::Privacy(vec![ProxyNode::socks5("tor", "127.0.0.1:9050")]).config();
    assert_eq!(config.profile, ProxyProfile::Privacy);
    assert!(!config.routing.allow_direct_fallback);
}

/// Verifies that proxy URL helpers create the expected proxy node.
#[test]
fn proxy_node_from_url_uses_scheme_defaults() {
    let node = ProxyNode::from_url("local-socks", "socks5://127.0.0.1").unwrap();
    let socks4 = ProxyNode::from_url("legacy-socks", "socks4://127.0.0.1").unwrap();
    let socks4a = ProxyNode::from_url("legacy-socks4a", "socks4a://127.0.0.1").unwrap();
    assert_eq!(node.kind, TransportKind::Socks5);
    assert_eq!(node.endpoint.as_deref(), Some("127.0.0.1:1080"));
    assert_eq!(
        node.effective_name_resolution(),
        ProxyNameResolutionMode::ProxyThenLocalFallback
    );
    assert_eq!(socks4.kind, TransportKind::Socks4);
    assert_eq!(socks4.endpoint.as_deref(), Some("127.0.0.1:1080"));
    assert_eq!(
        socks4.effective_name_resolution(),
        ProxyNameResolutionMode::Local
    );
    assert_eq!(socks4a.kind, TransportKind::Socks4);
    assert_eq!(
        socks4a.effective_name_resolution(),
        ProxyNameResolutionMode::ProxyOnly
    );
}

/// Verifies that SOCKS5 URLs can receive an explicit DNS resolution mode.
#[test]
fn proxy_node_from_socks5_url_accepts_dns_resolution_mode() {
    let node = ProxyNode::from_socks5_url(
        "local-socks",
        "socks5://127.0.0.1",
        ProxyNameResolutionMode::ProxyOnly,
    )
    .unwrap();
    let http = ProxyNode::from_socks5_url(
        "not-socks",
        "http://127.0.0.1",
        ProxyNameResolutionMode::ProxyOnly,
    );

    assert_eq!(node.kind, TransportKind::Socks5);
    assert_eq!(
        node.effective_name_resolution(),
        ProxyNameResolutionMode::ProxyOnly
    );
    assert!(matches!(http, Err(ProxyError::Config(_))));
}

/// Verifies that explicit proxy DNS modes override legacy hostname forwarding.
#[test]
fn proxy_node_dns_resolution_mode_overrides_send_hostname() {
    let default = ProxyNode::socks5("default-dns", "127.0.0.1:9050");
    let local = ProxyNode::socks5("local-dns", "127.0.0.1:9050")
        .with_dns_resolution(ProxyNameResolutionMode::Local);
    let fallback = ProxyNode::socks5("fallback-dns", "127.0.0.1:9050")
        .with_dns_resolution(ProxyNameResolutionMode::ProxyThenLocalFallback);
    let legacy = ProxyNode::socks5("legacy", "127.0.0.1:9050").with_hostname_forwarding(false);

    assert_eq!(
        default.effective_name_resolution(),
        ProxyNameResolutionMode::ProxyThenLocalFallback
    );
    assert_eq!(
        local.effective_name_resolution(),
        ProxyNameResolutionMode::Local
    );
    assert_eq!(
        fallback.effective_name_resolution(),
        ProxyNameResolutionMode::ProxyThenLocalFallback
    );
    assert_eq!(
        legacy.effective_name_resolution(),
        ProxyNameResolutionMode::Local
    );
}

/// Verifies that chain nodes can reference declared proxy pools.
#[test]
fn proxy_pool_chain_marker_resolves() {
    let config = ProxyConfig {
        upstreams: vec![ProxyNode::http("http-a", "127.0.0.1:8080")],
        egress_pools: vec![EgressPool::new(
            "pool-a",
            vec![ProxyNode::http("http-a", "127.0.0.1:8080")],
            None,
        )],
        chains: vec![ProxyChain::pool("pooled", "pool-a")],
        routing: RoutePolicy {
            default_chain: Some("pooled".to_string()),
            allow_direct_fallback: false,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    }
    .resolve()
    .unwrap();

    let chain = config.chain("pooled").unwrap();
    assert_eq!(chain.nodes[0].kind, TransportKind::ProxyPool);
    assert_eq!(chain.nodes[0].name, "pool-a");
}

/// Verifies that pools cannot contain direct nodes.
#[test]
fn proxy_pool_rejects_direct_member() {
    let config = ProxyConfig {
        egress_pools: vec![EgressPool::new(
            "pool-a",
            vec![ProxyNode::direct("direct")],
            None,
        )],
        chains: vec![ProxyChain::pool("pooled", "pool-a")],
        routing: RoutePolicy {
            default_chain: Some("pooled".to_string()),
            allow_direct_fallback: false,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    };

    assert!(matches!(config.resolve(), Err(ProxyError::Config(_))));
}

/// Verifies that malformed proxy URL ports are rejected early.
#[test]
fn proxy_node_from_url_rejects_invalid_ports() {
    let result = ProxyNode::from_url("bad", "socks5://127.0.0.1:not-a-port");
    assert!(matches!(result, Err(ProxyError::Config(_))));
}

/// Verifies that an extra proxy hop can be appended to a generated chain.
#[test]
fn config_appends_extra_proxy_to_generated_chain() {
    let config = ProxyNode::http("first", "127.0.0.1:8080")
        .config()
        .append_proxy(ProxyNode::socks5("second", "127.0.0.1:9050"))
        .unwrap()
        .resolve()
        .unwrap();
    let chain = config.chain("first").unwrap();
    assert_eq!(chain.nodes.len(), 2);
    assert_eq!(chain.nodes[1].name, "second");
}

/// Verifies that a core handle can append a proxy hop after construction.
#[test]
fn core_appends_proxy_to_default_chain() {
    let mut core =
        ArachneaProxyCore::new(ProxyNode::http("first", "127.0.0.1:8080").config()).unwrap();

    core.append_proxy(ProxyNode::socks5("second", "127.0.0.1:9050"))
        .unwrap();

    let chain = core.config().chain("first").unwrap();
    assert_eq!(chain.nodes.len(), 2);
    assert_eq!(chain.nodes[0].name, "first");
    assert_eq!(chain.nodes[1].name, "second");
}

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
        .any(|definition| definition.name == PROXY_PARAMETER_COUNTRY));
    assert!(definitions
        .iter()
        .any(|definition| definition.name == "dns_hint"));
}

/// Verifies that country routing handlers select a request-local extra proxy.
#[test]
fn country_parameter_handler_selects_extra_proxy() {
    let handler = CountryRoutingProxyHandler::new()
        .with_proxy_for_country("us", ProxyNode::socks5("us", "127.0.0.1:9050"))
        .unwrap();
    let mut context = ClientContext::new();
    context.insert(
        PROXY_PARAMETER_COUNTRY,
        ClientParameter::String("US".to_string()),
    );

    let decision = handler.proxy_from_parameters(&context).unwrap();

    assert!(decision.stop);
    assert_eq!(decision.extra_proxy.unwrap().name, "us");
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
        PROXY_PARAMETER_COUNTRY,
        ClientParameter::String("US".to_string()),
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

/// Verifies that usage profile timeouts are applied to generated configs.
#[test]
fn usage_profile_config_with_timeouts_sets_core_timeouts() {
    let timeouts = TimeoutConfig {
        connect_ms: 1_500,
        handshake_ms: 2_500,
        idle_ms: 3_500,
    };

    let config = UsageProfile::SingleForwarder(ProxyNode::socks5("proxy", "127.0.0.1:9050"))
        .config_with_timeouts(timeouts.clone());

    assert_eq!(config.timeouts, timeouts);
}

/// Verifies that fixed chains retain their node order.
#[test]
fn fixed_chain_keeps_nodes() {
    let chain = ProxyChain::fixed(
        "chain",
        vec![
            ProxyNode::socks5("first", "127.0.0.1:9050"),
            ProxyNode::http("second", "127.0.0.1:8080"),
        ],
    );
    assert_eq!(chain.nodes.len(), 2);
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
                match_client_parameter: Some(PROXY_PARAMETER_COUNTRY.to_string()),
                match_client_value: Some(ClientParameter::String("US".to_string())),
                chain: "premium".to_string(),
            }],
        },
        ..ProxyConfig::default()
    }
    .resolve()
    .unwrap();
    let mut context = ClientContext::new();
    context.insert(
        PROXY_PARAMETER_COUNTRY,
        ClientParameter::String("US".to_string()),
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

/// Verifies that proxy profiles select their matching DNS profile defaults.
#[cfg(feature = "arachnea-dns")]
#[test]
fn arachnea_dns_profile_matches_proxy_profile() {
    let privacy_dns =
        ArachneaProxyCore::arachnea_dns_config_for_proxy_profile(&ProxyProfile::Privacy);
    let censorship_dns = ArachneaProxyCore::arachnea_dns_config_for_proxy_profile(
        &ProxyProfile::CensorshipResistance,
    );
    let advanced_dns =
        ArachneaProxyCore::arachnea_dns_config_for_proxy_profile(&ProxyProfile::Advanced);

    assert!(matches!(
        privacy_dns.profile,
        arachnea_dns::core::UsageProfile::Privacy(_)
    ));
    assert!(matches!(
        censorship_dns.profile,
        arachnea_dns::core::UsageProfile::Resilience(_)
    ));
    assert!(matches!(
        advanced_dns.profile,
        arachnea_dns::core::UsageProfile::Advanced
    ));
    assert!(privacy_dns.resolve_effective_config().is_ok());
    assert!(censorship_dns.resolve_effective_config().is_ok());
    assert!(advanced_dns.resolve_effective_config().is_ok());
}

/// Verifies that the default constructor installs a DNS core when the feature is active.
#[cfg(feature = "arachnea-dns")]
#[test]
fn new_installs_default_arachnea_dns_core() {
    let core = ArachneaProxyCore::new(ProxyConfig::default()).unwrap();

    assert!(format!("{core:?}").contains("arachnea_dns_enabled: true"));
}

/// Verifies absolute-form forwarding for HTTP upstream proxy requests.
#[tokio::test]
async fn http_request_uses_absolute_form_for_http_upstream() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let accept = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = [0u8; 3];
        stream.read_exact(&mut bytes).await.unwrap();
        bytes
    });
    let core = ArachneaProxyCore::new(ProxyConfig {
        chains: vec![ProxyChain::fixed(
            "proxy",
            vec![ProxyNode::http("proxy", endpoint)],
        )],
        routing: RoutePolicy {
            default_chain: Some("proxy".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    })
    .unwrap();
    let mut outbound = core
        .connect_http_request(ConnectRequest::new(
            Destination::host_port("example.com", 80).with_protocol(ApplicationProtocol::Http),
        ))
        .await
        .unwrap();

    assert_eq!(outbound.target_form, HttpRequestTargetForm::AbsoluteForm);
    outbound.stream.write_all(b"GET").await.unwrap();
    assert_eq!(accept.await.unwrap(), *b"GET");
}

/// Verifies that HTTP upstream credentials are not sent when no password is defined.
#[tokio::test]
async fn http_upstream_without_password_does_not_send_proxy_authorization() {
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
    let mut node = ProxyNode::http("proxy", endpoint);
    node.username = Some("tester".to_string());
    let core = ArachneaProxyCore::new(ProxyConfig {
        chains: vec![ProxyChain::fixed("proxy", vec![node])],
        routing: RoutePolicy {
            default_chain: Some("proxy".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    })
    .unwrap();

    let stream = core
        .connect(Destination::host_port("example.com", 443))
        .await;
    assert!(stream.is_ok());
    let request = accept.await.unwrap();
    assert!(!request.to_ascii_lowercase().contains("proxy-authorization"));
}

/// Verifies that proxy pools test members and cache the first working candidate.
#[tokio::test]
async fn proxy_pool_selects_and_caches_first_working_member() {
    let bad_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bad_endpoint = bad_listener.local_addr().unwrap().to_string();
    let bad_accept = tokio::spawn(async move {
        let (mut stream, _) = bad_listener.accept().await.unwrap();
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
            .write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n")
            .await
            .unwrap();
    });

    let good_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let good_endpoint = good_listener.local_addr().unwrap().to_string();
    let good_accept = tokio::spawn(async move {
        let mut requests = Vec::new();
        for _ in 0..3 {
            let (mut stream, _) = good_listener.accept().await.unwrap();
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
            requests.push(String::from_utf8(data).unwrap());
        }
        requests
    });

    let core = ArachneaProxyCore::new(ProxyConfig {
        upstreams: vec![
            ProxyNode::http("bad", bad_endpoint.clone()),
            ProxyNode::http("good", good_endpoint.clone()),
        ],
        egress_pools: vec![EgressPool::new(
            "pool",
            vec![
                ProxyNode::http("bad", bad_endpoint.clone()),
                ProxyNode::http("good", good_endpoint.clone()),
            ],
            Some(1),
        )],
        chains: vec![ProxyChain::pool("pooled", "pool")],
        routing: RoutePolicy {
            default_chain: Some("pooled".to_string()),
            allow_direct_fallback: false,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    })
    .unwrap();

    let initial = core.proxy_pool_statuses("pool").unwrap();
    assert!(initial
        .iter()
        .all(|state| state.status == ProxyPoolMemberStatus::Untested));

    let first = core
        .connect(Destination::host_port("example.com", 443))
        .await;
    assert!(first.is_ok());
    drop(first);

    let statuses = core.proxy_pool_statuses("pool").unwrap();
    assert_eq!(statuses[0].status, ProxyPoolMemberStatus::Ko);
    assert!(!statuses[0].selected);
    assert_eq!(statuses[1].status, ProxyPoolMemberStatus::Ok);
    assert!(statuses[1].selected);

    let second = core
        .connect(Destination::host_port("example.com", 443))
        .await;
    assert!(second.is_ok());
    bad_accept.await.unwrap();
    let good_requests = good_accept.await.unwrap();
    assert_eq!(good_requests.len(), 3);
    assert!(good_requests
        .iter()
        .all(|request| request.starts_with("CONNECT example.com:443 ")));
}

/// Verifies that a cached pool member is marked failed and replaced on setup failure.
#[tokio::test]
async fn proxy_pool_fails_over_when_cached_member_stops_accepting_connections() {
    let first_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let first_endpoint = first_listener.local_addr().unwrap().to_string();
    let first_accept = tokio::spawn(async move {
        let mut requests = Vec::new();
        for _ in 0..2 {
            let (mut stream, _) = first_listener.accept().await.unwrap();
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
            requests.push(String::from_utf8(data).unwrap());
        }
        requests
    });

    let second_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second_endpoint = second_listener.local_addr().unwrap().to_string();
    let second_accept = tokio::spawn(async move {
        let mut requests = Vec::new();
        for _ in 0..2 {
            let (mut stream, _) = second_listener.accept().await.unwrap();
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
            requests.push(String::from_utf8(data).unwrap());
        }
        requests
    });

    let core = ArachneaProxyCore::new(ProxyConfig {
        upstreams: vec![
            ProxyNode::http("first", first_endpoint.clone()),
            ProxyNode::http("second", second_endpoint.clone()),
        ],
        egress_pools: vec![EgressPool::new(
            "pool",
            vec![
                ProxyNode::http("first", first_endpoint.clone()),
                ProxyNode::http("second", second_endpoint.clone()),
            ],
            None,
        )],
        chains: vec![ProxyChain::pool("pooled", "pool")],
        routing: RoutePolicy {
            default_chain: Some("pooled".to_string()),
            allow_direct_fallback: false,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    })
    .unwrap();

    let first = core
        .connect(Destination::host_port("example.com", 443))
        .await;
    assert!(first.is_ok());
    drop(first);
    assert!(core.proxy_pool_statuses("pool").unwrap()[0].selected);
    first_accept.await.unwrap();

    let second = core
        .connect(Destination::host_port("example.com", 443))
        .await;
    assert!(second.is_ok());
    drop(second);
    let statuses = core.proxy_pool_statuses("pool").unwrap();
    let second_requests = second_accept.await.unwrap();

    assert_eq!(statuses[0].status, ProxyPoolMemberStatus::Ko);
    assert!(!statuses[0].selected);
    assert_eq!(statuses[1].status, ProxyPoolMemberStatus::Ok);
    assert!(statuses[1].selected);
    assert_eq!(second_requests.len(), 2);
}

/// Verifies that a final HTTP proxy pool can relay plain HTTP without CONNECT.
#[tokio::test]
async fn proxy_pool_final_http_proxy_accepts_absolute_form_http() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let accept = tokio::spawn(async move {
        let (mut probe, _) = listener.accept().await.unwrap();
        let mut ignored = [0u8; 1];
        assert_eq!(probe.read(&mut ignored).await.unwrap(), 0);

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
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .await
            .unwrap();
        String::from_utf8(data).unwrap()
    });

    let core = ArachneaProxyCore::new(ProxyConfig {
        upstreams: vec![ProxyNode::http("http-proxy", endpoint.clone())],
        egress_pools: vec![EgressPool::new(
            "pool",
            vec![ProxyNode::http("http-proxy", endpoint.clone())],
            None,
        )],
        chains: vec![ProxyChain::pool("pooled", "pool")],
        routing: RoutePolicy {
            default_chain: Some("pooled".to_string()),
            allow_direct_fallback: false,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    })
    .unwrap();
    let mut outbound = core
        .connect_http_request(ConnectRequest::new(
            Destination::host_port("example.com", 80).with_protocol(ApplicationProtocol::Http),
        ))
        .await
        .unwrap();

    assert_eq!(outbound.target_form, HttpRequestTargetForm::AbsoluteForm);
    outbound
        .stream
        .write_all(b"GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\n\r\n")
        .await
        .unwrap();
    let mut body = Vec::new();
    outbound.stream.read_to_end(&mut body).await.unwrap();
    let request = accept.await.unwrap();

    assert!(request.starts_with("GET http://example.com/ "));
    assert_eq!(
        core.proxy_pool_statuses("pool").unwrap()[0].status,
        ProxyPoolMemberStatus::Ok
    );
}

/// Verifies that SOCKS5 pool probing uses the normal DNS fallback behavior.
#[tokio::test]
async fn proxy_pool_socks5_probe_uses_dns_fallback() {
    let target = spawn_echo_target_accepting(2).await;
    let proxy_addr = spawn_socks5_proxy_rejecting_domain_requests(3).await;
    let node = ProxyNode::socks5("socks", proxy_addr.to_string())
        .with_dns_resolution(ProxyNameResolutionMode::ProxyThenLocalFallback);
    let core = ArachneaProxyCore::new(ProxyConfig {
        upstreams: vec![node.clone()],
        egress_pools: vec![EgressPool::new("pool", vec![node], None)],
        chains: vec![ProxyChain::pool("pooled", "pool")],
        routing: RoutePolicy {
            default_chain: Some("pooled".to_string()),
            allow_direct_fallback: false,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    })
    .unwrap();

    let mut stream = core
        .connect(Destination::host_port("127.0.0.1", target.port()))
        .await
        .unwrap();
    stream.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    stream.read_exact(&mut echoed).await.unwrap();
    let statuses = core.proxy_pool_statuses("pool").unwrap();

    assert_eq!(&echoed, b"ping");
    assert_eq!(statuses[0].status, ProxyPoolMemberStatus::Ok);
    assert!(statuses[0].selected);
}

/// Verifies that SOCKS5 upstreams use no-auth when no password is defined.
#[tokio::test]
async fn socks_upstream_without_password_uses_no_auth_method() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let accept = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut methods = [0u8; 3];
        stream.read_exact(&mut methods).await.unwrap();
        stream.write_all(&[0x05, 0x00]).await.unwrap();

        let mut request_head = [0u8; 4];
        stream.read_exact(&mut request_head).await.unwrap();
        assert_eq!(request_head[1], 0x01);
        match request_head[3] {
            0x01 => {
                let mut rest = [0u8; 6];
                stream.read_exact(&mut rest).await.unwrap();
            }
            0x03 => {
                let mut len = [0u8; 1];
                stream.read_exact(&mut len).await.unwrap();
                let mut rest = vec![0u8; len[0] as usize + 2];
                stream.read_exact(&mut rest).await.unwrap();
            }
            0x04 => {
                let mut rest = [0u8; 18];
                stream.read_exact(&mut rest).await.unwrap();
            }
            other => panic!("unexpected SOCKS5 address type {other}"),
        }
        stream
            .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0, 0])
            .await
            .unwrap();
        methods
    });
    let mut node = ProxyNode::socks5("proxy", endpoint);
    node.username = Some("tester".to_string());
    let core = ArachneaProxyCore::new(ProxyConfig {
        chains: vec![ProxyChain::fixed("proxy", vec![node])],
        routing: RoutePolicy {
            default_chain: Some("proxy".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    })
    .unwrap();

    let stream = core
        .connect(Destination::host_port("example.com", 443))
        .await;
    assert!(stream.is_ok());
    assert_eq!(accept.await.unwrap(), [0x05, 0x01, 0x00]);
}

/// Verifies that SOCKS5 targets are locally resolved when hostname forwarding is disabled.
#[tokio::test]
async fn socks_upstream_without_hostname_forwarding_sends_ip_target() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let accept = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut methods = [0u8; 3];
        stream.read_exact(&mut methods).await.unwrap();
        stream.write_all(&[0x05, 0x00]).await.unwrap();

        let mut request_head = [0u8; 4];
        stream.read_exact(&mut request_head).await.unwrap();
        match request_head[3] {
            0x01 => {
                let mut rest = [0u8; 6];
                stream.read_exact(&mut rest).await.unwrap();
            }
            0x04 => {
                let mut rest = [0u8; 18];
                stream.read_exact(&mut rest).await.unwrap();
            }
            other => panic!("expected an IP target, got SOCKS5 address type {other}"),
        }
        stream
            .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0, 0])
            .await
            .unwrap();
        request_head[3]
    });
    let node = ProxyNode::socks5("proxy", endpoint).with_hostname_forwarding(false);
    let core = ArachneaProxyCore::new(ProxyConfig {
        chains: vec![ProxyChain::fixed("proxy", vec![node])],
        routing: RoutePolicy {
            default_chain: Some("proxy".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        },
        ..ProxyConfig::default()
    })
    .unwrap();

    let stream = core.connect(Destination::host_port("localhost", 443)).await;

    assert!(stream.is_ok());
    assert!(matches!(accept.await.unwrap(), 0x01 | 0x04));
}

/// Starts a TCP echo target that accepts a fixed number of connections.
///
/// # Parameters
///
/// - `connections`: Number of client connections to accept.
///
/// # Returns
///
/// Socket address of the echo target.
async fn spawn_echo_target_accepting(connections: usize) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for _ in 0..connections {
            let (mut stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                loop {
                    let read = stream.read(&mut buf).await.unwrap();
                    if read == 0 {
                        break;
                    }
                    stream.write_all(&buf[..read]).await.unwrap();
                }
            });
        }
    });
    addr
}

/// Starts a SOCKS5 proxy that rejects domain targets but accepts IP targets.
///
/// # Parameters
///
/// - `connections`: Number of SOCKS5 client connections to accept.
///
/// # Returns
///
/// Socket address of the SOCKS5 proxy.
async fn spawn_socks5_proxy_rejecting_domain_requests(connections: usize) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for _ in 0..connections {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                handle_socks5_proxy_rejecting_domain_request(stream)
                    .await
                    .unwrap();
            });
        }
    });
    addr
}

/// Handles one SOCKS5 request by rejecting domains and tunneling IP targets.
///
/// # Parameters
///
/// - `client`: Client stream connected to the fake SOCKS5 proxy.
///
/// # Errors
///
/// Returns an error when socket I/O fails.
async fn handle_socks5_proxy_rejecting_domain_request(
    mut client: TcpStream,
) -> std::io::Result<()> {
    let mut greeting_head = [0u8; 2];
    client.read_exact(&mut greeting_head).await?;
    let mut methods = vec![0u8; greeting_head[1] as usize];
    client.read_exact(&mut methods).await?;
    client.write_all(&[0x05, 0x00]).await?;

    let mut head = [0u8; 4];
    client.read_exact(&mut head).await?;
    if head[3] == 0x03 {
        let mut len = [0u8; 1];
        client.read_exact(&mut len).await?;
        let mut host = vec![0u8; len[0] as usize];
        client.read_exact(&mut host).await?;
        let mut port = [0u8; 2];
        client.read_exact(&mut port).await?;
        client
            .write_all(&[0x05, 0x04, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await?;
        return Ok(());
    }

    let target = read_socks5_ip_target(&mut client, head[3]).await?;
    let mut upstream = TcpStream::connect(target).await?;
    client
        .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0, 0])
        .await?;
    tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
    Ok(())
}

/// Reads an IP target from a SOCKS5 CONNECT request.
///
/// # Parameters
///
/// - `client`: SOCKS5 stream positioned after the request header.
/// - `address_type`: SOCKS5 address type byte from the request header.
///
/// # Returns
///
/// Socket address requested by the SOCKS5 client.
///
/// # Errors
///
/// Returns an error when the target cannot be read or is not an IP address.
async fn read_socks5_ip_target(
    client: &mut TcpStream,
    address_type: u8,
) -> std::io::Result<SocketAddr> {
    let ip = match address_type {
        0x01 => {
            let mut octets = [0u8; 4];
            client.read_exact(&mut octets).await?;
            std::net::IpAddr::from(octets)
        }
        0x04 => {
            let mut octets = [0u8; 16];
            client.read_exact(&mut octets).await?;
            std::net::IpAddr::from(octets)
        }
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "domain target was not expected after fallback",
            ));
        }
    };
    let mut port = [0u8; 2];
    client.read_exact(&mut port).await?;
    Ok(SocketAddr::new(ip, u16::from_be_bytes(port)))
}

/// Verifies that the optional DNS core can resolve direct proxy destinations.
#[cfg(feature = "arachnea-dns")]
#[tokio::test]
async fn arachnea_dns_core_resolves_direct_destination() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let accept = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.unwrap();
    });
    let dns_core = arachnea_dns::core::ArachneaDnsCore::new(arachnea_dns::core::CoreConfig {
        local_records: vec![arachnea_dns::core::LocalRecord {
            name: "proxy-target.test".to_string(),
            records: vec![arachnea_dns::core::Record {
                name: "proxy-target.test".to_string(),
                ttl: 60,
                data: arachnea_dns::core::RecordData::A {
                    address: "127.0.0.1".parse().unwrap(),
                },
            }],
            wildcard: false,
            priority_over_upstream: true,
        }],
        ..arachnea_dns::core::UsageProfile::SystemRelay.config()
    })
    .unwrap();
    let core = ArachneaProxyCore::new_with_arachnea_dns(ProxyConfig::default(), dns_core).unwrap();

    let stream = core
        .connect(Destination::host_port("proxy-target.test", port))
        .await;

    assert!(stream.is_ok());
    accept.await.unwrap();
}
