// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::{fs, path::Path};

use clap::{Parser, Subcommand};
use db::{ConnectionPool, ConnectionPoolConfig};
use tracing::{Level, error, info};
use tracing_subscriber::FmtSubscriber;
use utoipa::OpenApi;

use crate::rest::{ApiDoc, spawn_rest_server};

mod db;
mod models;
mod rest;
mod schema;

use tokio_util::sync::CancellationToken;

/// The main CLI application
#[derive(Parser, Clone, Debug)]
#[clap(
    name = "Rebased stardust indexer",
    about = "An application indexing data on migrated stardust outputs, and serving them through a REST API"
)]
struct Cli {
    #[clap(subcommand)]
    command: Command,
}

/// Commands supported by the application
#[derive(Subcommand, Clone, Debug)]
enum Command {
    /// Generate the OpenAPI specification
    GenerateSpec,
    /// Start the Indexer and its REST API
    StartIndexer {
        #[clap(long, default_value = "INFO", env = "LOG_LEVEL")]
        log_level: Level,
        #[clap(flatten)]
        connection_pool_config: ConnectionPoolConfig,
        #[clap(long, default_value = "0.0.0.0:3000", env = "REST_API_SOCKET_ADDRESS")]
        rest_api_address: std::net::SocketAddr,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::GenerateSpec => {
            generate_openapi_spec();
        }
        Command::StartIndexer {
            log_level,
            connection_pool_config,
            rest_api_address,
        } => {
            run_indexer(log_level, connection_pool_config, rest_api_address).await?;
        }
    }

    Ok(())
}

/// Generate and save the OpenAPI specification
fn generate_openapi_spec() {
    let spec_json = ApiDoc::openapi()
        .to_pretty_json()
        .expect("failed to generate OpenAPI spec");

    // Define the target path: `./spec/openrpc.json`
    let spec_dir = Path::new("spec");
    let spec_file = spec_dir.join("openapi.json");

    if let Err(e) = fs::create_dir_all(&spec_dir) {
        eprintln!("Failed to create directory '{}': {}", spec_dir.display(), e);
        std::process::exit(1);
    }

    if let Err(e) = fs::write(&spec_file, spec_json) {
        eprintln!(
            "Failed to write OpenAPI spec to '{}': {}",
            spec_file.display(),
            e
        );
        std::process::exit(1);
    }

    println!("OpenAPI spec written to '{}'", spec_file.display());
}

/// Run the indexer and start the REST API
async fn run_indexer(
    log_level: Level,
    connection_pool_config: ConnectionPoolConfig,
    rest_api_address: std::net::SocketAddr,
) -> anyhow::Result<()> {
    setup_logging(log_level);

    let token = setup_shutdown_signal();
    let connection_pool = ConnectionPool::new(connection_pool_config)?;
    connection_pool.run_migrations()?;

    spawn_rest_server(rest_api_address, connection_pool, token)
        .await
        .inspect_err(|e| error!("REST server terminated with error: {e}"))?;

    Ok(())
}

/// Set up logging based on the specified log level
fn setup_logging(log_level: Level) {
    let subscriber = FmtSubscriber::builder().with_max_level(log_level).finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("failed to set global logging subscriber");
}

/// Set up a CTRL+C handler to gracefully shut down
fn setup_shutdown_signal() -> CancellationToken {
    let token = CancellationToken::new();
    let cloned_token = token.clone();

    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for CTRL+C");
        info!("CTRL+C received, shutting down.");
        cloned_token.cancel();
    });

    token
}
