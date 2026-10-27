use super::*;
use crate::client::test_support::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

/// Verifies explicit Cloudflare refresh uses the solver refresh hook.
#[tokio::test]
async fn refresh_cloudflare_uses_refresh_hook_and_solver_user_agent() {
    let send_calls = Arc::new(AtomicUsize::new(0));
    let refresh_calls = Arc::new(AtomicUsize::new(0));
    let engine = RefreshEngine {
        send_calls: send_calls.clone(),
        refresh_calls: refresh_calls.clone(),
    };
    let config = ArachneaHttpConfig::builder()
        .engine_instance(StaticEngine {
            calls: Arc::new(AtomicUsize::new(0)),
        })
        .cloudflare_solver_instance(engine)
        .build()
        .expect("valid config");
    let client = ArachneaHttpClient::new(config).await.expect("client");

    client
        .refresh_cloudflare("https://refresh-hook.example/protected")
        .await
        .expect("refresh succeeds");

    let request = client
        .engine_request(RequestOptions {
            method: Method::GET,
            url: "https://refresh-hook.example/again".to_string(),
            mode: None,
            headers: HeaderMap::new(),
            body: None,
        })
        .await
        .expect("engine request");

    assert_eq!(send_calls.load(Ordering::SeqCst), 0);
    assert_eq!(refresh_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        request.headers.get(USER_AGENT).expect("user-agent"),
        "SolverUA/1"
    );
    assert_eq!(
        request.headers.get(COOKIE).expect("cookie"),
        "cf_clearance=ok"
    );
}
