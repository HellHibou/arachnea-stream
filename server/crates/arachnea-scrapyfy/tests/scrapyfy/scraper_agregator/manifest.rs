use super::super::test_support::unique_temp_dir;
use super::*;

#[test]
fn resolve_manifest_sources_expands_recursive_imports_in_depth_first_order() {
    let dir = unique_temp_dir();
    std::fs::create_dir_all(dir.join("dark-stream")).expect("dark stream dir should exist");
    std::fs::create_dir_all(dir.join("legal-stream")).expect("legal stream dir should exist");

    std::fs::write(
        dir.join("dark-stream/services.json"),
        r#"[
                {"path": "anime-sama.yaml", "enabled": true},
                {"path": "coflix.yaml", "enabled": false}
            ]"#,
    )
    .expect("dark-stream manifest should be written");
    std::fs::write(
        dir.join("legal-stream/services.json"),
        r#"[
                {"path": "rtlplay-be.yaml", "enabled": true}
            ]"#,
    )
    .expect("legal-stream manifest should be written");
    std::fs::write(
        dir.join("services.json"),
        r#"[
                {"import": "dark-stream/services.json"},
                {"import": "legal-stream/services.json"},
                {"path": "local.yaml", "enabled": true}
            ]"#,
    )
    .expect("root manifest should be written");

    let resolved = resolve_manifest_sources(dir.join("services.json"))
        .expect("manifest with imports should resolve");
    let paths: Vec<String> = resolved.iter().map(|s| s.path.to_string()).collect();
    let expected: Vec<String> = vec![
        "dark-stream/anime-sama.yaml",
        "dark-stream/coflix.yaml",
        "legal-stream/rtlplay-be.yaml",
        "local.yaml",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(paths, expected);
    assert!(resolved[0].enabled);
    assert!(!resolved[1].enabled);

    std::fs::remove_dir_all(dir).expect("test service directory should be removed");
}

#[test]
fn resolve_manifest_sources_skips_cyclic_imports() {
    let dir = unique_temp_dir();
    std::fs::create_dir_all(&dir).expect("test service directory should be created");
    std::fs::write(
        dir.join("a.json"),
        r#"[
                {"path": "a.yaml", "enabled": true},
                {"import": "b.json"}
            ]"#,
    )
    .expect("a manifest should be written");
    std::fs::write(
        dir.join("b.json"),
        r#"[
                {"import": "a.json"},
                {"path": "b.yaml", "enabled": true}
            ]"#,
    )
    .expect("b manifest should be written");

    let resolved = resolve_manifest_sources(dir.join("a.json"))
        .expect("cyclic manifest should still resolve without infinite recursion");
    let paths: Vec<String> = resolved.iter().map(|s| s.path.to_string()).collect();
    assert_eq!(paths, vec!["a.yaml".to_string(), "b.yaml".to_string()]);

    std::fs::remove_dir_all(dir).expect("test service directory should be removed");
}

#[test]
fn resolve_manifest_sources_rejects_ambiguous_entries() {
    let dir = unique_temp_dir();
    std::fs::create_dir_all(&dir).expect("test service directory should be created");
    std::fs::write(
        dir.join("services.json"),
        r#"[
                {"path": "a.yaml", "import": "b.json", "enabled": true}
            ]"#,
    )
    .expect("manifest should be written");

    match resolve_manifest_sources(dir.join("services.json")) {
        Ok(_) => panic!("ambiguous entry should be rejected"),
        Err(error) => {
            let message = format!("{error:#}");
            assert!(message.contains("exactly one"));
            assert!(message.contains("services.json"));
        }
    }

    std::fs::remove_dir_all(dir).expect("test service directory should be removed");
}

#[test]
fn resolve_manifest_sources_fails_on_missing_import() {
    let dir = unique_temp_dir();
    std::fs::create_dir_all(&dir).expect("test service directory should be created");
    std::fs::write(
        dir.join("services.json"),
        r#"[
                {"import": "missing/services.json"}
            ]"#,
    )
    .expect("manifest should be written");

    match resolve_manifest_sources(dir.join("services.json")) {
        Ok(_) => panic!("missing import should be an error"),
        Err(error) => {
            let message = format!("{error:#}");
            assert!(message.contains("import"));
            assert!(message.contains("missing/services.json"));
        }
    }

    std::fs::remove_dir_all(dir).expect("test service directory should be removed");
}
