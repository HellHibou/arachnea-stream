#![warn(missing_docs)]
//! Arachnea backend executable: wires scraper sources and controller backends.

use anyhow::{bail, Context, Result};
use std::net::IpAddr;
use std::sync::Arc;

use arachnea_core::{
    application,
    controler::{
        rest::{RestControlerConfiguration, RestControlerService},
        tauri::{TauriControlerConfiguration, TauriControlerService, TauriEmbeddedWebAssets},
        ControlerService, SharedWebAssets,
    },
    persistence::EncryptedFileCredentialsStore,
};
use arachnea_scrapyfy::*;
use arachnea_stream::StreamScraper;



#[derive(PartialEq)]
enum ApplicationMode {
    Desktop,
    Server,
}


/// Default port used by the REST controller when no CLI override is provided.
const DEFAULT_SERVER_PORT: u16 = 8080;

/// Default runtime mode used when no CLI mode flag is provided.
#[cfg(not(debug_assertions))] // Release mode defaults to server
const DEFAULT_MODE_SERVER: ApplicationMode = Desktop;
#[cfg(debug_assertions)] // Debug mode defaults.
const DEFAULT_MODE_SERVER: ApplicationMode = ApplicationMode::Server;

/// Default path used by the encrypted server credentials store.
const DEFAULT_ENCRYPTED_FILE_CREDENTIALS_STORE_PATH: &str = "data/credentials";

/// Custom URI scheme used by the desktop frontend.
const TAURI_WEB_SCHEME: &str = "arachnea-stream";

/// API prefix used by the desktop binary stream routes.
const TAURI_API_PREFIX: &str = "/api/";

/// Static AES-256-GCM key reserved for the encrypted file store.
const DEFAULT_SERVER_CREDENTIALS_KEY: [u8; 32] = *b"hell_hibou-arachnea-key-20260422";


/// Runtime options parsed from command line arguments.
struct RuntimeOptions {
    application_mode: ApplicationMode,
    server_port: u16,
    entrypoint_root: Option<String>,
    entrypoint_api: Option<String>,
    current_country: Option<String>,
}

/// Result of parsing command line arguments.
enum CliAction {
    Run(RuntimeOptions),
    RefreshIpCountries,
    ExitSuccess,
}



/// Returns the command line help in English.
fn help_message(program_name: &str) -> String {
    format!(
        "Usage: {program_name} [OPTIONS]

Options:
  --help                   Show this help message and exit
  --desktop                Run in desktop application mode (forces mode_server to false)
  --server                 Run in server mode (forces mode_server to true)
  --server-port <PORT>     Override the server port (default: {DEFAULT_SERVER_PORT})
  --entrypoint-root <PATH>
                           Public root path used before API routes in server mode
  --entrypoint-api <PATH>
                           Public API path segment used in server mode
  --current-country <ISO_CODE>
                           Explicit local country used for geo proxy decisions
  --refresh-ip-countries   Refresh IP-to-country geolocation data and exit"
    )
}

/// Prints the command line help in English.
fn print_help(program_name: &str) {
    println!("{}", help_message(program_name));
}

fn generated_tauri_context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!()
}

fn generated_embedded_web_assets() -> SharedWebAssets {
    let assets: Arc<dyn tauri::Assets<tauri::Wry>> = Arc::from(generated_tauri_context().assets);
    Arc::new(TauriEmbeddedWebAssets::new(assets))
}

fn tauri_controler_service() -> TauriControlerService {
    TauriControlerService::with_configuration(
        TauriControlerConfiguration::new(generated_tauri_context())
            .web_scheme(TAURI_WEB_SCHEME)
            .api_prefix(TAURI_API_PREFIX),
    )
}

/// Parses supported command line arguments for the backend executable.
///
/// Supported flags:
/// - `--desktop`: forces desktop mode
/// - `--server`: forces server mode
/// - `--server-port <port>`: overrides the REST server port
/// - `--current-country <ISO_CODE>`: sets the explicit local country
/// - `--help`: prints help and exits successfully
///
/// When both `--desktop` and `--server` are provided, the last one wins.
///
/// # Errors
/// Returns an error when an argument is unknown, when `--server-port` is missing
/// its value, or when the provided port is invalid.
fn parse_runtime_options(program_name: &str) -> Result<CliAction> {
    let mut options = RuntimeOptions {
        application_mode: DEFAULT_MODE_SERVER,
        server_port: DEFAULT_SERVER_PORT,
        entrypoint_root: None,
        entrypoint_api: None,
        current_country: None,
    };
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--desktop" => options.application_mode = ApplicationMode::Desktop,
            "--server" => options.application_mode = ApplicationMode::Server,
            "--help" => {
                print_help(&program_name);
                return Ok(CliAction::ExitSuccess);
            }
            "--refresh-ip-countries" => {
                return Ok(CliAction::RefreshIpCountries);
            }
            "--server-port" => {
                let port = args.next().context("missing value for `--server-port`")?;

                options.server_port = port
                    .parse::<u16>()
                    .with_context(|| format!("invalid value for `--server-port`: `{port}`"))?;
            }
            "--entrypoint-root" => {
                options.entrypoint_root = Some(
                    args.next()
                        .context("missing value for `--entrypoint-root`")?,
                );
            }
            "--entrypoint-api" => {
                options.entrypoint_api = Some(
                    args.next()
                        .context("missing value for `--entrypoint-api`")?,
                );
            }
            "--current-country" => {
                options.current_country = Some(
                    args.next()
                        .context("missing value for `--current-country`")?,
                );
            }
            _ => bail!("unknown argument: `{arg}`"),
        }
    }

    Ok(CliAction::Run(options))
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
    arachnea_core::logger::set_default_log_level_debug!(INFO);
    StreamScraper::init_sub_logger_levels();
    arachnea_core::logger::init_logger();

    let program_name = application::program_name();
    let options = match parse_runtime_options(&program_name) {
        Ok(CliAction::Run(options)) => options,
        Ok(CliAction::RefreshIpCountries) => {
            return refresh_ip_countries_cli().await;
        }
        Ok(CliAction::ExitSuccess) => return Ok(()),
        Err(error) => {
            eprintln!("Error: {error}\n");
            eprintln!("{}", help_message(&program_name));
            return Err(error);
        }
    };

    // Keep the encrypted store construction in main so the server key remains
    // initialized at bootstrap even while server mode still uses the clear JSON store.
    let _server_encrypted_store = EncryptedFileCredentialsStore::new(
        application::get_application_path(DEFAULT_ENCRYPTED_FILE_CREDENTIALS_STORE_PATH),
        DEFAULT_SERVER_CREDENTIALS_KEY,
    );

    let manager = StreamScraper::from_json(None)?;
    if let Some(current_country) = &options.current_country {
        manager.set_current_country(current_country).await;
    }
    let web_assets = generated_embedded_web_assets();

    let mut controler: Box<dyn ControlerService> = if options.application_mode
        == ApplicationMode::Server
    {
        let mut configuration =
            RestControlerConfiguration::default().server_port(options.server_port);
        if let Some(entrypoint_root) = &options.entrypoint_root {
            configuration = configuration.entrypoint_root(entrypoint_root);
        }
        if let Some(entrypoint_api) = &options.entrypoint_api {
            configuration = configuration.entrypoint_api(entrypoint_api);
        }

        Box::new(RestControlerService::new(configuration))
    } else {
        Box::new(tauri_controler_service())
    };

    // controler.register_web_directory(resources::get_application_path("front"), "");
    controler.register_embedded_web_assets(web_assets, "");

    manager.register_service(controler.as_mut());
    controler.launch();
    Ok(())
}
