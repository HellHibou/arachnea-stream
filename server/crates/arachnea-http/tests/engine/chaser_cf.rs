use super::*;

fn waf_session(expires_at: u64) -> WafSession {
    let mut headers = HashMap::new();
    headers.insert("user-agent".to_string(), "test-browser".to_string());
    WafSession::new(
        vec![ChaserCookie {
            name: "cf_clearance".to_string(),
            value: "ok".to_string(),
            domain: Some("example.com".to_string()),
            path: Some("/".to_string()),
            expires: Some(expires_at as f64),
            http_only: Some(true),
            secure: Some(true),
            same_site: Some("None".to_string()),
        }],
        headers,
    )
}

#[tokio::test]
async fn persistent_session_cache_returns_headers_for_valid_clearance() {
    let cache = ChaserSessionCache {
        repository: Arc::new(TypedCloudflareSessionRepository::new(
            memory_session_store().expect("session store"),
        )),
        refresh_margin: Duration::from_secs(300),
    };
    cache
        .store_session(
            "https://example.com/search",
            &waf_session(unix_timestamp() + 3_600),
        )
        .await
        .expect("session stores");

    let headers = cache
        .headers_for_url("https://example.com/other")
        .await
        .expect("cache lookup succeeds")
        .expect("session exists");
    assert!(headers.get_all(SET_COOKIE).iter().any(|value| value
        .to_str()
        .is_ok_and(|value| value.starts_with("cf_clearance=ok"))));
    assert_eq!(
        headers
            .get(SOLVER_USER_AGENT_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some("test-browser")
    );
}

#[tokio::test]
async fn persistent_session_cache_ignores_expiring_clearance() {
    let cache = ChaserSessionCache {
        repository: Arc::new(TypedCloudflareSessionRepository::new(
            memory_session_store().expect("session store"),
        )),
        refresh_margin: Duration::from_secs(300),
    };
    cache
        .store_session(
            "https://example.com/search",
            &waf_session(unix_timestamp() + 30),
        )
        .await
        .expect("session stores");

    assert!(cache
        .headers_for_url("https://example.com/other")
        .await
        .expect("cache lookup succeeds")
        .is_none());
}
