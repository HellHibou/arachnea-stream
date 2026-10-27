use super::*;
use arachnea_core::error_code::ErrorCodeGenerator;
use std::collections::HashMap;

#[test]
fn error_code_generator_monotonic() {
    let gen = ErrorCodeGenerator::new();
    let a = gen.next_code();
    let b = gen.next_code();
    let c = gen.next_code();

    let a_str = a.to_string();
    let b_str = b.to_string();
    let c_str = c.to_string();

    assert!(
        a_str.starts_with("ARACHNEA_E"),
        "code should start with prefix"
    );
    assert!(b_str >= a_str, "codes should be monotonic");
    assert!(c_str >= b_str, "codes should be monotonic");
    assert_ne!(a_str, b_str, "consecutive codes should differ");
}

#[test]
fn error_code_format() {
    let gen = ErrorCodeGenerator::new();
    let code = gen.next_code();
    let s = code.to_string();

    // Format: ARACHNEA_E{millis}{seq:02}
    assert!(s.starts_with("ARACHNEA_E"), "missing prefix");
    let numeric = s.trim_start_matches("ARACHNEA_E");
    assert!(
        numeric.len() >= 15,
        "expected millis + 2-digit seq, got '{}'",
        numeric
    );
    assert!(
        numeric.chars().all(|c| c.is_ascii_digit()),
        "numeric part should be all digits, got '{}'",
        numeric,
    );

    // Sequence part (last 2 chars)
    let seq = &numeric[numeric.len() - 2..];
    assert!(
        seq.parse::<u8>().is_ok(),
        "sequence should be 2-digit number, got '{}'",
        seq,
    );
}

#[test]
fn error_code_serializable() {
    let gen = ErrorCodeGenerator::new();
    let code = gen.next_code();
    let json = serde_json::to_string(&code).expect("serialization should succeed");
    assert!(
        json.contains("ARACHNEA_E"),
        "serialized code should contain prefix"
    );
}

#[test]
fn aggregation_result_ok_has_no_errors() {
    let data: Vec<HashMap<String, String>> = vec![HashMap::new()];
    let result = ScraperAggregationResult::ok(data.clone());
    assert_eq!(result.data, data);
    assert!(result.errors.is_empty());
    assert!(result.is_ok());
}

#[test]
fn aggregation_result_with_errors() {
    let data: Vec<i32> = vec![1, 2, 3];
    let errors = vec![ScraperExecutionError {
        code: ErrorCodeGenerator::new().next_code(),
        operation: "test_op".to_string(),
        source: Some("source-a".to_string()),
        origin: ScraperErrorOrigin::Backend,
        message: "something went wrong".to_string(),
    }];
    let result = ScraperAggregationResult::new(data.clone(), errors);
    assert_eq!(result.data, vec![1, 2, 3]);
    assert!(!result.errors.is_empty());
    assert!(!result.is_ok());
}

#[test]
fn execution_error_has_code() {
    let gen = ErrorCodeGenerator::new();
    let error = ScraperExecutionError {
        code: gen.next_code(),
        operation: "load_home".to_string(),
        source: Some("my-source".to_string()),
        origin: ScraperErrorOrigin::Backend,
        message: "HTTP 500".to_string(),
    };
    assert!(
        error.code.to_string().starts_with("ARACHNEA_E"),
        "error should carry a correlation code",
    );
    assert_eq!(error.operation, "load_home");
    assert_eq!(error.source.as_deref(), Some("my-source"));
}

#[test]
fn execution_error_nullable_source() {
    let gen = ErrorCodeGenerator::new();
    let error = ScraperExecutionError {
        code: gen.next_code(),
        operation: "validate".to_string(),
        source: None,
        origin: ScraperErrorOrigin::Backend,
        message: "pre-execution validation failed".to_string(),
    };
    assert!(
        error.source.is_none(),
        "pre-execution errors should have no source"
    );
}
