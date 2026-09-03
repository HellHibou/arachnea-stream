#![warn(missing_docs)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Arachnea backend executable: wires scraper sources and controller backends.

use anyhow::{bail, Context, Result};
use std::net::IpAddr;
use std::sync::Arc;

use arachnea_core::{
    application,
    controler::{
        ApplicationMode, CoreApplicationOptions, RestServerSettings, ServerNetworkMode,
        DEFAULT_SERVER_PORT,
    },
    persistence::CredentialsStore,
};
use arachnea_scrapyfy::admin::{
    dto::SettingSource, register_admin_service, AdminMode, AdminRuntimeSettings,
    AdminServiceGroupConfig, AdminState,
};
use arachnea_scrapyfy::*;
use arachnea_stream::{
    admin_composition::StreamAdminRuntimeAdapter, configuration::ApplicationConfiguration,
    ReloadableStreamScraper, StreamScraper, StreamScraperBuildOptions,
    TypedServiceCredentialsStore,
};

/// Runtime options parsed from command line arguments.
struct RuntimeOptions {
    application_option: CoreApplicationOptions,
    current_country: Option<String>,
    cache_max_disk_bytes: Option<u64>,
    cache_max_memory_bytes: Option<u64>,
    server_port_specified: bool,
    network_mode_specified: bool,
    entrypoint_root_specified: bool,
}

/// Static AES-256-GCM key used to encrypt service credentials stored in the
/// `arachnea-services` persistence store.
const DEFAULT_SERVER_CREDENTIALS_KEY: [u8; 32] = *b"hell_hibou-arachnea-key-20260422";

/// Result of parsing command line arguments.
enum CliAction {
    Run(RuntimeOptions),
    RefreshIpCountries,
    ExitSuccess,
}

/// Prints the command line help in English.
fn help_message() -> String {
    let program_name = application::program_name();
    format!(
        "Usage: {program_name} [OPTIONS]

Options:
  --help                        Show this help message and exit
  --desktop                     Run in desktop application mode (forces mode_server to false)
  --server                      Run in server mode (forces mode_server to true)
  --server-port <PORT>          Override the server port (default: {DEFAULT_SERVER_PORT})
  --network <MODE>              Client access rule: local, private or public (default: private)
  --entrypoint-root <PATH>      Public root path used before API routes in server mode
  --no-tray                     Disable the server tray icon even when a GUI is available
  --current-country <ISO_CODE>  Explicit local country used for geo proxy decisions
  --cache-max-disk-bytes <BYTES>  Override the maximum on-disk size of the server cache (default: 100 MiB)
  --cache-max-memory-bytes <BYTES>  Override the maximum in-memory size of the server cache (default: 32 MiB)
  --refresh-ip-countries        Refresh IP-to-country geolocation data and exit"
    )
}

/// Parses supported command line arguments for the backend executable.
///
/// Supported flags:
/// - `--desktop`: forces desktop mode
/// - `--server`: forces server mode
/// - `--server-port <port>`: overrides the REST server port
/// - `--network <local|private|public>`: sets which client connections the
///   server accepts. `local` binds to loopback only, `private` binds to all
///   interfaces and rejects clients outside the local network ranges, and
///   `public` binds to all interfaces and accepts every client. Defaults to
///   `private`.
/// - `--current-country <ISO_CODE>`: sets the explicit local country
/// - `--cache-max-disk-bytes <BYTES>`: overrides the server cache on-disk size
/// - `--cache-max-memory-bytes <BYTES>`: overrides the server cache in-memory size
/// - `--help`: prints help and exits successfully
/// - macOS Finder process serial number arguments (`-psn_...`) are ignored
///
/// When both `--desktop` and `--server` are provided, the last one wins.
///
/// # Errors
/// Returns an error when an argument is unknown, when `--server-port` is missing
/// its value, or when the provided port is invalid.
fn parse_runtime_options() -> Result<CliAction> {
    let mut application_option = CoreApplicationOptions::default();
    application_option.web_scheme = Some("arachnea-stream".to_string());
    let mut options = RuntimeOptions {
        application_option,
        current_country: None,
        cache_max_disk_bytes: None,
        cache_max_memory_bytes: None,
        server_port_specified: false,
        network_mode_specified: false,
        entrypoint_root_specified: false,
    };

    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--desktop" => {
                options.application_option.application_mode = Some(ApplicationMode::Desktop)
            }
            "--server" => {
                options.application_option.application_mode = Some(ApplicationMode::Server)
            }
            "--help" => {
                println!("{}", help_message());
                return Ok(CliAction::ExitSuccess);
            }
            "--refresh-ip-countries" => {
                return Ok(CliAction::RefreshIpCountries);
            }
            "--server-port" => {
                let port = args.next().context("missing value for `--server-port`")?;

                options.application_option.server_port = Some(
                    port.parse::<u16>()
                        .with_context(|| format!("invalid value for `--server-port`: `{port}`"))?,
                );
                options.server_port_specified = true;
            }
            "--network" => {
                let value = args.next().context("missing value for `--network`")?;
                options.application_option.network_mode = value
                    .parse::<ServerNetworkMode>()
                    .map_err(|error| anyhow::anyhow!("invalid value for `--network`: {error}"))?;
                options.network_mode_specified = true;
            }
            "--entrypoint-root" => {
                options.application_option.entrypoint_root = Some(
                    args.next()
                        .context("missing value for `--entrypoint-root`")?,
                );
                options.entrypoint_root_specified = true;
            }
            "--no-tray" => options.application_option.tray_enabled = false,
            "--current-country" => {
                options.current_country = Some(
                    args.next()
                        .context("missing value for `--current-country`")?,
                );
            }
            "--cache-max-disk-bytes" => {
                let value = args
                    .next()
                    .context("missing value for `--cache-max-disk-bytes`")?;

                options.cache_max_disk_bytes = Some(value.parse::<u64>().with_context(|| {
                    format!("invalid value for `--cache-max-disk-bytes`: `{value}`")
                })?);
            }
            "--cache-max-memory-bytes" => {
                let value = args
                    .next()
                    .context("missing value for `--cache-max-memory-bytes`")?;

                options.cache_max_memory_bytes = Some(value.parse::<u64>().with_context(|| {
                    format!("invalid value for `--cache-max-memory-bytes`: `{value}`")
                })?);
            }
            // Finder passes this process serial number argument when opening a
            // macOS application bundle. It is not an application option.
            arg if cfg!(target_os = "macos") && arg.starts_with("-psn_") => {}
            _ => bail!("unknown argument: `{arg}`"),
        }
    }

    Ok(CliAction::Run(options))
}

/// Applies persisted values when the equivalent command-line option is absent.
fn apply_persistent_configuration(
    options: &mut RuntimeOptions,
    configuration: &ApplicationConfiguration,
) -> Result<()> {
    if !options.server_port_specified {
        options.application_option.server_port = configuration.server_port;
    }
    if !options.entrypoint_root_specified {
        // An empty persisted root means "no prefix": normalize it to `None` so
        // the effective settings and the hot-apply target agree.
        options.application_option.entrypoint_root = configuration
            .entrypoint_root
            .as_deref()
            .filter(|root| !root.is_empty())
            .map(ToString::to_string);
    }
    if !options.network_mode_specified {
        if let Some(network_mode) = configuration.network_mode.as_deref() {
            options.application_option.network_mode = network_mode
                .parse::<ServerNetworkMode>()
                .map_err(|error| anyhow::anyhow!("invalid configured network mode: {error}"))?;
        }
    }
    Ok(())
}

/// Builds the effective administration settings from CLI options and the
/// persisted configuration, with the provenance of each value.
fn build_admin_runtime_settings(
    options: &RuntimeOptions,
    configuration: &ApplicationConfiguration,
    mode: AdminMode,
) -> AdminRuntimeSettings {
    AdminRuntimeSettings {
        mode,
        server_port: options
            .application_option
            .server_port
            .unwrap_or(DEFAULT_SERVER_PORT),
        server_port_source: if options.server_port_specified {
            SettingSource::CommandLine
        } else if configuration.server_port.is_some() {
            SettingSource::Configuration
        } else {
            SettingSource::Default
        },
        network_mode: match options.application_option.network_mode {
            ServerNetworkMode::Local => "local".to_string(),
            ServerNetworkMode::Private => "private".to_string(),
            ServerNetworkMode::Public => "public".to_string(),
        },
        network_mode_source: if options.network_mode_specified {
            SettingSource::CommandLine
        } else if configuration.network_mode.is_some() {
            SettingSource::Configuration
        } else {
            SettingSource::Default
        },
        entrypoint_root: options.application_option.entrypoint_root.clone(),
        entrypoint_root_source: if options.entrypoint_root_specified {
            SettingSource::CommandLine
        } else if configuration.entrypoint_root.is_some() {
            SettingSource::Configuration
        } else {
            SettingSource::Default
        },
    }
}

/// Builds a compact, log-friendly summary of a configuration reload.
///
/// Used by the server tray "Reload configuration" action so reload problems
/// stay diagnosable from the log window and the administration interface.
///
/// # Arguments
/// * `reloadable` - Reloadable scraper facade to reload.
///
/// # Returns
/// A single-line summary reporting the outcome and, when applicable, the
/// build failure context.
fn tray_reload_summary(reloadable: Arc<ReloadableStreamScraper>) -> String {
    match reloadable.reload_blocking() {
        Ok(report) => {
            let mut summary = format!("applied={}", report.applied);
            if let Some(build_error) = &report.build_error {
                summary.push_str(&format!("; build_error={build_error}"));
            }
            summary
        }
        Err(error) => format!("reload failed: {error}"),
    }
}

/// Runs the IP-country geolocation refresh and exits.
///
/// Initialises the scraper aggregator, collects proxy IPs from the dynamic
/// proxy inventory, resolves each IP to a country via the YAML-defined
/// ip-api.com query, and persists the results to `data/ip-countries.json`.
async fn refresh_ip_countries_cli() -> Result<()> {
    let mut manager = StreamScraper::from_json(None)?;
    let agregator = manager.get_scraper_agregator_mut();

    // Collect IPs from the proxy inventory if available
    let ip_addresses: Vec<IpAddr> = {
        if let Some(proxy_core) = agregator.proxy_core() {
            let inventory = proxy_core.proxy_inventory();
            let records = match inventory {
                Some(inv) => inv.all_records().await,
                None => {
                    tracing::warn!("no proxy inventory available; refresh will be a no-op");
                    Vec::new()
                }
            };
            let ips: Vec<IpAddr> = records
                .iter()
                .filter_map(|record| record.host.parse::<IpAddr>().ok())
                .collect();
            if !ips.is_empty() {
                ips
            } else {
                tracing::warn!("no proxy IPs found in inventory; refresh will be a no-op");
                Vec::new()
            }
        } else {
            tracing::warn!("no proxy core available; refresh will be a no-op");
            Vec::new()
        }
    };

    let config = IpCountryRefreshConfig::default();
    refresh_ip_country_store(agregator, &ip_addresses, &config).await?;

    Ok(())
}

/// Starts the configured backend controller using command line runtime options.
#[tokio::main]
async fn main() -> Result<()> {
    application::configure_application_data_dir_name(env!("ARACHNEA_TAURI_IDENTIFIER"))
        .map_err(anyhow::Error::msg)?;
    arachnea_core::application::application_init();
    arachnea_core::logger::set_default_log_level_debug!(INFO);
    StreamScraper::init_sub_logger_levels();
    arachnea_core::logger::init_logger();

    let mut options = match parse_runtime_options() {
        Ok(CliAction::Run(options)) => options,
        Ok(CliAction::RefreshIpCountries) => {
            return refresh_ip_countries_cli().await;
        }
        Ok(CliAction::ExitSuccess) => return Ok(()),
        Err(error) => {
            eprintln!("Error: {error}\n");
            eprintln!("{}", help_message());
            return Err(error);
        }
    };

    let configuration = ApplicationConfiguration::load()?;
    apply_persistent_configuration(&mut options, &configuration)?;
    // The controller macro treats a `None` application mode as server mode.
    let effective_mode = if matches!(
        options.application_option.application_mode,
        Some(ApplicationMode::Desktop)
    ) {
        AdminMode::Desktop
    } else {
        AdminMode::Server
    };
    // Service credentials are stored encrypted (per-field AES-256-GCM) in the
    // typed `arachnea-services` SQLite store; the activation overrides,
    // Cloudflare sessions and proxy inventory live in the same typed stores.
    let stores = arachnea_stream::stream_scraper::sqlite_application_stores(
        &application::get_application_data_path(""),
    )?;
    let typed_credentials_store = Arc::new(TypedServiceCredentialsStore::new(
        Arc::clone(&stores.source_enabled),
        DEFAULT_SERVER_CREDENTIALS_KEY,
        arachnea_stream::stream_scraper::STREAM_SERVICE_GROUP_NAME,
    ));
    let credentials_store: Arc<dyn CredentialsStore> = typed_credentials_store.clone();

    let mut build_options = StreamScraperBuildOptions::new(credentials_store, stores.clone());
    if options.cache_max_disk_bytes.is_some() || options.cache_max_memory_bytes.is_some() {
        let mut cache_config = ScraperCacheConfig::default();
        if let Some(bytes) = options.cache_max_disk_bytes {
            cache_config.max_disk_bytes = bytes;
        }
        if let Some(bytes) = options.cache_max_memory_bytes {
            cache_config.max_memory_bytes = bytes;
        }
        build_options = build_options.with_cache_config(cache_config);
    }

    let reloadable = ReloadableStreamScraper::new(build_options)?;
    if let Some(current_country) = &options.current_country {
        reloadable.set_current_country(current_country).await;
    }

    let admin_runtime_settings =
        build_admin_runtime_settings(&options, &configuration, effective_mode);

    // Mount the administration bundle below the application root (`/admin/`),
    // in both server and desktop backends.
    options
        .application_option
        .web_mount_paths
        .push("admin".to_string());

    // Wire the server tray "Reload configuration" action to the reloadable
    // facade. The facade is only registered once the controller exists, so the
    // callback resolves it lazily through a shared slot filled just after.
    let late_reload: Arc<std::sync::OnceLock<Arc<ReloadableStreamScraper>>> =
        Arc::new(std::sync::OnceLock::new());
    let late_reload_callback = Arc::clone(&late_reload);
    let reload_configuration: Arc<dyn Fn() -> String + Send + Sync> =
        Arc::new(move || match late_reload_callback.get() {
            Some(reloadable) => tray_reload_summary(Arc::clone(reloadable)),
            None => "reload unavailable: scraper not initialized yet".to_string(),
        });
    options.application_option.reload_configuration = Some(reload_configuration);

    // Command-line overrides of the dynamic server settings, captured before
    // the application options are moved into the controller constructor.
    let cli_port = options.application_option.server_port;
    let port_pinned = options.server_port_specified;
    let cli_network = options.application_option.network_mode;
    let network_pinned = options.network_mode_specified;
    let cli_root = options.application_option.entrypoint_root.clone();
    let root_pinned = options.entrypoint_root_specified;

    let mut controler = arachnea_core::create_application_controler!(options.application_option);
    let reloadable = reloadable.register_service(controler.as_mut());
    let _ = late_reload.set(Arc::clone(&reloadable));

    // Hot application of server settings is still performed by Core's REST
    // supervisor; the configuration source is owned by the Stream adapter.
    let rest_server_handle = controler.rest_server_handle();
    let adapter = Arc::new(StreamAdminRuntimeAdapter::new(
        configuration.clone(),
        typed_credentials_store,
        Arc::clone(&reloadable),
        rest_server_handle.clone(),
    ));
    if let Some(rest_server) = &rest_server_handle {
        let configuration = adapter.configuration();
        rest_server.set_settings_source(Arc::new(move || {
            let config = configuration.read().ok()?.clone();
            let server_port = if port_pinned {
                cli_port?
            } else {
                config.server_port.unwrap_or(DEFAULT_SERVER_PORT)
            };
            let network_mode = if network_pinned {
                cli_network
            } else {
                match config.network_mode.as_deref() {
                    Some("local") => ServerNetworkMode::Local,
                    Some("public") => ServerNetworkMode::Public,
                    _ => ServerNetworkMode::Private,
                }
            };
            let entrypoint_root = if root_pinned {
                cli_root.clone()
            } else {
                config.entrypoint_root.clone()
            };
            Some(RestServerSettings {
                server_port,
                network_mode,
                entrypoint_root,
            })
        }));
    }

    let admin_state = AdminState::build(
        admin_runtime_settings,
        vec![
            AdminServiceGroupConfig {
                service_store_id: "arachnea-stream".to_string(),
                manifest_path: "services/arachnea-stream/services.json".to_string(),
            },
            AdminServiceGroupConfig {
                service_store_id: "arachnea-stream-hoster".to_string(),
                manifest_path: "services/arachnea-stream-hoster/services.json".to_string(),
            },
            AdminServiceGroupConfig {
                service_store_id: "arachnea-proxies".to_string(),
                manifest_path: "services/arachnea-proxies/services.json".to_string(),
            },
            AdminServiceGroupConfig {
                service_store_id: "arachnea-ip-countries".to_string(),
                manifest_path: "services/arachnea-ip-countries/services.json".to_string(),
            },
        ],
        Arc::clone(&stores.source_enabled),
        adapter,
    )?;
    if let Some(password) = &admin_state.temporary_password {
        println!("Temporary administrator password for remote administration: {password}");
    }
    let admin_state = admin_state.state;
    register_admin_service(&admin_state, controler.as_mut());
    controler.launch();

    Ok(())
}
