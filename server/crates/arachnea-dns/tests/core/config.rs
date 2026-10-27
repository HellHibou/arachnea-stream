use crate::core::*;

/// Verifies that the resilience profile falls back after NXDOMAIN responses.
#[test]
fn profile_resilience_falls_back_on_nxdomain() {
    let config = UsageProfile::Resilience(Vec::new()).config();

    assert_eq!(config.resolver.mode, ResolutionMode::Recursive);
    assert_eq!(
        config.resolver.nxdomain_strategy,
        NxdomainStrategy::FallbackOnNxdomain
    );
}

/// Verifies that embedded root hints are loaded by default.
#[test]
fn embedded_root_hints_are_loaded_by_default() {
    let config = UsageProfile::SystemRelay
        .config()
        .resolve_effective_config()
        .unwrap();

    assert!(config
        .config
        .resolver
        .root_hints
        .contains(&"198.41.0.4".to_owned()));
    assert!(config
        .config
        .resolver
        .root_hints
        .contains(&"2001:503:ba3e::2:30".to_owned()));
}

/// Verifies that configured root hints override embedded defaults.
#[test]
fn direct_root_hints_override_embedded_defaults() {
    let mut config = UsageProfile::SystemRelay.config();
    config.resolver.root_hints = vec!["203.0.113.53".to_owned()];

    let effective = config.resolve_effective_config().unwrap();

    assert_eq!(effective.config.resolver.root_hints, vec!["203.0.113.53"]);
}

/// Verifies that root hints can be loaded from an external file.
#[test]
fn root_hints_can_be_loaded_from_external_file() {
    let path = std::env::temp_dir().join(format!("arachnea-root-hints-{}.txt", std::process::id()));
    std::fs::write(&path, "# test root\n192.0.2.53\n2001:db8::53\n").unwrap();

    let mut config = UsageProfile::SystemRelay.config();
    config.resolver.root_hints_file = Some(path.clone());

    let effective = config.resolve_effective_config().unwrap();
    let _ = std::fs::remove_file(path);

    assert_eq!(
        effective.config.resolver.root_hints,
        vec!["192.0.2.53", "2001:db8::53"]
    );
}
