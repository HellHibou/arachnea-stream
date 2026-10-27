use super::*;

#[test]
fn test_simple_js_execution() {
    let texts = vec!["var x = 42;".to_string()];
    let result = apply(texts, 500, None, false, &[]).unwrap();
    assert!(result.contains(&"x=42".to_string()));
}

#[test]
fn test_text_is_js_not_html() {
    let texts = vec!["<html><script>var a = 1;</script></html>".to_string()];
    let result = apply(texts, 500, None, false, &[]);
    assert!(result.is_err());
}

#[test]
fn test_inject_html_scripts_extracts_from_response_body() {
    let texts = vec!["".to_string()];
    let html = "<html><script>var a = 99;</script></html>";
    let result = apply(texts, 500, Some(html), true, &[]).unwrap();
    assert!(result.contains(&"a=99".to_string()));
}

#[test]
fn test_hardcoded_scripts_are_injected() {
    let texts = vec!["var y = helper();".to_string()];
    let result = apply(
        texts,
        500,
        None,
        false,
        &["function helper() { return 42; }".to_string()],
    )
    .unwrap();
    assert!(result.contains(&"y=42".to_string()));
}

#[test]
fn test_inject_html_plus_hardcoded_plus_text() {
    let texts = vec!["var z = x + y;".to_string()];
    let html = "<html><script>var x = 10;</script></html>";
    let result = apply(texts, 500, Some(html), true, &["var y = 5;".to_string()]).unwrap();
    assert!(result.contains(&"x=10".to_string()));
    assert!(result.contains(&"y=5".to_string()));
    assert!(result.contains(&"z=15".to_string()));
}

#[test]
fn test_numeric_variable_detection() {
    let texts = vec!["var score = 100; var name = 'alice';".to_string()];
    let result = apply(texts, 500, None, false, &[]).unwrap();
    assert!(result.contains(&"score=100".to_string()));
    assert!(!result.iter().any(|line| line.starts_with("name=")));
}

#[test]
fn test_xor_evaluation() {
    let texts = vec!["var Eight4EightNine = 88 ^ 0;".to_string()];
    let result = apply(texts, 500, None, false, &[]).unwrap();
    assert!(result.contains(&"Eight4EightNine=88".to_string()));
}

#[test]
fn test_transitive_resolution() {
    let texts = vec![
        "Six0Eight = 0; ZeroThreeSeven = Six0Eight ^ 0; Eight4EightNine = ZeroThreeSeven ^ 88;"
            .to_string(),
    ];
    let result = apply(texts, 500, None, false, &[]).unwrap();
    assert!(result.contains(&"Eight4EightNine=88".to_string()));
}

#[test]
fn test_loop_limit_does_not_block() {
    let texts = vec!["var quick = 1;".to_string()];
    let result = apply(texts, 5000, None, false, &[]).unwrap();
    assert!(result.contains(&"quick=1".to_string()));
}

#[test]
fn test_empty_input() {
    let texts: Vec<String> = vec![];
    let result = apply(texts, 500, None, false, &[]).unwrap();
    assert!(result.is_empty());
}

#[test]
fn test_spysone_simulation() {
    let html =
        r#"<html><script>ZeroThreeSeven=0;Eight4EightNine=ZeroThreeSeven^88;</script></html>"#;
    let texts = vec!["".to_string()];
    let result = apply(texts, 500, Some(html), true, &[]).unwrap();
    assert!(result
        .iter()
        .any(|line| line.starts_with("Eight4EightNine")));
}

#[test]
fn test_configured_scripts_run_before_html_scripts() {
    let html = r#"<html><script>var result = (typeof document !== 'undefined' && typeof document.write === 'function') ? 1 : 0;</script></html>"#;
    let texts = vec!["var spysone_port = result + 99;".to_string()];
    let result = apply(
        texts,
        500,
        Some(html),
        true,
        &["var document = { write: function(s) {} };".to_string()],
    )
    .unwrap();
    assert!(result.iter().any(|line| line == "spysone_port=100"));
}

#[test]
fn test_configured_script_provides_document_write_html_has_packer_vars() {
    let packer_script =
        "Eight4EightNine=90^0;TwoSevenFive=60^0;Seven4SixZero=47^0;SixNineFour=27^0;";
    let html = format!(r#"<html><script>{}</script></html>"#, packer_script);
    let texts = vec![
        "var spysone_port = (Eight4EightNine ^ TwoSevenFive) + (Seven4SixZero ^ SixNineFour);"
            .to_string(),
    ];
    let result = apply(
        texts,
        500,
        Some(&html),
        true,
        &["var document = { write: function(s) { return s; } };".to_string()],
    )
    .unwrap();
    assert!(result.iter().any(|line| line == "Eight4EightNine=90"));
    assert!(result.iter().any(|line| line == "TwoSevenFive=60"));
    assert!(result.iter().any(|line| line == "Seven4SixZero=47"));
    assert!(result.iter().any(|line| line == "SixNineFour=27"));
    // (90 ^ 60) + (47 ^ 27) = 102 + 52 = 154
    assert!(result.iter().any(|line| line == "spysone_port=154"));
}

#[test]
fn test_window_stub_prevents_ga_crash() {
    let html = r#"<html><script>window.dataLayer = window.dataLayer || []; function gtag(){dataLayer.push(arguments);} gtag('js', new Date()); gtag('config', 'G-XWX5S73YKH');</script><script>var Eight4EightNine = 88 ^ 0;</script></html>"#;
    let texts = vec!["var spysone_port = Eight4EightNine + 0;".to_string()];
    let result = apply(
        texts,
        500,
        Some(html),
        true,
        &[
            "var window = {}; var dataLayer = []; var document = { write: function(s) {} };"
                .to_string(),
        ],
    )
    .unwrap();
    assert!(result.iter().any(|line| line == "Eight4EightNine=88"));
    assert!(result.iter().any(|line| line == "spysone_port=88"));
}
