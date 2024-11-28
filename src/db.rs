//! Database related logic.
use std::env;

use anyhow::{Context, Result, anyhow};
use diesel::{prelude::*, sqlite::Sqlite};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use dotenvy::dotenv;

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/");

/// Run any pending migrations to the connected database.
pub fn run_migrations(connection: &mut impl MigrationHarness<Sqlite>) -> Result<()> {
    connection
        .run_pending_migrations(MIGRATIONS)
        .map_err(|e| anyhow!("failed to run migrations {e}"))?;

    Ok(())
}

/// This function establishes a connection to the database
/// defined in the `.env` file through `DATABASE_URL`.
pub fn establish_connection() -> Result<SqliteConnection> {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    SqliteConnection::establish(&database_url)
        .with_context(|| format!("error connecting to {database_url}"))
}
