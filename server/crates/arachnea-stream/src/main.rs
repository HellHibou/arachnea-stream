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
use arachnea_proxy::core::{
    ArachneaProxyCore, ParameterHandlerConfig, ParameterHandlerKind, ParameterProxyRoute,
    ProxyChain, ProxyConfig, ProxyNode, ProxyProfile, RoutePolicy, PROXY_HEADER_PARAMETER_COUNTRY,
    PROXY_PARAMETER_COUNTRY,
};
use arachnea_scrapyfy::*;
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
    entrypoint_root: Option<String>,
    entrypoint_api: Option<String>,
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
  --help                 Show this help message and exit
  --desktop              Run in desktop application mode (forces mode_server to false)
  --server               Run in server mode (forces mode_server to true)
  --server-port <PORT>   Override the server port (default: {DEFAULT_SERVER_PORT})
  --entrypoint-root <PATH>
                          Public root path used before API routes in server mode
  --entrypoint-api <PATH>
                          Public API path segment used in server mode"
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
        entrypoint_root: None,
        entrypoint_api: None,
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
            _ => bail!("unknown argument: `{arg}`"),
        }
    }

    Ok(CliAction::Run(options))
}

/// Starts the configured backend controller using command line runtime options.
#[tokio::main]
async fn main() -> Result<()> {
    arachnea_core::logger::set_default_log_level_debug!(INFO);
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

    /*
    manager.clear_proxy();

    let proxy_fr = ProxyNode::from_url(
        "proxy-fr",
       //  "socks5://158.178.198.31:1080", 
        // "socks5://62.133.62.3:1081"
        //"socks5://45.95.233.237:1081",
       "socks5://51.210.5.144:1088"
    )
    .context("Failed to configure the FR proxy node")?;
    let proxy_config = ProxyConfig {
        profile: ProxyProfile::Advanced,
        chains: vec![ProxyChain::direct()],
        routing: RoutePolicy {
            default_chain: Some("direct".to_string()),
            ..RoutePolicy::default()
        },
        parameter_handlers: vec![ParameterHandlerConfig {
            kind: ParameterHandlerKind::CountryRouting,
            parameter_name: Some(PROXY_PARAMETER_COUNTRY.to_string()),
            http_header: Some(PROXY_HEADER_PARAMETER_COUNTRY.to_string()),
            forward_header: false,
            stop_on_match: true,
            routes: vec![ParameterProxyRoute {
                value: "FR".to_string(),
                proxy: proxy_fr,
            }],
        }],
        ..ProxyConfig::default()
    };

    let proxy_core_for_http = match ArachneaProxyCore::new(proxy_config) {
        Ok(proxy_core) => {
            manager
                .get_scraper_agregator_mut()
                .set_proxy_core(proxy_core.clone());
            tracing::info!("Proxy core created with FR country routing");
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
    manager.set_proxy_http_core(proxy_core_for_http);
    // */
    let web_assets = generated_embedded_web_assets();

    let mut controler: Box<dyn ControlerService> = if options.mode_server {
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
