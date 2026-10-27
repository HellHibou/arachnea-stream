use super::*;

/// Verifies parsing of the single-forwarder sample profile.
#[test]
fn parses_single_forwarder_profile() {
    let config: FileConfig = toml::from_str(
        r#"
profile = "single_forwarder"

[server]
enabled = true
listen_udp = "127.0.0.1:5353"
listen_tcp = "127.0.0.1:5353"

[forwarder]
transport = "udp"
endpoint = "1.1.1.1:53"
timeout_ms = 2000
"#,
    )
    .unwrap();

    let core_config = config.into_core_config().unwrap();

    assert_eq!(core_config.upstreams.len(), 1);
    assert_eq!(
        core_config.resolver.mode,
        crate::core::ResolutionMode::Forwarder
    );
}

/// Verifies parsing of local record configuration entries.
#[test]
fn parses_local_records() {
    let config: FileConfig = toml::from_str(
        r#"
profile = "system_relay"

[[local_records]]
name = "nas.home"
records = ["A 192.168.1.20 300"]
"#,
    )
    .unwrap();

    let core_config = config.into_core_config().unwrap();

    assert_eq!(core_config.local_records.len(), 1);
    assert_eq!(core_config.local_records[0].records[0].ttl, 300);
}
