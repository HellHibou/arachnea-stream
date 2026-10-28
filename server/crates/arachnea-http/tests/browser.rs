use super::*;
use std::time::Duration;

// --- BrowserSessionKey tests ---

#[test]
fn test_browser_session_key_equality() {
    let a = BrowserSessionKey {
        origin: "https://example.com".into(),
        profile: "chrome".into(),
        proxy_route: None,
    };
    let b = BrowserSessionKey {
        origin: "https://example.com".into(),
        profile: "chrome".into(),
        proxy_route: None,
    };
    let c = BrowserSessionKey {
        origin: "https://other.com".into(),
        profile: "chrome".into(),
        proxy_route: None,
    };
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn test_browser_session_key_hash() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(BrowserSessionKey {
        origin: "https://a.com".into(),
        profile: "firefox".into(),
        proxy_route: None,
    });
    set.insert(BrowserSessionKey {
        origin: "https://a.com".into(),
        profile: "firefox".into(),
        proxy_route: None,
    });
    assert_eq!(set.len(), 1);
}

// --- BrowserSessionManager tests ---

#[test]
fn test_manager_get_or_create_new() {
    let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
    let key = BrowserSessionKey {
        origin: "https://example.com".into(),
        profile: "chrome".into(),
        proxy_route: None,
    };
    let h1 = mgr.get_or_create(&key);
    let h2 = mgr.get_or_create(&key);
    assert!(h1.ptr_eq(&h2));
    assert_eq!(mgr.len(), 1);
}

#[test]
fn test_manager_different_keys_different_sessions() {
    let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
    let k1 = BrowserSessionKey {
        origin: "https://a.com".into(),
        profile: "chrome".into(),
        proxy_route: None,
    };
    let k2 = BrowserSessionKey {
        origin: "https://b.com".into(),
        profile: "chrome".into(),
        proxy_route: None,
    };
    let h1 = mgr.get_or_create(&k1);
    let h2 = mgr.get_or_create(&k2);
    assert!(!h1.ptr_eq(&h2));
    assert_eq!(mgr.len(), 2);
}

#[test]
fn test_manager_capacity_eviction() {
    let cfg = BrowserSessionConfig {
        max_sessions: 2,
        idle_timeout: Duration::from_secs(300),
    };
    let mgr = BrowserSessionManager::new(cfg);
    let k1 = BrowserSessionKey {
        origin: "https://a.com".into(),
        profile: "p".into(),
        proxy_route: None,
    };
    let k2 = BrowserSessionKey {
        origin: "https://b.com".into(),
        profile: "p".into(),
        proxy_route: None,
    };
    let k3 = BrowserSessionKey {
        origin: "https://c.com".into(),
        profile: "p".into(),
        proxy_route: None,
    };

    mgr.get_or_create(&k1);
    mgr.get_or_create(&k2);
    assert_eq!(mgr.len(), 2);

    // k3 should evict k1 (oldest).
    mgr.get_or_create(&k3);
    assert_eq!(mgr.len(), 2);

    // k1 should get a new handle (it was evicted).
    let h1_new = mgr.get_or_create(&k1);
    assert!(h1_new.is_valid());
}

#[test]
fn test_manager_invalidate_origin() {
    let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
    let k1 = BrowserSessionKey {
        origin: "https://a.com".into(),
        profile: "p".into(),
        proxy_route: None,
    };
    let k2 = BrowserSessionKey {
        origin: "https://a.com".into(),
        profile: "q".into(),
        proxy_route: None,
    };
    let k3 = BrowserSessionKey {
        origin: "https://b.com".into(),
        profile: "p".into(),
        proxy_route: None,
    };

    mgr.get_or_create(&k1);
    mgr.get_or_create(&k2);
    mgr.get_or_create(&k3);
    assert_eq!(mgr.len(), 3);

    mgr.invalidate_origin("https://a.com");
    assert_eq!(mgr.len(), 1); // only k3 remains
}

#[test]
fn test_manager_invalidate_all() {
    let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
    mgr.get_or_create(&BrowserSessionKey {
        origin: "https://a.com".into(),
        profile: "p".into(),
        proxy_route: None,
    });
    mgr.get_or_create(&BrowserSessionKey {
        origin: "https://b.com".into(),
        profile: "p".into(),
        proxy_route: None,
    });
    assert_eq!(mgr.len(), 2);

    mgr.invalidate_all();
    assert_eq!(mgr.len(), 0);
}

#[test]
fn test_manager_evict_idle() {
    let cfg = BrowserSessionConfig {
        max_sessions: 8,
        idle_timeout: Duration::from_nanos(1),
    };
    let mgr = BrowserSessionManager::new(cfg);
    mgr.get_or_create(&BrowserSessionKey {
        origin: "https://a.com".into(),
        profile: "p".into(),
        proxy_route: None,
    });
    mgr.get_or_create(&BrowserSessionKey {
        origin: "https://b.com".into(),
        profile: "p".into(),
        proxy_route: None,
    });
    // Let the idle timeouts elapse.
    std::thread::sleep(Duration::from_nanos(100));
    let evicted = mgr.evict_idle();
    assert_eq!(evicted, 2);
    assert_eq!(mgr.len(), 0);
}

#[test]
fn test_manager_reuses_existing_after_get_or_create() {
    let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
    let key = BrowserSessionKey {
        origin: "https://example.com".into(),
        profile: "chrome".into(),
        proxy_route: None,
    };
    let h1 = mgr.get_or_create(&key);
    let h2 = mgr.get_or_create(&key);
    assert!(h1.ptr_eq(&h2));
}

#[test]
fn test_turnstile_token_state_transition() {
    let mut state = TurnstileTokenState::Pending;
    assert!(matches!(state, TurnstileTokenState::Pending));

    state = TurnstileTokenState::Ready {
        token: "dummy-token".into(),
    };
    assert!(matches!(state, TurnstileTokenState::Ready { .. }));

    state = TurnstileTokenState::Consumed;
    assert!(matches!(state, TurnstileTokenState::Consumed));

    state = TurnstileTokenState::Rejected;
    assert!(matches!(state, TurnstileTokenState::Rejected));
}

#[test]
fn test_turnstile_token_debug_redacts_value() {
    let state = TurnstileTokenState::Ready {
        token: "sensitive-token".to_string(),
    };
    assert_eq!(format!("{state:?}"), "Ready(<redacted>)");
}

#[test]
fn test_page_fetch_classifies_configured_token_rejection() {
    let request = PageFetchRequest {
        method: http::Method::POST,
        url: "https://example.com/ajax".to_string(),
        headers: HeaderMap::new(),
        body: None,
        turnstile_token_placeholder: Some("{token}".to_string()),
        reuse_turnstile_token: true,
        token_rejection_statuses: vec![http::StatusCode::FORBIDDEN],
        token_rejection_body_markers: vec!["captcha invalid".to_string()],
    };
    assert!(request.is_token_rejected_by(&PageFetchResponse {
        url: "https://example.com/ajax".to_string(),
        status: http::StatusCode::OK,
        headers: HeaderMap::new(),
        body: "captcha invalid".to_string(),
    }));
    assert!(!request.is_token_rejected_by(&PageFetchResponse {
        url: "https://example.com/ajax".to_string(),
        status: http::StatusCode::OK,
        headers: HeaderMap::new(),
        body: "accepted".to_string(),
    }));
}

// --- BrowserSessionHandle token cache lifecycle tests ---

#[test]
fn test_handle_token_cache_starts_empty() {
    let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
    let key = BrowserSessionKey {
        origin: "https://token-cache.example".into(),
        profile: "p".into(),
        proxy_route: None,
    };
    let handle = mgr.get_or_create(&key);
    assert_eq!(handle.cached_turnstile_token(), None);
}

#[test]
fn test_handle_token_cache_roundtrip() {
    let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
    let key = BrowserSessionKey {
        origin: "https://token-cache.example".into(),
        profile: "p".into(),
        proxy_route: None,
    };
    let handle = mgr.get_or_create(&key);
    handle.cache_turnstile_token("test-token".to_string());
    assert_eq!(
        handle.cached_turnstile_token(),
        Some("test-token".to_string())
    );
}

#[test]
fn test_handle_token_cache_clear() {
    let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
    let key = BrowserSessionKey {
        origin: "https://token-cache.example".into(),
        profile: "p".into(),
        proxy_route: None,
    };
    let handle = mgr.get_or_create(&key);
    handle.cache_turnstile_token("test-token".to_string());
    handle.clear_turnstile_token();
    assert_eq!(handle.cached_turnstile_token(), None);
}

#[test]
fn test_handle_token_cache_replace() {
    let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
    let key = BrowserSessionKey {
        origin: "https://token-cache.example".into(),
        profile: "p".into(),
        proxy_route: None,
    };
    let handle = mgr.get_or_create(&key);
    handle.cache_turnstile_token("first".to_string());
    handle.cache_turnstile_token("second".to_string());
    assert_eq!(handle.cached_turnstile_token(), Some("second".to_string()));
}

#[test]
fn test_handle_token_cache_does_not_share_between_handles() {
    let mgr = BrowserSessionManager::new(BrowserSessionConfig::default());
    let k1 = BrowserSessionKey {
        origin: "https://a.example".into(),
        profile: "p".into(),
        proxy_route: None,
    };
    let k2 = BrowserSessionKey {
        origin: "https://b.example".into(),
        profile: "p".into(),
        proxy_route: None,
    };
    let h1 = mgr.get_or_create(&k1);
    let h2 = mgr.get_or_create(&k2);
    h1.cache_turnstile_token("token-a".to_string());
    assert_eq!(h2.cached_turnstile_token(), None);
}
impl BrowserSessionHandle {
    /// Tests whether two handles reference one logical browser session.
    fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}
