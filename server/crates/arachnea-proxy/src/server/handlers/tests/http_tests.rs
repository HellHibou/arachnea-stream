use super::*;
use crate::core::{PROXY_HEADER_PARAMETER_COUNTRIES, PROXY_PARAMETER_COUNTRIES};

/// Verifies that registered headers are extracted into generic parameters.
#[test]
fn extracts_registered_parameter_headers() {
    let headers = vec![Header {
        name: PROXY_HEADER_PARAMETER_COUNTRIES.to_string(),
        value: r#"["be", "BE"]"#.to_string(),
    }];
    let definitions = vec![ParameterDefinition::new(
        PROXY_HEADER_PARAMETER_COUNTRIES,
        PROXY_PARAMETER_COUNTRIES,
        false,
    )];

    let context = context_from_headers(&headers, &definitions, ClientContext::new());

    assert_eq!(
        context.get_string(PROXY_PARAMETER_COUNTRIES),
        Some(r#"["BE"]"#)
    );
}

/// Verifies that non-forwarded parameter headers are stripped from HTTP relay requests.
#[test]
fn strips_non_forwarded_parameter_headers() {
    let definitions = vec![ParameterDefinition::new(
        PROXY_HEADER_PARAMETER_COUNTRIES,
        PROXY_PARAMETER_COUNTRIES,
        false,
    )];

    assert!(should_strip_header(
        "arachnea-proxy-countries",
        &definitions
    ));
}

/// Verifies that forwarded parameter headers can be preserved.
#[test]
fn preserves_forwarded_parameter_headers() {
    let definitions = vec![ParameterDefinition::new(
        "Arachnea-Proxy-Trace",
        "trace",
        true,
    )];

    assert!(!should_strip_header("arachnea-proxy-trace", &definitions));
}
