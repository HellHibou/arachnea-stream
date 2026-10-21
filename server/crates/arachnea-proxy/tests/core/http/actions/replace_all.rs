use super::*;
use crate::core::http::actions::PostActionContext;
use crate::core::http::actions::ProxyHttpPostAction;

fn replace_all_action(params: HashMap<String, String>) -> ReplaceAllAction {
    ReplaceAllAction::from_config(&ProxyHttpPostActionConfig {
        action: "ReplaceAll".to_string(),
        order: None,
        params,
    })
    .unwrap()
}

#[test]
fn test_is_textual_content_type() {
    let action = replace_all_action(HashMap::from([
        ("pattern".to_string(), "hello".to_string()),
        ("replacement".to_string(), "hi".to_string()),
    ]));

    assert!(action.is_textual_content_type("text/html"));
    assert!(action.is_textual_content_type("text/plain; charset=utf-8"));
    assert!(action.is_textual_content_type("application/json"));
    assert!(action.is_textual_content_type("application/javascript"));
    assert!(action.is_textual_content_type("image/svg+xml"));
    assert!(action.is_textual_content_type("application/vnd.apple.mpegurl"));
    assert!(action.is_textual_content_type("application/dash+xml"));
    assert!(!action.is_textual_content_type("application/x-mpegURL"));
    assert!(!action.is_textual_content_type("audio/mpegurl; charset=utf-8"));
    assert!(!action.is_textual_content_type("audio/x-mpegurl"));
    assert!(!action.is_textual_content_type("image/png"));
    assert!(!action.is_textual_content_type("video/mp4"));
    assert!(!action.is_textual_content_type("application/octet-stream"));
}

#[test]
fn test_custom_content_types_override_default_filter() {
    let action = replace_all_action(HashMap::from([
        ("pattern".to_string(), "hello".to_string()),
        ("replacement".to_string(), "hi".to_string()),
        (
            "content_types".to_string(),
            serde_json::json!(["application/x-mpegurl", "audio/mpegurl"]).to_string(),
        ),
    ]));

    assert!(action.is_textual_content_type("application/x-mpegURL"));
    assert!(action.is_textual_content_type("audio/mpegurl; charset=utf-8"));
    assert!(!action.is_textual_content_type("text/plain"));
    assert!(!action.is_textual_content_type("application/vnd.apple.mpegurl"));
}

#[test]
fn test_proxy_variable_uses_public_path() {
    let context = PostActionContext {
        entry_point: "http://127.0.0.1:8080/api/proxy".to_string(),
        target_url: "https://example.test/manifest.mpd".to_string(),
        opts_encoded: String::new(),
    };
    assert_eq!(
        resolve_variables("{proxy}/https://cdn.test/a", &context),
        "/api/proxy/https://cdn.test/a"
    );

    let context = PostActionContext {
        entry_point: "arachnea://api/proxy".to_string(),
        target_url: "https://example.test/manifest.mpd".to_string(),
        opts_encoded: String::new(),
    };
    assert_eq!(
        resolve_variables("{proxy}/https://cdn.test/a", &context),
        "/api/proxy/https://cdn.test/a"
    );
}

#[test]
fn test_base_url_variable_uses_target_origin() {
    let context = PostActionContext {
        entry_point: "http://127.0.0.1:8080/api/proxy".to_string(),
        target_url: "https://myhost.be:8080/path/to/manifest.mpd?token=abc".to_string(),
        opts_encoded: String::new(),
    };

    assert_eq!(
        resolve_variables("{proxy}/{base_url}/m6web/", &context),
        "/api/proxy/https://myhost.be:8080/m6web/"
    );
}

#[test]
fn test_proxy_inherited_with_opts() {
    let context = PostActionContext {
        entry_point: "http://127.0.0.1:8080/api/proxy".to_string(),
        target_url: "https://example.test/manifest.mpd".to_string(),
        opts_encoded: "ABCD".to_string(),
    };
    assert_eq!(
        resolve_variables("{proxy_inherited}", &context),
        "/api/proxy/opts_ABCD"
    );
}

#[test]
fn test_proxy_inherited_without_opts() {
    let context = PostActionContext {
        entry_point: "http://127.0.0.1:8080/api/proxy".to_string(),
        target_url: "https://example.test/manifest.mpd".to_string(),
        opts_encoded: String::new(),
    };
    assert_eq!(
        resolve_variables("{proxy_inherited}", &context),
        "/api/proxy"
    );
}

#[test]
fn test_replace_all_text() {
    let config = ProxyHttpPostActionConfig {
        action: "ReplaceAll".to_string(),
        order: None,
        params: HashMap::from([
            ("pattern".to_string(), "hello".to_string()),
            ("replacement".to_string(), "hi".to_string()),
        ]),
    };
    let action = ReplaceAllAction::from_config(&config).unwrap();
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "text/plain".to_string());
    let body = b"hello world, hello!".to_vec();
    let result = action
        .post_apply(200, &mut headers, body, &PostActionContext::default())
        .unwrap();
    assert_eq!(result, b"hi world, hi!".to_vec());
}

#[test]
fn test_replace_all_no_match() {
    let config = ProxyHttpPostActionConfig {
        action: "ReplaceAll".to_string(),
        order: None,
        params: HashMap::from([
            ("pattern".to_string(), "xyz".to_string()),
            ("replacement".to_string(), "abc".to_string()),
        ]),
    };
    let action = ReplaceAllAction::from_config(&config).unwrap();
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "text/plain".to_string());
    let body = b"hello world".to_vec();
    let result = action
        .post_apply(
            200,
            &mut headers,
            body.clone(),
            &PostActionContext::default(),
        )
        .unwrap();
    assert_eq!(result, body);
}

#[test]
fn test_replace_all_binary_unaffected() {
    let config = ProxyHttpPostActionConfig {
        action: "ReplaceAll".to_string(),
        order: None,
        params: HashMap::from([
            ("pattern".to_string(), "hello".to_string()),
            ("replacement".to_string(), "hi".to_string()),
        ]),
    };
    let action = ReplaceAllAction::from_config(&config).unwrap();
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "image/png".to_string());
    let body = b"hello world".to_vec();
    let result = action
        .post_apply(
            200,
            &mut headers,
            body.clone(),
            &PostActionContext::default(),
        )
        .unwrap();
    assert_eq!(result, body);
}

#[test]
fn test_from_config_missing_pattern() {
    let config = ProxyHttpPostActionConfig {
        action: "ReplaceAll".to_string(),
        order: None,
        params: HashMap::from([("replacement".to_string(), "hi".to_string())]),
    };
    assert!(ReplaceAllAction::from_config(&config).is_err());
}

#[test]
fn test_from_config_invalid_regex() {
    let config = ProxyHttpPostActionConfig {
        action: "ReplaceAll".to_string(),
        order: None,
        params: HashMap::from([
            ("pattern".to_string(), "[invalid".to_string()),
            ("replacement".to_string(), "hi".to_string()),
        ]),
    };
    assert!(ReplaceAllAction::from_config(&config).is_err());
}
