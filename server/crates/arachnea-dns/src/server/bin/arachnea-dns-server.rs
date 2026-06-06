use anyhow::{Context, Result};
use arachnea_dns::core::{ArachneaDnsCore, QueryRequest, RecordType};
use arachnea_dns::server::{FileConfig, Server};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Command-line options for the DNS server binary.
#[derive(Debug, Parser)]
#[command(name = "arachnea-dns-server")]
#[command(about = "Arachnea DNS server and diagnostics")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Top-level command selected by the DNS CLI.
#[derive(Debug, Subcommand)]
enum Command {
    Serve {
        #[arg(long)]
        config: PathBuf,
    },
    Resolve {
        name: String,
        #[arg(long, default_value = "A")]
        record_type: RecordType,
        #[arg(long)]
        config: PathBuf,
    },
    ValidateConfig {
        config: PathBuf,
    },
    ShowEffectiveConfig {
        #[arg(long)]
        config: PathBuf,
    },
}

/// Runs the DNS server command-line entrypoint.
///
/// # Errors
///
/// Returns an error when configuration loading, validation, server startup, or
/// diagnostic resolution fails.
#[tokio::main]
async fn main() -> Result<()> {
    let env_filter = tracing_subscriber::EnvFilter::from_default_env();
    if std::env::var("ARACHNEA_DNS_LOG_JSON").as_deref() == Ok("1") {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    }

    match Cli::parse().command {
        Command::Serve { config } => {
            let file_config = FileConfig::load(&config)?;
            let server_config = file_config.server.clone();
            let core = ArachneaDnsCore::new(file_config.into_core_config()?)?;
            Server::new(server_config, core).start().await
        }
        Command::Resolve {
            name,
            record_type,
            config,
        } => {
            let core = ArachneaDnsCore::new(FileConfig::load(config)?.into_core_config()?)?;
            let answer = core
                .resolve(QueryRequest::new(name, record_type))
                .await
                .context("resolution failed")?;
            println!("{}", serde_json::to_string_pretty(&answer)?);
            Ok(())
        }
        Command::ValidateConfig { config } => {
            let core_config = FileConfig::load(config)?.into_core_config()?;
            core_config.resolve_effective_config()?;
            println!("configuration OK");
            Ok(())
        }
        Command::ShowEffectiveConfig { config } => {
            let effective = FileConfig::load(config)?
                .into_core_config()?
                .resolve_effective_config()?;
            println!("{}", serde_json::to_string_pretty(&effective)?);
            Ok(())
        }
    }
}
