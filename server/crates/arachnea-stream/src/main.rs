#![warn(missing_docs)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Arachnea backend executable: wires scraper sources and controller backends.

use anyhow::{bail, Context, Result};
use rand::{distr::Alphanumeric, Rng};
use std::net::IpAddr;
use std::sync::Arc;

use arachnea_core::{
    application,
    controler::{ApplicationMode, CoreApplicationOptions, ServerNetworkMode, DEFAULT_SERVER_PORT},
    persistence::{
        CredentialsStore, EncryptedFileCredentialsStore, FilePersistenceStore, PersistenceStore,
    },
};
use arachnea_scrapyfy::*;
use arachnea_stream::{
    admin::{dto::SettingSource, register_admin_service, AdminMode, AdminRuntimeSettings, AdminState},
    configuration::ApplicationConfiguration, ReloadableStreamScraper, StreamScraper,
    StreamScraperBuildOptions,
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

/// Default path used by the encrypted server credentials store.
const DEFAULT_ENCRYPTED_FILE_CREDENTIALS_STORE_PATH: &str = "data/credentials";

/// Static AES-256-GCM key reserved for the encrypted file store.
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
        options.application_option.entrypoint_root = configuration.entrypoint_root.clone();
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

/// Generates the one-time administrator password used until one is persisted.
fn generate_temporary_admin_password() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(24)
        .map(char::from)
        .collect()
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
/// per-source error details.
fn tray_reload_summary(reloadable: Arc<ReloadableStreamScraper>) -> String {
    match reloadable.reload_blocking() {
        Ok(report) => {
            let mut summary = format!(
                "applied={}; loaded={}; disabled={}; ignored={}; errors={}",
                report.applied,
                report.loaded.len(),
                report.disabled.len(),
                report.ignored.len(),
                report.errors.len(),
            );
            if !report.errors.is_empty() {
                let details = report
                    .errors
                    .iter()
                    .map(|error| format!("{}: {}", error.path, error.message))
                    .collect::<Vec<_>>()
                    .join("; ");
                summary.push_str(&format!("; error_details=[{details}]"));
            }
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
    // A temporary password is generated once when no permanent hash is
    // configured and kept in memory for the login operation; it is never
    // persisted and printed a single time to the console.
    let temp_password = if configuration.password_hash.is_none()
        && effective_mode == AdminMode::Server
    {
        let password = generate_temporary_admin_password();
        println!("Temporary administrator password for remote administration: {password}");
        Some(password)
    } else {
        None
    };

    // Service credentials are persisted in the encrypted store configured here;
    // the activation overrides live in the shared persistence store.
    let credentials_store: Arc<dyn CredentialsStore> = Arc::new(EncryptedFileCredentialsStore::new(
        application::get_application_data_path(DEFAULT_ENCRYPTED_FILE_CREDENTIALS_STORE_PATH),
        DEFAULT_SERVER_CREDENTIALS_KEY,
    ));
    let persistence_store: Arc<dyn PersistenceStore> =
        Arc::new(FilePersistenceStore::default_data_dir());

    let mut build_options =
        StreamScraperBuildOptions::new(credentials_store, persistence_store);
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

    let admin_runtime_settings = build_admin_runtime_settings(&options, &configuration, effective_mode);

    // Mount the administration bundle below the application root (`/admin/`),
    // in both server and desktop backends.
    options.application_option.web_mount_paths.push("admin".to_string());

    // Wire the server tray "Reload configuration" action to the reloadable
    // facade. The facade is only registered once the controller exists, so the
    // callback resolves it lazily through a shared slot filled just after.
    let late_reload: Arc<std::sync::OnceLock<Arc<ReloadableStreamScraper>>> =
        Arc::new(std::sync::OnceLock::new());
    let late_reload_callback = Arc::clone(&late_reload);
    let reload_configuration: Arc<dyn Fn() -> String + Send + Sync> = Arc::new(move || {
        match late_reload_callback.get() {
            Some(reloadable) => tray_reload_summary(Arc::clone(reloadable)),
            None => "reload unavailable: scraper not initialized yet".to_string(),
        }
    });
    options.application_option.reload_configuration = Some(reload_configuration);

    let mut controler = arachnea_core::create_application_controler!(options.application_option);
    let reloadable = reloadable.register_service(controler.as_mut());
    let _ = late_reload.set(Arc::clone(&reloadable));

    let admin_state = Arc::new(AdminState::new(
        admin_runtime_settings,
        configuration,
        temp_password,
        reloadable,
    ));
    register_admin_service(&admin_state, controler.as_mut());
    controler.launch();

    Ok(())
}
