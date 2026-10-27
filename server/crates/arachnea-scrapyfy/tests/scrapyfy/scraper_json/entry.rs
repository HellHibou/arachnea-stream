use super::*;
use serde_json::json;

#[test]
fn test_select_json_values_with_filter() {
    let data = json!({
        "images": [
            {"role": "banner", "external_key": "key1"},
            {"role": "mea", "external_key": "key2"},
            {"role": "thumbnail", "external_key": "key3"},
            {"role": "mea", "external_key": "key4"}
        ]
    });

    // Test filtering by role=mea
    let results = select_json_values(
        &data,
        Some("/images/*[role=mea]/external_key"),
        HtmlScraperSelectMode::All,
    );
    let strings: Vec<String> = results
        .into_iter()
        .flat_map(json_value_to_strings)
        .collect();

    assert_eq!(strings, vec!["key2", "key4"]);
}

#[test]
fn test_select_json_values_without_filter() {
    let data = json!({
        "images": [
            {"external_key": "key1"},
            {"external_key": "key2"}
        ]
    });

    // Test without filter (should get all)
    let results = select_json_values(
        &data,
        Some("/images/*/external_key"),
        HtmlScraperSelectMode::All,
    );
    let strings: Vec<String> = results
        .into_iter()
        .flat_map(json_value_to_strings)
        .collect();

    assert_eq!(strings, vec!["key1", "key2"]);
}

#[test]
fn test_select_json_values_with_nested_filter() {
    let data = json!([
        {"content": [{"idType": 7, "content": {"name": "Cat A"}}]},
        {"content": [{"idType": 8, "content": {"name": "Film B"}}]},
        {"content": [{"idType": 8, "content": {"name": "Film C"}}]}
    ]);

    let results = select_json_values(
        &data,
        Some("/*[content/0/idType=8]/content/0/content/name"),
        HtmlScraperSelectMode::All,
    );
    let strings: Vec<String> = results
        .into_iter()
        .flat_map(json_value_to_strings)
        .collect();

    assert_eq!(strings, vec!["Film B", "Film C"]);
}

#[test]
fn test_select_json_values_with_nested_filter_preserves_brackets() {
    let data = json!({
        "result": {
            "homecontent": [
                {"content": [{"idType": 7}]},
                {"content": [{"idType": 8}]},
                {"content": [{"idType": 8}]}
            ]
        }
    });

    let results = select_json_values(
        &data,
        Some("/result/homecontent/*[content/0/idType=8]"),
        HtmlScraperSelectMode::All,
    );

    assert_eq!(results.len(), 2);
}
