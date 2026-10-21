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
