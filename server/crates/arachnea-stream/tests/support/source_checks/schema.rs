//! Compatibility checks for extended operations using the existing input format.

use super::SourceCheck;
use arachnea_scrapyfy::scrapyfy::scraper_manager::tests::read_strict_yaml;

fn contract(operation: &str, inputs: &str) -> anyhow::Result<SourceCheck> {
    let yaml = format!(
        r#"schema_version: 1
source: example
source_config: arachnea-stream/example.yaml
capabilities:
  expected: [service_stream_metadata, {operation}]
  not_expected: []
steps:
  - id: metadata
    operation: service_stream_metadata
    expect:
      items_path: []
      min_count: 1
      min_valid_count: 1
      required_non_empty: [[title]]
  - id: content
    operation: {operation}
{inputs}
    expect:
      items_path: []
      min_count: 1
      min_valid_count: 1
      required_non_empty: [[title]]
"#
    );
    read_strict_yaml(yaml.as_bytes())
}

#[test]
fn extended_operations_accept_existing_input_forms() -> anyhow::Result<()> {
    for operation in [
        "get_season",
        "get_live",
        "get_banners",
        "get_recommendations",
        "resolve_stream",
        "get_episode",
    ] {
        contract(operation, "    inputs:\n      query_url: reference")?.validate()?;
    }
    contract("list_lives", "")?.validate()?;
    Ok(())
}

#[test]
fn extended_operations_reject_extra_parameters() -> anyhow::Result<()> {
    for operation in ["get_category", "get_section", "get_live", "resolve_stream"] {
        for extra in ["channel: example", "home_type: example"] {
            assert!(contract(
                operation,
                &format!("    inputs:\n      query_url: reference\n      {extra}")
            )?
            .validate()
            .is_err());
        }
        assert!(contract(
            operation,
            "    inputs:\n      query_url: reference\n      source_params: {channel: example}"
        )
        .is_err());
    }
    assert!(
        contract("list_lives", "    inputs:\n      query_url: reference")?
            .validate()
            .is_err()
    );
    Ok(())
}

#[test]
fn pagination_remains_literal_and_operation_scoped() -> anyhow::Result<()> {
    for operation in ["get_season", "get_recommendations"] {
        contract(
            operation,
            "    inputs:\n      query_url: reference\n      page: 2",
        )?
        .validate()?;
        for page in [
            "0",
            "\"2\"",
            "{from_step: metadata, items_path: [], select: first_valid, value_path: [title]}",
        ] {
            assert!(contract(
                operation,
                &format!("    inputs:\n      query_url: reference\n      page: {page}")
            )?
            .validate()
            .is_err());
        }
    }
    for operation in ["get_live", "get_banners", "resolve_stream", "get_episode"] {
        assert!(contract(
            operation,
            "    inputs:\n      query_url: reference\n      page: 2"
        )?
        .validate()
        .is_err());
    }
    Ok(())
}
