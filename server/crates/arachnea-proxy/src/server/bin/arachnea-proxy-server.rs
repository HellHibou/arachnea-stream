use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use tokio::net::TcpStream;
use tokio::time;

use arachnea_proxy::core::{ApplicationProtocol, ConnectRequest, Destination};
use arachnea_proxy::server::{FileConfig, ProxyServer};

/// Arachnea proxy server.
#[derive(Debug, Parser)]
#[command(name = "arachnea-proxy-server")]
#[command(about = "Arachnea HTTP/HTTPS/SOCKS proxy server")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Path to a TOML configuration file.
    #[arg(short, long)]
    config: Option<PathBuf>,
}

/// Top-level command selected by the proxy CLI.
#[derive(Debug, Subcommand)]
enum Command {
    /// Starts configured proxy listeners.
    Serve {
        /// Path to a TOML configuration file.
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Validates a TOML configuration file.
    ValidateConfig {
        /// Path to a TOML configuration file.
        config: PathBuf,
    },
    /// Prints the effective core and server configuration as JSON.
    ShowEffectiveConfig {
        /// Path to a TOML configuration file.
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Prints configured proxy listeners as JSON.
    ShowListeners {
        /// Path to a TOML configuration file.
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Prints configured proxy chains as JSON.
    ShowChains {
        /// Path to a TOML configuration file.
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Resolves the route that would be used for a destination.
    TestRoute {
        /// Destination authority such as example.com:443.
        destination: String,
        /// Application protocol used for route matching.
        #[arg(long, default_value = "tcp")]
        protocol: CliProtocol,
        /// Path to a TOML configuration file.
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Opens a proxied TCP connection to verify the selected route.
    TestConnect {
        /// Destination authority such as example.com:443.
        destination: String,
        /// Application protocol used for route matching.
        #[arg(long, default_value = "tcp")]
        protocol: CliProtocol,
        /// Path to a TOML configuration file.
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Checks TCP reachability for configured upstream proxy endpoints.
    TestUpstream {
        /// Optional upstream node name to test.
        name: Option<String>,
        /// Path to a TOML configuration file.
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
}

/// Application protocol accepted by diagnostic CLI commands.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum CliProtocol {
    /// Raw TCP destination.
    Tcp,
    /// UDP destination used for route selection only.
    Udp,
    /// Plain HTTP destination.
    Http,
    /// HTTPS destination.
    Https,
}

impl CliProtocol {
    /// Converts the CLI value into the core protocol enum.
    ///
    /// # Returns
    ///
    /// Core application protocol matching the CLI value.
    fn into_protocol(self) -> ApplicationProtocol {
        match self {
            Self::Tcp => ApplicationProtocol::Tcp,
            Self::Udp => ApplicationProtocol::Udp,
            Self::Http => ApplicationProtocol::Http,
            Self::Https => ApplicationProtocol::Https,
        }
    }
}

/// Runs the proxy server command-line entrypoint.
///
/// # Errors
///
/// Returns an error when configuration loading, validation, or listener startup
/// fails.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Serve { config }) => serve(load_config(config)?).await,
        Some(Command::ValidateConfig { config }) => {
            let file_config = load_config(Some(config))?;
            file_config.server.validate().map_err(anyhow::Error::msg)?;
            file_config.core.resolve()?;
            println!("configuration OK");
            Ok(())
        }
        Some(Command::ShowEffectiveConfig { config }) => {
            let file_config = load_config(config)?;
            let server = file_config.server;
            server.validate().map_err(anyhow::Error::msg)?;
            let core = file_config.core.resolve()?;
            println!(
                "{}",
                serde_json::json!({
                    "server": server,
                    "core": core.as_config(),
                })
            );
            Ok(())
        }
        Some(Command::ShowListeners { config }) => show_listeners(load_config(config)?),
        Some(Command::ShowChains { config }) => show_chains(load_config(config)?),
        Some(Command::TestRoute {
            destination,
            protocol,
            config,
        }) => test_route(load_config(config)?, &destination, protocol).await,
        Some(Command::TestConnect {
            destination,
            protocol,
            config,
        }) => test_connect(load_config(config)?, &destination, protocol).await,
        Some(Command::TestUpstream { name, config }) => {
            test_upstream(load_config(config)?, name.as_deref()).await
        }
        None => serve(load_config(cli.config)?).await,
    }
}

/// Starts proxy listeners from a resolved file configuration.
///
/// # Parameters
///
/// - `file_config`: Parsed TOML configuration for the server and core.
///
/// # Errors
///
/// Returns an error when the server cannot be created or a listener fails.
async fn serve(file_config: FileConfig) -> anyhow::Result<()> {
    let server = ProxyServer::new(file_config.server, file_config.core)?;
    server.run().await
}

/// Loads a TOML configuration file or returns defaults.
///
/// # Parameters
///
/// - `path`: Optional path to a TOML configuration file.
///
/// # Returns
///
/// Parsed file configuration, or defaults when `path` is `None`.
///
/// # Errors
///
/// Returns an error when the file cannot be read or parsed.
fn load_config(path: Option<PathBuf>) -> anyhow::Result<FileConfig> {
    if let Some(path) = path {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        toml::from_str::<FileConfig>(&text)
            .with_context(|| format!("failed to parse {}", path.display()))
    } else {
        Ok(FileConfig::default())
    }
}

/// Prints listener diagnostics for a configuration.
///
/// # Parameters
///
/// - `file_config`: Parsed file configuration.
///
/// # Errors
///
/// Returns an error when server validation fails or JSON output cannot be
/// serialized.
fn show_listeners(file_config: FileConfig) -> anyhow::Result<()> {
    file_config.server.validate().map_err(anyhow::Error::msg)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "http": file_config.server.listen_http,
            "socks": file_config.server.listen_socks,
            "https": file_config.server.listen_https,
            "authentication_required": file_config.server.authentication.is_required(),
            "max_http_header_bytes": file_config.server.max_http_header_bytes,
        }))?
    );
    Ok(())
}

/// Prints configured proxy chains without exposing secrets.
///
/// # Parameters
///
/// - `file_config`: Parsed file configuration.
///
/// # Errors
///
/// Returns an error when configuration validation or JSON serialization fails.
fn show_chains(file_config: FileConfig) -> anyhow::Result<()> {
    file_config.server.validate().map_err(anyhow::Error::msg)?;
    let core = file_config.core.resolve()?;
    let chains: Vec<_> = core
        .as_config()
        .chains
        .iter()
        .map(|chain| {
            serde_json::json!({
                "name": chain.name,
                "strategy": chain.strategy,
                "nodes": chain.nodes.iter().map(|node| {
                    serde_json::json!({
                        "name": node.name,
                        "kind": node.kind,
                        "endpoint": node.endpoint,
                        "send_hostname": node.send_hostname,
                        "authentication_configured": node.username.is_some() && node.password_env.is_some(),
                        "verify_tls": node.verify_tls,
                    })
                }).collect::<Vec<_>>(),
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&chains)?);
    Ok(())
}

/// Prints the selected route for a destination.
///
/// # Parameters
///
/// - `file_config`: Parsed file configuration.
/// - `destination`: Destination authority such as `example.com:443`.
/// - `protocol`: Application protocol used for route matching.
///
/// # Errors
///
/// Returns an error when config, destination parsing, routing, or JSON output
/// fails.
async fn test_route(
    file_config: FileConfig,
    destination: &str,
    protocol: CliProtocol,
) -> anyhow::Result<()> {
    file_config.server.validate().map_err(anyhow::Error::msg)?;
    let core = file_config.core.resolve()?;
    let destination = Destination::from_authority(destination, protocol.into_protocol())?;
    let request = ConnectRequest::new(destination);
    let decision = core.as_config().routing.decide(&core, &request)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "selected_chain": decision.selected_chain,
            "chain": decision.chain,
        }))?
    );
    Ok(())
}

/// Opens a proxied TCP connection and prints connection metadata.
///
/// # Parameters
///
/// - `file_config`: Parsed file configuration.
/// - `destination`: Destination authority such as `example.com:443`.
/// - `protocol`: Application protocol used for route matching.
///
/// # Errors
///
/// Returns an error when configuration, routing, or connection setup fails.
async fn test_connect(
    file_config: FileConfig,
    destination: &str,
    protocol: CliProtocol,
) -> anyhow::Result<()> {
    file_config.server.validate().map_err(anyhow::Error::msg)?;
    let destination = Destination::from_authority(destination, protocol.into_protocol())?;
    let core = arachnea_proxy::core::ArachneaProxyCore::new(file_config.core)?;
    if protocol == CliProtocol::Udp {
        let socket = core.connect_udp(destination).await?;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "connected",
                "protocol": "udp",
                "local_addr": socket.local_addr()?,
            }))?
        );
        return Ok(());
    }
    let stream = core.connect(destination).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "status": "connected",
            "metadata": stream.metadata(),
        }))?
    );
    Ok(())
}

/// Checks TCP reachability for one or more upstream proxy endpoints.
///
/// # Parameters
///
/// - `file_config`: Parsed file configuration.
/// - `name`: Optional upstream node name filter.
///
/// # Errors
///
/// Returns an error when configuration validation or JSON serialization fails.
async fn test_upstream(file_config: FileConfig, name: Option<&str>) -> anyhow::Result<()> {
    file_config.server.validate().map_err(anyhow::Error::msg)?;
    let core = file_config.core.resolve()?;
    let mut results = Vec::new();
    for node in core
        .as_config()
        .chains
        .iter()
        .flat_map(|chain| chain.nodes.iter())
        .filter(|node| name.is_none_or(|expected| node.name == expected))
    {
        let Some(endpoint) = &node.endpoint else {
            results.push(serde_json::json!({
                "name": node.name,
                "kind": node.kind,
                "status": "skipped",
                "reason": "direct node has no upstream endpoint",
            }));
            continue;
        };
        let status = match time::timeout(
            std::time::Duration::from_millis(core.as_config().timeouts.connect_ms),
            TcpStream::connect(endpoint),
        )
        .await
        {
            Ok(Ok(_stream)) => serde_json::json!({"status": "reachable"}),
            Ok(Err(error)) => {
                serde_json::json!({"status": "unreachable", "error": error.to_string()})
            }
            Err(_) => serde_json::json!({"status": "timeout"}),
        };
        results.push(serde_json::json!({
            "name": node.name,
            "kind": node.kind,
            "endpoint": endpoint,
            "result": status,
        }));
    }
    if results.is_empty() {
        anyhow::bail!("no upstream node matched the requested name");
    }
    println!("{}", serde_json::to_string_pretty(&results)?);
    Ok(())
}
