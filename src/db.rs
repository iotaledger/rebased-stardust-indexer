// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

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

pub const DEFAULT_DB_MIGRATIONS: EmbeddedMigrations =
    embed_migrations!("migrations/default_db_migrations");
pub const PROGRESS_STORE_MIGRATIONS: EmbeddedMigrations =
    embed_migrations!("migrations/progress_store_migrations");

pub type PoolConnection = PooledConnection<ConnectionManager<SqliteConnection>>;
pub type ProgressStorePoolConnection = PooledConnection<ConnectionManager<SqliteConnection>>;

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

#[allow(dead_code)]
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
pub struct GenericConnectionPool(Pool<ConnectionManager<SqliteConnection>>);

impl GenericConnectionPool {
    /// Build a new pool of connections.
    ///
    /// Resolves the database URL from the environment.
    pub fn new(pool_config: ConnectionPoolConfig, db_url_env_var: &str) -> Result<Self> {
        dotenv().ok();
        let database_url =
            env::var(db_url_env_var).unwrap_or_else(|_| panic!("{db_url_env_var} must be set"));
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
    pub fn get_connection(&self) -> Result<PooledConnection<ConnectionManager<SqliteConnection>>> {
        self.0
            .get()
            .map_err(|e| anyhow!("failed to get connection from connection pool with error: {e:?}"))
    }
}

/// Run any pending migrations.
pub fn run_migrations(
    connection: &mut impl MigrationHarness<Sqlite>,
    migrations: EmbeddedMigrations,
) -> Result<()> {
    connection
        .run_pending_migrations(migrations)
        .map_err(|e| anyhow!("failed to run migrations {e}"))?;

    Ok(())
}

/// Revert all applied migrations.
pub fn revert_all_migrations(
    connection: &mut impl MigrationHarness<Sqlite>,
    migrations: EmbeddedMigrations,
) -> Result<()> {
    connection
        .revert_all_migrations(migrations)
        .map_err(|e| anyhow!("failed to revert all migrations {e}"))?;

    Ok(())
}

/// Create a new instance of `ConnectionPool`.
#[derive(Debug, Clone)]
pub struct ConnectionPool(GenericConnectionPool);

/// Create a new instance of `ProgressStorePool`.
#[derive(Debug, Clone)]
pub struct ProgressStorePool(GenericConnectionPool);

#[allow(dead_code)]
impl ConnectionPool {
    /// Build a new pool of connections.
    ///
    /// Resolves the database URL from the environment.
    pub fn new(pool_config: ConnectionPoolConfig) -> Result<Self> {
        GenericConnectionPool::new(pool_config, "DATABASE_URL").map(Self)
    }

    /// Build a new pool of connections to the given URL.
    pub fn new_with_url(db_url: &str, pool_config: ConnectionPoolConfig) -> Result<Self> {
        GenericConnectionPool::new_with_url(db_url, pool_config).map(Self)
    }

    /// Get a connection from the pool.
    pub fn get_connection(&self) -> Result<PoolConnection> {
        self.0
            .get_connection()
            .map_err(|e| anyhow!("failed to get connection from connection pool with error: {e:?}"))
    }

    /// Run pending migrations.
    pub fn run_migrations(&self) -> Result<()> {
        run_migrations(&mut self.get_connection()?, DEFAULT_DB_MIGRATIONS)
    }

    /// Revert all applied migrations
    pub fn revert_all_migrations(&self) -> Result<()> {
        revert_all_migrations(&mut self.get_connection()?, DEFAULT_DB_MIGRATIONS)
    }
}

impl ProgressStorePool {
    /// Build a new pool of connections.
    ///
    /// Resolves the database URL from the environment.
    pub fn new(pool_config: ConnectionPoolConfig) -> Result<Self> {
        GenericConnectionPool::new(pool_config, "PROGRESS_STORE_DB_URL").map(Self)
    }

    /// Get a connection from the pool.
    pub fn get_connection(&self) -> Result<ProgressStorePoolConnection> {
        self.0
            .get_connection()
            .map_err(|e| anyhow!("failed to get connection from connection pool with error: {e:?}"))
    }

    /// Run pending migrations.
    pub fn run_migrations(&self) -> Result<()> {
        run_migrations(&mut self.get_connection()?, PROGRESS_STORE_MIGRATIONS)
    }

    /// Revert all applied migrations
    pub fn revert_all_migrations(&self) -> Result<()> {
        revert_all_migrations(&mut self.get_connection()?, PROGRESS_STORE_MIGRATIONS)
    }
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
