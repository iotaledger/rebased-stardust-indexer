// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use db::{ConnectionPool, ConnectionPoolConfig};
use tracing::{Level, error, info};
use tracing_subscriber::FmtSubscriber;

use crate::rest::spawn_rest_server;

mod db;
mod models;
mod rest;
mod schema;

use rest::config::RestApiConfig;
use tokio::sync::oneshot;

#[derive(Parser, Clone, Debug)]
#[clap(
    name = "Rebased stardust indexer",
    about = "An application indexing data on migrated stardust outputs, and serving them through a REST API"
)]
pub struct Config {
    #[arg(long, default_value = "INFO")]
    #[arg(env = "LOG_LEVEL")]
    pub log_level: Level,
    #[clap(flatten)]
    pub connection_pool_config: ConnectionPoolConfig,
    #[clap(flatten)]
    pub rest_api_config: RestApiConfig,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Config::parse();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(opts.log_level)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    // Create a oneshot channel for shutdown signaling
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Spawn a task to listen for CTRL+C and send a shutdown signal
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for CTRL+C");
        info!("CTRL+C received, shutting down.");
        let _ = shutdown_tx.send(());
    });

    let connection_pool = ConnectionPool::new(opts.connection_pool_config)?;
    connection_pool.run_migrations()?;

    // TODO: Spawn synchronization logic

    // Spawn the REST server
    _ = spawn_rest_server(opts.rest_api_config, connection_pool, shutdown_rx)
        .await
        .inspect_err(|e| error!("REST server terminated with error: {e}"));

    Ok(())
}
