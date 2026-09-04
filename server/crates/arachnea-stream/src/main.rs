#![warn(missing_docs)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Arachnea backend executable: wires scraper sources and controller backends.

use anyhow::{bail, Result};
use std::net::IpAddr;
use std::sync::Arc;

use arachnea_core::{
    application::{self, ApplicationOptionsProvider},
    controler::{
        options::{ApplicationMode, CoreApplicationOptions, SettingSource, DEFAULT_SERVER_PORT},
        RestServerSettings,
    },
    persistence::CredentialsStore,
};
use arachnea_scrapyfy::admin::register_admin_service;
use arachnea_scrapyfy::*;
use arachnea_stream::{
    admin_composition::StreamAdminRuntimeAdapter, ReloadableStreamScraper, StreamScraper,
    StreamScraperBuildOptions, TypedServiceCredentialsStore,
};

/// Static AES-256-GCM key used to encrypt service credentials stored in the
/// `arachnea-services` persistence store.
const DEFAULT_SERVER_CREDENTIALS_KEY: [u8; 32] = *b"hell_hibou-arachnea-key-20260422";

/// Relative configuration path in the writable application data directory.
const CONFIGURATION_PATH: &str = "data/config.json";

/// Returns the absolute persisted application configuration path.
fn configuration_path() -> std::path::PathBuf {
    std::path::PathBuf::from(application::get_application_data_path(CONFIGURATION_PATH))
}

/// Result of parsing command line arguments.
enum CliAction {
    Run {
        options: SrcapyfyApplicationOptions,
        configuration: CoreApplicationOptions,
    },
    RefreshIpCountries,
    ExitSuccess,
}

/// Builds the command-line help message in English.
///
/// # Arguments
///
/// * `aop` - Option provider supplying the core options.
///
/// # Returns
///
/// The formatted help message, extended with the executable-specific options.
fn help_message(aop: Box<dyn ApplicationOptionsProvider>) -> String {
    format!(
        "{}{}",
        application::get_application_help_message(vec![aop]),
        "
  --refresh-ip-countries    Refresh IP-to-country geolocation data and exit"
    )
}

/// Parses supported command line arguments for the backend executable.
///
/// # Errors
/// Returns an error when an argument is unknown, when `--server-port` is missing
/// its value, or when the provided port is invalid.
fn parse_runtime_options() -> Result<CliAction> {
    let persisted_options = SrcapyfyApplicationOptions::default().load_file(configuration_path())?;
    let configuration = persisted_options.application_option.clone();
    let options = persisted_options
        .load_args()?
        .with_web_scheme("arachnea-stream");

    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => {
                println!("{}", help_message(Box::new(options)));
                return Ok(CliAction::ExitSuccess);
            }
            "--refresh-ip-countries" => {
                return Ok(CliAction::RefreshIpCountries);
            }
            // Finder passes this process serial number argument when opening a
            // macOS application bundle. It is not an application option.
            arg if options.application_option.contains_option(arg.to_string()) => {}
            _ => bail!("unknown argument: `{arg}`"),
        }
    }

    Ok(CliAction::Run {
        options,
        configuration,
    })
}

/// Builds the effective application options reported by the administration API.
fn build_admin_runtime_settings(
    options: &SrcapyfyApplicationOptions,
    mode: ApplicationMode,
) -> CoreApplicationOptions {
    let mut settings = options.application_option.clone();
    settings.application_mode = Some(mode);
    settings.server_port = Some(settings.server_port.unwrap_or(DEFAULT_SERVER_PORT));
    settings
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

    let (mut options, configuration) = match parse_runtime_options() {
        Ok(CliAction::Run {
            options,
            configuration,
        }) => (options, configuration),
        Ok(CliAction::RefreshIpCountries) => {
            return refresh_ip_countries_cli().await;
        }
        Ok(CliAction::ExitSuccess) => return Ok(()),
        Err(error) => {
            eprintln!("Error: {error}\n");
            eprintln!(
                "{}",
                help_message(Box::new(CoreApplicationOptions::default()))
            );
            return Err(error);
        }
    };

    // The controller macro treats a `None` application mode as server mode.
    let effective_mode = options
        .application_option
        .application_mode
        .unwrap_or_default();
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

    let admin_runtime_settings = build_admin_runtime_settings(&options, effective_mode);

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
    let port_pinned = options.application_option.server_port_source == SettingSource::CommandLine;
    let cli_network = options.application_option.network_mode;
    let network_pinned =
        options.application_option.network_mode_source == SettingSource::CommandLine;
    let cli_root = options.application_option.entrypoint_root.clone();
    let root_pinned =
        options.application_option.entrypoint_root_source == SettingSource::CommandLine;

    let mut controler = arachnea_core::create_application_controler!(options.application_option);
    let reloadable = reloadable.register_service(controler.as_mut());
    let _ = late_reload.set(Arc::clone(&reloadable));

    // Hot application of server settings is still performed by Core's REST
    // supervisor; the configuration source is owned by the Stream adapter.
    let rest_server_handle = controler.rest_server_handle();
    let adapter = Arc::new(StreamAdminRuntimeAdapter::new(
        configuration.clone(),
        configuration_path(),
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
                config.network_mode
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

    let active_scraper = reloadable.current();
    register_admin_service(
        admin_runtime_settings,
        active_scraper.scraper_agregator(),
        Arc::clone(&stores.source_enabled),
        adapter,
        controler.as_mut(),
    )?;

    controler.launch();

    Ok(())
}
