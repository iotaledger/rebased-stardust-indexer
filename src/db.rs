//! Database related logic.
use std::{env, time::Duration};

use anyhow::{Result, anyhow};
use clap::Args;
use diesel::{
    prelude::*,
    r2d2::{ConnectionManager, Pool, PooledConnection},
    sqlite::Sqlite,
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use dotenvy::dotenv;

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/");

pub type PoolConnection = PooledConnection<ConnectionManager<SqliteConnection>>;

#[derive(Args, Debug, Clone)]
pub struct ConnectionPoolConfig {
    #[arg(long, default_value_t = 20)]
    #[arg(env = "DB_POOL_SIZE")]
    pub pool_size: u32,
    #[arg(long, value_parser = parse_duration, default_value = "30")]
    #[arg(env = "DB_CONNECTION_TIMEOUT_SECS")]
    pub connection_timeout_secs: Duration,
}

fn parse_duration(arg: &str) -> Result<std::time::Duration, std::num::ParseIntError> {
    let seconds = arg.parse()?;
    Ok(std::time::Duration::from_secs(seconds))
}

impl ConnectionPoolConfig {
    const DEFAULT_POOL_SIZE: u32 = 20;
    const DEFAULT_CONNECTION_TIMEOUT_SECS: u64 = 30;

    pub fn set_pool_size(&mut self, size: u32) {
        self.pool_size = size;
    }

    pub fn set_connection_timeout(&mut self, timeout: Duration) {
        self.connection_timeout_secs = timeout;
    }
}

impl Default for ConnectionPoolConfig {
    fn default() -> Self {
        Self {
            pool_size: Self::DEFAULT_POOL_SIZE,
            connection_timeout_secs: Duration::from_secs(Self::DEFAULT_CONNECTION_TIMEOUT_SECS),
        }
    }
}

/// Newtype to represent the connection pool.
///
/// Uses [`Arc`][`std::sync::Arc`] internally.
#[derive(Debug, Clone)]
pub struct ConnectionPool(Pool<ConnectionManager<SqliteConnection>>);

impl ConnectionPool {
    /// Build a new pool of connections.
    ///
    /// Resolves the database URL from the environment.
    pub fn new(pool_config: ConnectionPoolConfig) -> Result<Self> {
        dotenv().ok();
        let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        Self::new_with_url(&database_url, pool_config)
    }

    /// Build a new pool of connections to the given URL.
    pub fn new_with_url(db_url: &str, pool_config: ConnectionPoolConfig) -> Result<Self> {
        let manager = ConnectionManager::new(db_url);

        Ok(Self(
            Pool::builder()
                .max_size(pool_config.pool_size)
                .connection_timeout(pool_config.connection_timeout_secs)
                .build(manager)
                .map_err(|e| {
                    anyhow!("failed to initialize connection pool for {db_url} with error: {e:?}")
                })?,
        ))
    }

    /// Get a connection from the pool.
    pub fn get_connection(&self) -> Result<PoolConnection> {
        self.0.get().map_err(|e| {
            anyhow!("failed to get connection from PG connection pool with error: {e:?}",)
        })
    }

    /// Run pending migrations.
    pub fn run_migrations(&self) -> Result<()> {
        run_migrations(&mut self.get_connection()?)
    }

    /// Revert all applied migrations
    pub fn revert_all_migrations(&self) -> Result<()> {
        revert_all_migrations(&mut self.get_connection()?)
    }
}

/// Run any pending migrations to the connected database.
pub fn run_migrations(connection: &mut impl MigrationHarness<Sqlite>) -> Result<()> {
    connection
        .run_pending_migrations(MIGRATIONS)
        .map_err(|e| anyhow!("failed to run migrations {e}"))?;

    Ok(())
}

/// Revert all applied migrations to the connected database
pub fn revert_all_migrations(connection: &mut impl MigrationHarness<Sqlite>) -> Result<()> {
    connection
        .revert_all_migrations(MIGRATIONS)
        .map_err(|e| anyhow!("failed to revert all migrations {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_migrations_with_pool() {
        let test_db = "run_migrations_with_pool.db";
        let pool = ConnectionPool::new_with_url(test_db, Default::default()).unwrap();
        pool.run_migrations().unwrap();

        // clean-up test db
        std::fs::remove_file(test_db).unwrap();
    }
}
