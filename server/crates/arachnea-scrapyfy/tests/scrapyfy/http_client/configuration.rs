use super::*;

#[test]
fn resolves_runtime_proxy_countries_json_template() {
    let mut config = ScraperHttpConfig {
        proxy_countries: vec!["{proxy_countries}".to_string()],
        ..Default::default()
    };

    config
        .resolve_collection_params("test", "resolve_stream", &HashMap::new())
        .unwrap();

    assert_eq!(config.proxy_countries, vec!["{proxy_countries}"]);

    let params = HashMap::from([(
        "proxy_countries".to_string(),
        "[\"fr\", \"BE\", \"fr\", \"invalid\"]".to_string(),
    )]);

    config
        .resolve_collection_params("test", "resolve_stream", &params)
        .unwrap();

    assert_eq!(config.proxy_countries, vec!["BE", "FR"]);
}

#[test]
fn normalizes_static_proxy_countries() {
    let mut config = ScraperHttpConfig {
        proxy_countries: vec!["fr".to_string(), "BE".to_string(), "fr".to_string()],
        ..Default::default()
    };

    config
        .resolve_collection_params("test", "resolve_stream", &HashMap::new())
        .unwrap();

    assert_eq!(config.proxy_countries, vec!["BE", "FR"]);
    assert_eq!(config.proxy_countries_hint(), vec!["BE", "FR"]);
    let grouped =
        ScraperHttpConfig::default().proxy_domain_group(["m6.fr".into(), "6play.fr".into()]);
    assert!(!grouped.is_empty());
    assert_eq!(
        grouped
            .merge(&ScraperHttpConfig::default())
            .proxy_domain_group,
        grouped.proxy_domain_group
    );
    let child: ScraperHttpConfig = serde_json::from_value(
        serde_json::json!({"proxy_domain_group": ["6cloud.fr", "bedrock.tech"]}),
    )
    .unwrap();
    assert_eq!(
        grouped.merge(&child).proxy_domain_group,
        child.proxy_domain_group
    );
    assert_eq!(
        serde_json::to_value(&child).unwrap()["proxy_domain_group"],
        serde_json::json!(["6cloud.fr", "bedrock.tech"])
    );
    #[cfg(feature = "arachnea-proxy")]
    {
        let handle = SharedProxyConfigHandle::new();
        let independent = handle.with_independent_config();
        assert!(Arc::ptr_eq(&handle.runtime, &independent.runtime));
        assert!(Arc::ptr_eq(&handle.transports, &independent.transports));
        handle.set_proxy(HttpProxyConfig::Disabled);
        assert!(
            independent.snapshot().proxy.is_none(),
            "sharing transports must not share mutable routing overrides"
        );
        use arachnea_proxy::core::http::response_classifier::*;
        let parent = ScraperHttpConfig {
            proxy_response_classifier: Some(ProxyResponseClassifierConfig::Http403),
            proxy_retry_replayable: Some(true),
            ..config.clone()
        };
        assert_eq!(
            parent
                .merge(&ScraperHttpConfig::default())
                .proxy_response_classifier,
            parent.proxy_response_classifier
        );
        assert_eq!(
            parent
                .merge(&ScraperHttpConfig::default())
                .proxy_retry_replayable,
            Some(true)
        );
        assert_eq!(
            parent
                .merge(&ScraperHttpConfig {
                    proxy_retry_replayable: Some(false),
                    ..Default::default()
                })
                .proxy_retry_replayable,
            Some(false)
        );
        let child: ScraperHttpConfig = serde_json::from_value(
            serde_json::json!({ "proxy_response_classifier": { "type": "default" } }),
        )
        .unwrap();
        assert_eq!(
            parent.merge(&child).proxy_response_classifier,
            Some(ProxyResponseClassifierConfig::Default)
        );
        let policy = ProxyResponseClassifierConfig::Default.build().unwrap();
        let client =
            HttpClient::with_http_config(parent).with_proxy_response_classifier(policy.clone());
        assert!(Arc::ptr_eq(
            &client.configured(child).response_classifier.unwrap().0,
            &policy.0
        ));
        assert!(Arc::ptr_eq(
            &client
                .with_isolated_cookies()
                .response_classifier
                .unwrap()
                .0,
            &policy.0
        ));
    }
}
