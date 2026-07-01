use arachnea_proxy::core::http::actions::{apply_post_actions, PostActionContext, ProxyHttpPostActionConfig};
use std::collections::HashMap;

#[test]
fn test_replace_all_action_pipeline() {
    // Simulate a JSON response from RTBF
    let body = br#"{"data":{"id":"17226","name":"CATEGORY_LIST"}}"#.to_vec();
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "application/json".to_string());

    let actions = vec![ProxyHttpPostActionConfig {
        action: "ReplaceAll".to_string(),
        order: None,
        params: HashMap::from([
            ("pattern".to_string(), "CATEGORY_LIST".to_string()),
            (
                "replacement".to_string(),
                "CATEGORY_LIST  Hello Toto".to_string(),
            ),
        ]),
    }];

    let result = apply_post_actions(200, &mut headers, body, &actions, &PostActionContext::default()).unwrap();

    let result_str = String::from_utf8(result.clone()).unwrap();
    assert!(
        result_str.contains("Hello Toto"),
        "Expected 'Hello Toto' in result, got: {result_str}"
    );
    assert!(
        result_str.contains("CATEGORY_LIST  Hello Toto"),
        "Expected replacement text, got: {result_str}"
    );

    // Content-Length must be managed by the caller (parse_http_response in client.rs)
    assert!(!result.is_empty(), "Body should not be empty after replace");
    eprintln!("ReplaceAll result ({} bytes): {}", result.len(), result_str);
}

#[test]
fn test_replace_all_preserves_non_text_body() {
    let body = b"hello world".to_vec();
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "image/png".to_string());

    let actions = vec![ProxyHttpPostActionConfig {
        action: "ReplaceAll".to_string(),
        order: None,
        params: HashMap::from([
            ("pattern".to_string(), "hello".to_string()),
            ("replacement".to_string(), "hi".to_string()),
        ]),
    }];

    let result = apply_post_actions(200, &mut headers, body.clone(), &actions, &PostActionContext::default()).unwrap();
    assert_eq!(result, body);
}

#[test]
fn test_no_actions_preserves_body() {
    let body = b"test body".to_vec();
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "text/plain".to_string());

    let result = apply_post_actions(200, &mut headers, body.clone(), &[], &PostActionContext::default()).unwrap();
    assert_eq!(result, body);
}

#[test]
fn test_empty_body_unchanged() {
    let body = Vec::new();
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "text/plain".to_string());

    let actions = vec![ProxyHttpPostActionConfig {
        action: "ReplaceAll".to_string(),
        order: None,
        params: HashMap::from([
            ("pattern".to_string(), "anything".to_string()),
            ("replacement".to_string(), "replaced".to_string()),
        ]),
    }];

    let result = apply_post_actions(200, &mut headers, body.clone(), &actions, &PostActionContext::default()).unwrap();
    assert_eq!(result, body);
}
