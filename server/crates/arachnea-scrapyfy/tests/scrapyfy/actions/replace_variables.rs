use super::*;

#[test]
fn apply_replaces_only_dynamic_placeholders() {
    let params = HashMap::new();
    let dynamic = Mutex::new(DynamicTemplateVariables::new());
    dynamic.lock().unwrap().insert(&params, "@A", "80").unwrap();

    let values = apply(vec!["{@A}:{country}:{Missing}".to_string()], &dynamic);

    assert_eq!(values, vec!["80:{country}:{Missing}".to_string()]);
}

#[test]
fn validate_accepts_action_without_prefix_configuration() {
    validate("port", "entry").unwrap();
}
