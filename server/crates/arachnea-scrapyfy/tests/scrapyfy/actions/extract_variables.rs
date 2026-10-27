use super::*;

#[test]
fn apply_extracts_variables_and_preserves_values() {
    let texts = vec!["var A=80; var B=443;".to_string()];
    let params = HashMap::new();
    let dynamic = Mutex::new(DynamicTemplateVariables::new());

    let out = apply(
        texts.clone(),
        r"\b([A-Za-z][A-Za-z0-9_]*)\s*=\s*(\d+)\s*;",
        "@{1}",
        "{2}",
        ExtractVariablesDuplicatePolicy::Error,
        &params,
        &dynamic,
    )
    .unwrap();

    assert_eq!(out, texts);
    assert_eq!(
        dynamic
            .lock()
            .unwrap()
            .as_map()
            .get("@A")
            .map(String::as_str),
        Some("80")
    );
    assert_eq!(
        dynamic
            .lock()
            .unwrap()
            .as_map()
            .get("@B")
            .map(String::as_str),
        Some("443")
    );
}

#[test]
fn apply_rejects_duplicate_variables() {
    let texts = vec!["A=80; A=81;".to_string()];
    let params = HashMap::new();
    let dynamic = Mutex::new(DynamicTemplateVariables::new());

    let error = apply(
        texts,
        r"\b([A-Za-z][A-Za-z0-9_]*)\s*=\s*(\d+)\s*;",
        "@{1}",
        "{2}",
        ExtractVariablesDuplicatePolicy::Error,
        &params,
        &dynamic,
    )
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("Duplicate dynamic template variable"));
}

#[test]
fn apply_can_replace_duplicate_variables() {
    let texts = vec!["A=80; A=81;".to_string()];
    let params = HashMap::new();
    let dynamic = Mutex::new(DynamicTemplateVariables::new());

    apply(
        texts,
        r"\b([A-Za-z][A-Za-z0-9_]*)\s*=\s*(\d+)\s*;",
        "@{1}",
        "{2}",
        ExtractVariablesDuplicatePolicy::Replace,
        &params,
        &dynamic,
    )
    .unwrap();

    assert_eq!(
        dynamic
            .lock()
            .unwrap()
            .as_map()
            .get("@A")
            .map(String::as_str),
        Some("81")
    );
}
