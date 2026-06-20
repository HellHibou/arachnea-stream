#![warn(missing_docs)]
//! Arachnea backend executable: wires scraper sources and controller backends.

use anyhow::{bail, Context, Result};
use std::{path::Path, sync::Arc};

use arachnea_core::{
    controler::{
        rest::{RestControlerConfiguration, RestControlerService},
        tauri::{TauriControlerConfiguration, TauriControlerService, TauriEmbeddedWebAssets},
        ControlerService, SharedWebAssets,
    },
    persistence::{
        resources, CredentialsStore, EncryptedFileCredentialsStore, FileCredentialsStore,
    },
};
use arachnea_scrapyfy::*;
use arachnea_proxy::core::{ArachneaProxyCore, EgressPool, ProxyNode};
use arachnea_stream::StreamScraper;

/// Default port used by the REST controller when no CLI override is provided.
const DEFAULT_SERVER_PORT: u16 = 8080;

/// Default runtime mode used when no CLI mode flag is provided.
#[cfg(not(debug_assertions))] // Release mode defaults to server
const DEFAULT_MODE_SERVER: bool = false;
#[cfg(debug_assertions)] // Debug mode defaults.
const DEFAULT_MODE_SERVER: bool = true;

/// Default path used by the clear JSON credentials store.
const DEFAULT_FILE_CREDENTIALS_STORE_PATH: &str = "data/credentials.json";

/// Default path used by the encrypted server credentials store.
const DEFAULT_ENCRYPTED_FILE_CREDENTIALS_STORE_PATH: &str = "data/credentials";

/// Default path used by the services configuration file.
const DEFAULT_SERVICES_CONFIG_PATH: &str = "services/services.json";

/// Custom URI scheme used by the desktop frontend.
const TAURI_WEB_SCHEME: &str = "arachnea";

/// API prefix used by the desktop binary stream routes.
const TAURI_API_PREFIX: &str = "/api/";

/// Static AES-256-GCM key reserved for the encrypted file store.
const DEFAULT_SERVER_CREDENTIALS_KEY: [u8; 32] = *b"hell_hibou-arachnea-key-20260422";

/// Runtime options parsed from command line arguments.
struct RuntimeOptions {
    mode_server: bool,
    server_port: u16,
}

/// Result of parsing command line arguments.
enum CliAction {
    Run(RuntimeOptions),
    ExitSuccess,
}

/// Returns the executable name used in help and error messages.
fn program_name() -> String {
    std::env::args_os()
        .next()
        .and_then(|arg0| {
            let path = Path::new(&arg0);

            path.file_name()
                .map(|file_name| file_name.to_string_lossy().into_owned())
                .or_else(|| {
                    let raw = arg0.to_string_lossy().into_owned();
                    (!raw.is_empty()).then_some(raw)
                })
        })
        .unwrap_or_else(|| String::from("arachnea"))
}

/// Returns the command line help in English.
fn help_message(program_name: &str) -> String {
    format!(
        "Usage: {program_name} [OPTIONS]

Options:
  --desktop              Run in desktop application mode (forces mode_server to false)
  --server               Run in server mode (forces mode_server to true)
  --server-port <PORT>   Override the server port (default: {DEFAULT_SERVER_PORT})
  --help                 Show this help message and exit"
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
/// - `--help`: prints help and exits successfully
///
/// When both `--desktop` and `--server` are provided, the last one wins.
///
/// # Errors
/// Returns an error when an argument is unknown, when `--server-port` is missing
/// its value, or when the provided port is invalid.
fn parse_runtime_options(program_name: &str) -> Result<CliAction> {
    let mut options = RuntimeOptions {
        mode_server: DEFAULT_MODE_SERVER,
        server_port: DEFAULT_SERVER_PORT,
    };
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--desktop" => options.mode_server = false,
            "--server" => options.mode_server = true,
            "--help" => {
                print_help(&program_name);
                return Ok(CliAction::ExitSuccess);
            }
            "--server-port" => {
                let port = args.next().context("missing value for `--server-port`")?;

                options.server_port = port
                    .parse::<u16>()
                    .with_context(|| format!("invalid value for `--server-port`: `{port}`"))?;
            }
            _ => bail!("unknown argument: `{arg}`"),
        }
    }

    Ok(CliAction::Run(options))
}

/// Starts the configured backend controller using command line runtime options.
#[tokio::main]
async fn main() -> Result<()> {
    arachnea_core::logger::set_default_log_level_debug!(DEBUG);
    StreamScraper::init_sub_logger_levels();
    arachnea_core::logger::init_logger();

    let program_name = program_name();
    let options = match parse_runtime_options(&program_name) {
        Ok(CliAction::Run(options)) => options,
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
        resources::get_application_path(DEFAULT_ENCRYPTED_FILE_CREDENTIALS_STORE_PATH),
        DEFAULT_SERVER_CREDENTIALS_KEY,
    );
    let credentials_store: Arc<dyn CredentialsStore> = if options.mode_server {
        Arc::new(FileCredentialsStore::new(resources::get_application_path(
            DEFAULT_FILE_CREDENTIALS_STORE_PATH,
        )))
    } else {
        Arc::new(FileCredentialsStore::new(resources::get_application_path(
            DEFAULT_FILE_CREDENTIALS_STORE_PATH,
        )))
    };

    let mut manager = StreamScraper::with_credentials_store(credentials_store);
    manager
        .get_scraper_agregator_mut()
        .add_query_collection_from_config_json(resources::get_application_path(
            DEFAULT_SERVICES_CONFIG_PATH,
        ))?;

    // Configure proxy pool for the manager and proxy_http handler
    let proxy_nodes = vec![
        ProxyNode::socks5("proxy-socks5-1", "158.178.198.31:1080"),
    ];
    let proxy_pool = EgressPool::new("main-pool", proxy_nodes, None);
    let pool_config = proxy_pool.config("main-chain");

    let proxy_core_for_http = match ArachneaProxyCore::new(pool_config) {
        Ok(proxy_core) => {
            manager.set_proxy(HttpProxyConfig::Arachnea(proxy_core.clone()));
            tracing::info!("Proxy core created for proxy_http handler");
            Some(proxy_core)
        }
        Err(e) => {
            tracing::warn!(
                "Failed to create proxy core: {}; proxy_http will be unavailable",
                e
            );
            None
        }
    };

    let web_assets = generated_embedded_web_assets();

    let mut controler: Box<dyn ControlerService> = if options.mode_server {
        Box::new(RestControlerService::new(
            RestControlerConfiguration::default().server_port(options.server_port),
        ))
    } else {
        Box::new(tauri_controler_service())
    };

    // Register proxy_http handler if proxy core is available
    if let Some(proxy_core) = proxy_core_for_http.as_ref() {
        arachnea_proxy::core::http::register_service(controler.as_mut(), proxy_core, "proxy");
    } else {
        tracing::warn!("proxy_http handler not registered: no proxy core available");
    }

    // controler.register_web_directory(resources::get_application_path("front"), "");
    controler.register_embedded_web_assets(web_assets, "");

    manager.register_service(controler.as_mut());
    controler.launch();
    Ok(())
}
