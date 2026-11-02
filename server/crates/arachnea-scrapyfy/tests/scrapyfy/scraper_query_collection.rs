use anyhow::Context;
/// Verifies that the YAML configuration can be deserialized
/// into a query collection and serialized back as YAML.
#[test]
fn service_configs_load_and_round_trip() -> super::Result<()> {
    let mut directories =
        vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../services")];
    let mut count = 0;
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().and_then(|value| value.to_str()) == Some("yaml") {
                let collection =
                    super::ScraperQueryCollection::from_file_with(&path, serde_yaml::from_reader)
                        .with_context(|| format!("while parsing {}", path.display()))?;
                let yaml = serde_yaml::to_string(&collection)?;
                let restored: super::ScraperQueryCollection = serde_yaml::from_str(&yaml)?;
                assert_eq!(restored.name(), collection.name());
                if collection.name() == "tf1-fr" {
                    for (country, catalog) in [
                        ("fr", "fr-fr"),
                        ("be", "fr-be"),
                        ("lu", "fr-lu"),
                        ("ch", "fr-ch"),
                    ] {
                        let mut raw: super::ScraperQueryCollectionRaw =
                            serde_yaml::from_reader(std::fs::File::open(&path)?)?;
                        crate::scrapyfy::service_parameters::apply_overrides(
                            &mut raw.parameters,
                            &std::collections::HashMap::from([("country".into(), country.into())]),
                        )?;
                        let configured: super::ScraperQueryCollection = raw.try_into()?;
                        let values = configured.effective_parameters();
                        assert_eq!(
                            values
                                .iter()
                                .find(|value| value.name == "catalog_code")
                                .unwrap()
                                .value,
                            catalog
                        );
                        assert_eq!(
                            values
                                .iter()
                                .find(|value| value.name == "base_url")
                                .unwrap()
                                .value,
                            format!("https://www.tf1.fr/{catalog}")
                        );
                    }
                }
                count += 1;
            }
        }
    }
    assert!(
        count > 0,
        "production service configurations must be checked"
    );

    Ok(())
}

#[tokio::test]
async fn static_entry_actions_are_applied() -> super::Result<()> {
    let yaml = r#"
id: static-action-test
title: Static Action Test
metadata:
  languages: [fr]
constants:
  - name: logo_url
    value: "{base_url}/logo.png"
parameters:
  - name: base_url
    label: Base URL
    value: https://example.test
    editor:
      type: regex
      pattern: '^https?://[^\s/?#]+$'
      message: Invalid URL
queries:
  - name: service_stream_metadata
    scraper_type: static
    entries:
      - name: metadata
        type: object
        value: "{service_metadata}"
      - name: logo
        type: string
        value: "{logo_url}"
        actions:
          - type: resolve_url
            proxy: true
"#;
    let mut raw: super::ScraperQueryCollectionRaw = serde_yaml::from_str(yaml)?;
    let defaults = crate::scrapyfy::service_parameters::resolve_values(
        &raw.constants
            .iter()
            .chain(&raw.parameters)
            .cloned()
            .collect::<Vec<_>>(),
    )?;
    assert_eq!(defaults["logo_url"], "https://example.test/logo.png");
    assert!(crate::scrapyfy::service_parameters::validate_parameter(
        &raw.parameters[0],
        "not-an-url"
    )
    .is_err());
    let store = crate::scrapyfy::service_parameters_store::memory_parameters_store();
    let key = crate::SourceServiceKey::new("stream", &raw.id);
    use crate::scrapyfy::service_parameters_store::{
        load_parameter_values, replace_parameter_values,
    };
    replace_parameter_values(
        store.as_ref(),
        &key,
        std::collections::HashMap::from([
            ("base_url".into(), "https://changed.test".into()),
            ("obsolete".into(), "removed on next save".into()),
        ]),
    )
    .await?;
    assert!(load_parameter_values(
        store.as_ref(),
        &crate::SourceServiceKey::new("other", &raw.id)
    )
    .await?
    .is_empty());
    replace_parameter_values(
        store.as_ref(),
        &key,
        std::collections::HashMap::from([("base_url".into(), "https://changed.test".into())]),
    )
    .await?;
    let saved = load_parameter_values(store.as_ref(), &key).await?;
    assert_eq!(saved.len(), 1);
    crate::scrapyfy::service_parameters::apply_overrides(&mut raw.parameters, &saved)?;
    replace_parameter_values(store.as_ref(), &key, std::collections::HashMap::new()).await?;
    assert!(load_parameter_values(store.as_ref(), &key)
        .await?
        .is_empty());
    #[cfg(feature = "sqlite-persistence")]
    {
        use crate::scrapyfy::service_parameters_store::sqlite_parameters_store;
        use arachnea_core::persistence::{SqliteDatabase, SqliteStoreConfig};
        let dir = std::env::temp_dir().join(format!(
            "arachnea-parameter-store-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos(),
        ));
        let database = SqliteDatabase::open(SqliteStoreConfig::new("parameter-store")?, &dir)?;
        let sqlite = sqlite_parameters_store(&database)?;
        let other_key = crate::SourceServiceKey::new("other", &raw.id);
        let sibling_key = crate::SourceServiceKey::new("stream", format!("{}-sibling", raw.id));
        replace_parameter_values(
            sqlite.as_ref(),
            &key,
            std::collections::HashMap::from([
                ("base_url".into(), "https://stored.test".into()),
                ("country".into(), "ch".into()),
            ]),
        )
        .await?;
        replace_parameter_values(
            sqlite.as_ref(),
            &other_key,
            std::collections::HashMap::from([("base_url".into(), "https://other.test".into())]),
        )
        .await?;
        replace_parameter_values(
            sqlite.as_ref(),
            &sibling_key,
            std::collections::HashMap::from([("country".into(), "fr".into())]),
        )
        .await?;
        assert_eq!(load_parameter_values(sqlite.as_ref(), &key).await?.len(), 2);
        let reopened = sqlite_parameters_store(&database)?;
        assert_eq!(
            load_parameter_values(reopened.as_ref(), &key).await?["base_url"],
            "https://stored.test"
        );
        replace_parameter_values(
            sqlite.as_ref(),
            &key,
            std::collections::HashMap::from([("country".into(), "be".into())]),
        )
        .await?;
        assert_eq!(
            load_parameter_values(reopened.as_ref(), &key).await?.len(),
            1
        );
        assert_eq!(
            load_parameter_values(reopened.as_ref(), &key).await?["country"],
            "be"
        );
        replace_parameter_values(sqlite.as_ref(), &key, std::collections::HashMap::new()).await?;
        assert!(load_parameter_values(reopened.as_ref(), &key)
            .await?
            .is_empty());
        assert_eq!(
            load_parameter_values(reopened.as_ref(), &sibling_key).await?["country"],
            "fr"
        );
        drop((sqlite, reopened, database));
        let database = SqliteDatabase::open(SqliteStoreConfig::new("parameter-store")?, &dir)?;
        let restarted = sqlite_parameters_store(&database)?;
        assert!(load_parameter_values(restarted.as_ref(), &key)
            .await?
            .is_empty());
        assert_eq!(
            load_parameter_values(restarted.as_ref(), &other_key).await?["base_url"],
            "https://other.test"
        );
        drop((restarted, database));
        std::fs::remove_dir_all(dir)?;
    }
    let collection: super::ScraperQueryCollection = raw.try_into()?;
    let params = std::collections::HashMap::from([(
        crate::scrapyfy::HTTP_PROXY_PUBLIC_PATH_PARAM.to_string(),
        "/api/proxy".to_string(),
    )]);

    let rows = collection
        .execute_query("service_stream_metadata", &params, None, None)
        .await?;
    assert_eq!(
        rows[0]["metadata"]
            .to_json_value()
            .map_err(anyhow::Error::msg)?,
        serde_json::json!({"languages": ["fr"]})
    );
    let logo = rows
        .first()
        .and_then(|row| row.get("logo"))
        .and_then(|node| node.values.first())
        .cloned();

    assert_eq!(
        logo.as_deref(),
        Some("/api/proxy/https://changed.test/logo.png")
    );

    Ok(())
}
