use clap::Parser;
use db::{ConnectionPool, ConnectionPoolConfig};

mod db;
mod models;
mod schema;

#[derive(Parser, Clone, Debug)]
#[clap(
    name = "Rebased stardust indexer",
    about = "An application indexing data on migrated stardust outputs, and serving them through a REST API"
)]
pub struct Config {
    #[clap(flatten)]
    pub connection_pool_config: ConnectionPoolConfig,
}

fn main() -> anyhow::Result<()> {
    let opts = Config::parse();
    let connection_pool = ConnectionPool::new(opts.connection_pool_config)?;
    connection_pool.run_migrations()?;

    // TODO: Spawn synchronization logic

    // TODO: Spawn the REST API
    Ok(())
}
