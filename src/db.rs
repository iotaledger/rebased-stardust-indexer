// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! Database related logic.
use std::{env, time::Duration};

use anyhow::{Result, anyhow};
use clap::Args;
use diesel::{
    connection::SimpleConnection,
    prelude::*,
    r2d2::{ConnectionManager, Pool, PooledConnection},
    sqlite::Sqlite,
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use dotenvy::dotenv;

pub const STARDUST_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/stardust");
pub const PROGRESS_STORE_MIGRATIONS: EmbeddedMigrations =
    embed_migrations!("migrations/progress_store");
pub type PoolConnection = PooledConnection<ConnectionManager<SqliteConnection>>;

#[derive(Args, Debug, Clone)]
pub struct ConnectionPoolConfig {
    #[arg(long, default_value_t = 20)]
    #[arg(env = "DB_POOL_SIZE")]
    pub pool_size: u32,
    #[arg(long, value_parser = parse_duration, default_value = "30")]
    #[arg(env = "DB_CONNECTION_TIMEOUT_SECS")]
    pub connection_timeout_secs: Duration,
    /// Enable WAL mode in the database.
    #[arg(long)]
    pub enable_wal: bool,
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
            enable_wal: false,
        }
    }
}

/// Configure custom PRAGMA statements.
///
/// Adapted from: https://stackoverflow.com/a/57717533
///
/// See more in: https://www.sqlite.org/pragma.html
impl diesel::r2d2::CustomizeConnection<SqliteConnection, diesel::r2d2::Error>
    for ConnectionPoolConfig
{
    fn on_acquire(&self, conn: &mut SqliteConnection) -> Result<(), diesel::r2d2::Error> {
        (|| {
            conn.batch_execute("PRAGMA foreign_keys = ON;")?;
            conn.batch_execute(&format!(
                "PRAGMA busy_timeout = {};",
                self.connection_timeout_secs.as_millis()
            ))?;
            if self.enable_wal {
                conn.batch_execute("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
            }
            Ok(())
        })()
        .map_err(diesel::r2d2::Error::QueryError)
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Name {
    Objects,
    ProgressStore,
}

/// Newtype to represent the connection pool.
///
/// Uses [`Arc`][`std::sync::Arc`] internally.
#[derive(Debug, Clone)]
pub struct ConnectionPool {
    pool: Pool<ConnectionManager<SqliteConnection>>,
    db_name: Name,
}

impl ConnectionPool {
    /// Build a new pool of connections.
    ///
    /// Resolves the database URL from the environment.
    pub fn new(pool_config: ConnectionPoolConfig, db_name: Name) -> Result<Self> {
        dotenv().ok();

        let db_url_env_var = match db_name {
            Name::Objects => "OBJECTS_DB_URL",
            Name::ProgressStore => "PROGRESS_STORE_DB_URL",
        };

        let database_url = env::var(db_url_env_var)
            .unwrap_or_else(|_| panic!("the {db_url_env_var} environment variable must be set"));
        Self::new_with_url(&database_url, pool_config, db_name)
    }

    /// Build a new pool of connections to the given URL.
    pub fn new_with_url(
        db_url: &str,
        pool_config: ConnectionPoolConfig,
        db_name: Name,
    ) -> Result<Self> {
        let manager = ConnectionManager::new(db_url);

        Ok(Self {
            pool: Pool::builder()
                .max_size(pool_config.pool_size)
                .connection_timeout(pool_config.connection_timeout_secs)
                .connection_customizer(Box::new(pool_config))
                .build(manager)
                .map_err(|e| {
                    anyhow!("failed to initialize connection pool for {db_url} with error: {e:?}")
                })?,
            db_name,
        })
    }

    fn migrations(&self) -> EmbeddedMigrations {
        match self.db_name {
            Name::Objects => STARDUST_MIGRATIONS,
            Name::ProgressStore => PROGRESS_STORE_MIGRATIONS,
        }
    }

    /// Get a connection from the pool.
    pub fn get_connection(&self) -> Result<PoolConnection> {
        self.pool.get().map_err(|e| {
            anyhow!("failed to get connection from PG connection pool with error: {e:?}",)
        })
    }

    /// Run pending migrations.
    pub fn run_migrations(&self) -> Result<()> {
        run_migrations(&mut self.get_connection()?, self.migrations())
    }

    /// Revert all applied migrations
    pub fn revert_all_migrations(&self) -> Result<()> {
        revert_all_migrations(&mut self.get_connection()?, self.migrations())
    }
}

/// Run any pending migrations to the connected database.
pub fn run_migrations(
    connection: &mut impl MigrationHarness<Sqlite>,
    migrations: EmbeddedMigrations,
) -> Result<()> {
    connection
        .run_pending_migrations(migrations)
        .map_err(|e| anyhow!("failed to run migrations {e}"))?;

    Ok(())
}

/// Revert all applied migrations to the connected database
pub fn revert_all_migrations(
    connection: &mut impl MigrationHarness<Sqlite>,
    migrations: EmbeddedMigrations,
) -> Result<()> {
    connection
        .revert_all_migrations(migrations)
        .map_err(|e| anyhow!("failed to revert all migrations {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn run_objects_migrations_with_pool() {
        let test_db = "run_objects_migrations_with_pool.db";

        if Path::new(test_db).exists() {
            std::fs::remove_file(test_db).unwrap();
        }

        let pool =
            ConnectionPool::new_with_url(test_db, Default::default(), Name::Objects).unwrap();
        pool.run_migrations().unwrap();

        // clean-up test db
        std::fs::remove_file(test_db).unwrap();
    }

    #[test]
    fn run_progress_store_migrations_with_pool() {
        let test_db = "run_progress_store_migrations_with_pool.db";

        if Path::new(test_db).exists() {
            std::fs::remove_file(test_db).unwrap();
        }

        let pool =
            ConnectionPool::new_with_url(test_db, Default::default(), Name::ProgressStore).unwrap();
        pool.run_migrations().unwrap();

        // clean-up test db
        std::fs::remove_file(test_db).unwrap();
    }
}
