#![warn(missing_docs)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Arachnea backend executable: wires scraper sources and controller backends.

use anyhow::{bail, Context, Result};
use std::net::IpAddr;
use std::sync::Arc;

use arachnea_core::{
    application::{self, ApplicationOptionsProvider},
    controler::{
        options::{ApplicationMode, CoreApplicationOptions, DEFAULT_SERVER_PORT},
    },
    persistence::CredentialsStore,
};
use arachnea_scrapyfy::admin::{register_admin_service, ReloadCoordinator};
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

    // The controller macro treats a `None` application mode as server mode.
    let effective_mode = options
        .application_option
        .application_mode
        .unwrap_or_default();

    let build_options = StreamScraperBuildOptions::new(credentials_store, stores.clone());
    let runtime_options = ScraperRuntimeOptions {
        cache_config: options
            .scraper_cache_config()
            .context("Invalid cache sizing command-line options")?,
    };

    let reloadable = ReloadableStreamScraper::new(build_options, runtime_options)?;
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

    // Wire the server tray "Reload configuration" action to Scrapyfy's shared
    // coordinator. The coordinator exists only after the administration service
    // has been registered, so the callback resolves it through a late slot.
    let late_reload: Arc<std::sync::OnceLock<Arc<ReloadCoordinator>>> =
        Arc::new(std::sync::OnceLock::new());
    let late_reload_callback = Arc::clone(&late_reload);
    let reload_configuration: Arc<dyn Fn() -> String + Send + Sync> =
        Arc::new(move || match late_reload_callback.get() {
            Some(coordinator) => {
                match coordinator.reload_all_and_apply_server_settings_blocking() {
                    Ok(summary) => summary.log_summary(),
                    Err(error) => format!("Reload failed: {error}"),
                }
            }
            None => "Reload unavailable: administration not initialized yet".to_string(),
        });
    options.application_option.reload_configuration = Some(reload_configuration);

    // Command-line overrides of the dynamic server settings, captured before
    // the application options are moved into the controller constructor.
    let cli_options = options.application_option.clone();
    // Command-line-pinned scraper overrides, captured before the application
    // options field is moved into the controller constructor.
    let cli_scraper = options.cli_scraper_settings().clone();

    let mut controler = arachnea_core::create_application_controler!(options.application_option);
    let reloadable = reloadable.register_service(controler.as_mut());

    // Hot application of server settings is owned by Core's REST supervisor;
    // Core's factory combines the Stream-owned persisted configuration reader
    // with the command-line overrides.
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
        let persisted: Arc<dyn Fn() -> Option<CoreApplicationOptions> + Send + Sync> =
            Arc::new(move || configuration.read().ok().map(|config| config.clone()));
        rest_server.set_settings_source(arachnea_core::controler::rest_settings_source(
            persisted,
            cli_options,
        ));
    }

    let active_scraper = reloadable.current();
    let admin_state = register_admin_service(
        admin_runtime_settings,
        active_scraper.scraper_agregator(),
        Arc::clone(&stores.source_enabled),
        adapter,
        cli_scraper,
        controler.as_mut(),
    )?;
    let _ = late_reload.set(admin_state.reload_coordinator);

    controler.launch();

    Ok(())
}
