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

use tokio_util::sync::CancellationToken;

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
    #[arg(long, default_value = "0.0.0.0::3000")]
    #[arg(env = "REST_API_SOCKET_ADDRESS")]
    pub rest_api_socket_address: std::net::SocketAddr,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Config::parse();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(opts.log_level)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    // Spawn a task to listen for CTRL+C and send a shutdown signal
    let token = CancellationToken::new();
    let cloned_token = token.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for CTRL+C");
        info!("CTRL+C received, shutting down.");
        cloned_token.cancel();
    });

    let connection_pool = ConnectionPool::new(opts.connection_pool_config)?;
    connection_pool.run_migrations()?;

    // TODO: Spawn synchronization logic

    // Spawn the REST server
    _ = spawn_rest_server(opts.rest_api_socket_address, connection_pool, token)
        .await
        .inspect_err(|e| error!("REST server terminated with error: {e}"));

    Ok(())
}
