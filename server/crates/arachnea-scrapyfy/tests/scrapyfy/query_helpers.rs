use super::*;

#[test]
fn resolve_nested_template_params_resolves_chained_collection_params() {
    let params = HashMap::from([
        ("catalog_code".to_string(), "fr-fr".to_string()),
        (
            "android_graphql_url".to_string(),
            "https://app-api.tf1.fr/graphql/{catalog_code}/android".to_string(),
        ),
        (
            "android_home_covers_url".to_string(),
            "{android_graphql_url}?id=abc".to_string(),
        ),
    ]);

    let resolved = resolve_nested_template_params(&params);

    assert_eq!(
        resolved.get("android_graphql_url").map(String::as_str),
        Some("https://app-api.tf1.fr/graphql/fr-fr/android")
    );
    assert_eq!(
        resolved.get("android_home_covers_url").map(String::as_str),
        Some("https://app-api.tf1.fr/graphql/fr-fr/android?id=abc")
    );
}

#[test]
fn replace_template_placeholders_resolves_dynamic_prefixed_keys() {
    let params = HashMap::from([
        ("country".to_string(), "BE".to_string()),
        ("@PortPart".to_string(), "80".to_string()),
    ]);

    let (rendered, missing) = replace_template_placeholders("{country}:{@PortPart}", &params);

    assert_eq!(rendered, "BE:80");
    assert!(missing.is_empty());
}

#[test]
fn dynamic_template_variables_reject_missing_prefix() {
    let params = HashMap::new();
    let mut dynamic = DynamicTemplateVariables::new();

    let error = dynamic.insert(&params, "PortPart", "80").unwrap_err();

    assert!(error.to_string().contains("must start with @"));
}

#[test]
fn dynamic_template_variables_reject_duplicates() {
    let params = HashMap::new();
    let mut dynamic = DynamicTemplateVariables::new();

    dynamic.insert(&params, "@PortPart", "80").unwrap();
    let error = dynamic.insert(&params, "@PortPart", "81").unwrap_err();

    assert!(error
        .to_string()
        .contains("Duplicate dynamic template variable"));
}

#[test]
fn build_template_params_with_dynamic_variables_keeps_namespaces_separate() {
    let params = HashMap::from([("country".to_string(), "BE".to_string())]);
    let mut dynamic = DynamicTemplateVariables::new();

    dynamic.insert(&params, "@PortPart", "80").unwrap();
    let merged = build_template_params_with_dynamic_variables(&params, &dynamic).unwrap();

    assert_eq!(merged.get("country").map(String::as_str), Some("BE"));
    assert_eq!(merged.get("@PortPart").map(String::as_str), Some("80"));
}

#[test]
fn build_template_params_with_dynamic_variables_rejects_standard_dynamic_prefix() {
    let params = HashMap::from([("@PortPart".to_string(), "80".to_string())]);
    let dynamic = DynamicTemplateVariables::new();

    let error = build_template_params_with_dynamic_variables(&params, &dynamic).unwrap_err();

    assert!(error
        .to_string()
        .contains("cannot use reserved dynamic prefix"));
}
