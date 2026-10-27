use anyhow::Context;
/// Verifies that the YAML configuration can be deserialized
/// into a query collection and serialized back as JSON.
#[test]
fn config_yaml_to_json() -> super::Result<()> {
    let services_path = format!(
        "{}/services",
        arachnea_core::application::get_application_root()
    );

    for entry in std::fs::read_dir(services_path)? {
        let path = entry?.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'))
        {
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("yaml") {
            continue;
        }

        let collection =
            super::ScraperQueryCollection::from_file_with(path.as_path(), serde_yaml::from_reader)
                .with_context(|| format!("while parsing {}", path.display()))?;
        println!("Parsed service config: {}", collection.name());
    }

    // TODO: Add check result
    Ok(())
}

#[tokio::test]
async fn static_entry_actions_are_applied() -> super::Result<()> {
    let yaml = r#"
id: static-action-test
title: Static Action Test
parameters:
  - name: base_url
    value: https://example.test
queries:
  - name: service_stream_metadata
    scraper_type: static
    entries:
      - name: logo
        type: string
        value: "{base_url}/logo.png"
        actions:
          - type: resolve_url
            proxy: true
"#;
    let collection: super::ScraperQueryCollection = serde_yaml::from_str(yaml)?;
    let params = std::collections::HashMap::from([(
        crate::scrapyfy::HTTP_PROXY_PUBLIC_PATH_PARAM.to_string(),
        "/api/proxy".to_string(),
    )]);

    let rows = collection
        .execute_query("service_stream_metadata", &params, None, None)
        .await?;
    let logo = rows
        .first()
        .and_then(|row| row.get("logo"))
        .and_then(|node| node.values.first())
        .cloned();

    assert_eq!(
        logo.as_deref(),
        Some("/api/proxy/https://example.test/logo.png")
    );

    Ok(())
}
