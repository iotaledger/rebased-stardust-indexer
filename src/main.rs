// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use clap::Parser;
use db::{ConnectionPool, ConnectionPoolConfig};
use handlers::IndexerHandle;
use tokio_graceful_shutdown::{
    IntoSubsystem, SubsystemBuilder, Toplevel,
    errors::{GracefulShutdownError, SubsystemError},
};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use crate::{handlers::IndexerConfig, rest::spawn_rest_server};

mod db;
mod handlers;
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
    #[arg(long, default_value = "0.0.0.0:3000")]
    #[arg(env = "REST_API_SOCKET_ADDRESS")]
    pub rest_api_socket_address: std::net::SocketAddr,
    #[clap(flatten)]
    pub indexer_config: IndexerConfig,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Config::parse();

    init_tracing(opts.log_level);

    let connection_pool = ConnectionPool::new(opts.connection_pool_config)?;
    connection_pool.run_migrations()?;

    // Spawn synchronization logic from a Fullnode
    let indexer_handle = IndexerHandle::init(connection_pool.clone(), opts.indexer_config).await?;

    // Spawn the REST server
    let rest_api_handle = spawn_rest_server(
        opts.rest_api_socket_address,
        connection_pool,
        CancellationToken::new(),
    );

    // Register the subsystems we want to notify for a graceful shutdown
    Toplevel::new(|s| async move {
        s.start(SubsystemBuilder::new(
            "IndexerHandle",
            indexer_handle.into_subsystem(),
        ));
        s.start(SubsystemBuilder::new(
            "RestApi",
            rest_api_handle.into_subsystem(),
        ));
    })
    .catch_signals()
    .handle_shutdown_requests(Duration::from_millis(1000))
    .await
    .inspect_err(log_subsystem_error)
    .map_err(Into::into)
}

/// Initialize the tracing with custom subsribers
fn init_tracing(log_level: Level) {
    let subscriber = FmtSubscriber::builder().with_max_level(log_level).finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}

/// Log subsystem errors
fn log_subsystem_error(err: &GracefulShutdownError) {
    for subsystem_error in err.get_subsystem_errors() {
        match subsystem_error {
            SubsystemError::Failed(name, e) => {
                tracing::error!("subsystem '{name}' failed: {}", e.get_error());
            }
            SubsystemError::Panicked(name) => {
                tracing::error!("subsystem '{name}' panicked")
            }
        }
    }
}
