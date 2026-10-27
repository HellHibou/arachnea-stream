use super::*;

/// Builds options with explicit port, network and root values/provenances.
fn options_with(
    port: Option<u16>,
    port_source: SettingSource,
    network: Option<ServerNetworkMode>,
    network_source: SettingSource,
    root: Option<&str>,
    root_source: SettingSource,
) -> CoreApplicationOptions {
    let mut options =
        CoreApplicationOptions::new(None, port, root.map(str::to_string), None, None, None);
    options.server_port_source = port_source;
    options.network_mode = network;
    options.network_mode_source = network_source;
    options.entrypoint_root_source = root_source;
    options
}

fn reader_of(
    port: Option<u16>,
    network: Option<ServerNetworkMode>,
    root: Option<&str>,
) -> Arc<dyn Fn() -> Option<CoreApplicationOptions> + Send + Sync> {
    let mut persisted =
        CoreApplicationOptions::new(None, port, root.map(str::to_string), None, None, None);
    persisted.network_mode = network;
    Arc::new(move || Some(persisted.clone()))
}

#[test]
fn command_line_pins_win_over_persisted_configuration() {
    let cli = options_with(
        Some(9000),
        SettingSource::CommandLine,
        Some(ServerNetworkMode::Public),
        SettingSource::CommandLine,
        Some("/cli-root"),
        SettingSource::CommandLine,
    );
    let reader = reader_of(
        Some(1234),
        Some(ServerNetworkMode::Local),
        Some("/config-root"),
    );
    let settings = rest_settings_source(reader, cli)().expect("settings must resolve");
    assert_eq!(settings.server_port, 9000);
    assert_eq!(settings.network_mode, ServerNetworkMode::Public);
    assert_eq!(settings.entrypoint_root.as_deref(), Some("/cli-root"));
}

#[test]
fn persisted_configuration_wins_over_defaults() {
    let cli = options_with(
        None,
        SettingSource::Default,
        None,
        SettingSource::Default,
        None,
        SettingSource::Default,
    );
    let reader = reader_of(
        Some(1234),
        Some(ServerNetworkMode::Local),
        Some("/config-root"),
    );
    let settings = rest_settings_source(reader, cli)().expect("settings must resolve");
    assert_eq!(settings.server_port, 1234);
    assert_eq!(settings.network_mode, ServerNetworkMode::Local);
    assert_eq!(settings.entrypoint_root.as_deref(), Some("/config-root"));
}

#[test]
fn defaults_apply_when_neither_cli_nor_configuration_provide_values() {
    let cli = options_with(
        None,
        SettingSource::Default,
        None,
        SettingSource::Default,
        None,
        SettingSource::Default,
    );
    let reader = reader_of(None, None, None);
    let settings = rest_settings_source(reader, cli)().expect("settings must resolve");
    assert_eq!(settings.server_port, DEFAULT_SERVER_PORT);
    assert_eq!(settings.network_mode, ServerNetworkMode::Private);
    assert_eq!(settings.entrypoint_root, None);
}

#[test]
fn configuration_source_values_are_not_pinned() {
    // Values coming from the configuration file (not the command line)
    // must never block a later persisted update through the admin.
    let cli = options_with(
        Some(9000),
        SettingSource::Configuration,
        Some(ServerNetworkMode::Public),
        SettingSource::Configuration,
        Some("/config-root"),
        SettingSource::Configuration,
    );
    let reader = reader_of(
        Some(1234),
        Some(ServerNetworkMode::Local),
        Some("/updated-root"),
    );
    let settings = rest_settings_source(reader, cli)().expect("settings must resolve");
    assert_eq!(settings.server_port, 1234);
    assert_eq!(settings.network_mode, ServerNetworkMode::Local);
    assert_eq!(settings.entrypoint_root.as_deref(), Some("/updated-root"));
}

#[test]
fn pinned_port_survives_persisted_update_while_other_settings_follow_config() {
    let cli = options_with(
        Some(9000),
        SettingSource::CommandLine,
        None,
        SettingSource::Default,
        None,
        SettingSource::Default,
    );
    // Simulates an administration update of the persisted configuration.
    let mut persisted = CoreApplicationOptions::new(None, Some(1234), None, None, None, None);
    persisted.network_mode = Some(ServerNetworkMode::Local);
    let reader: Arc<dyn Fn() -> Option<CoreApplicationOptions> + Send + Sync> =
        Arc::new(move || Some(persisted.clone()));
    let settings = rest_settings_source(reader, cli)().expect("settings must resolve");
    assert_eq!(settings.server_port, 9000);
    assert_eq!(settings.network_mode, ServerNetworkMode::Local);
}

#[test]
fn unavailable_persisted_configuration_disables_hot_application() {
    let cli = options_with(
        None,
        SettingSource::Default,
        None,
        SettingSource::Default,
        None,
        SettingSource::Default,
    );
    let reader: Arc<dyn Fn() -> Option<CoreApplicationOptions> + Send + Sync> = Arc::new(|| None);
    assert!(rest_settings_source(reader, cli)().is_none());
}
