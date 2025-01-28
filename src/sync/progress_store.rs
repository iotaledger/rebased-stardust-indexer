//! The progress store is responsible for indicating the last synced checkpoint
//! when the Indexer restarts or crashes
//!
//! It is required by the `iota-data-ingestion-core`

use axum::async_trait;
use diesel::prelude::*;
use iota_data_ingestion_core::ProgressStore;
use iota_types::messages_checkpoint::CheckpointSequenceNumber;

use crate::{
    METRICS, db::ConnectionPool, models::LastCheckpointSync, schema::last_checkpoint_sync::dsl::*,
};

/// Record in `SQLite` the latest synced checkpoint, this will allow the Indexer
/// to resume syncing checkpoints from last registered one instead of starting
/// from the checkpoint with sequence number `0`
pub struct SqliteProgressStore {
    pool: ConnectionPool,
}

impl SqliteProgressStore {
    pub fn new(pool: ConnectionPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ProgressStore for SqliteProgressStore {
    async fn load(&mut self, task_name: String) -> anyhow::Result<CheckpointSequenceNumber> {
        let mut conn = self.pool.get_connection()?;

        let last_checkpoint = last_checkpoint_sync
            .select(LastCheckpointSync::as_select())
            .find(task_name)
            .first::<LastCheckpointSync>(&mut conn)
            .optional()?;

        Ok(last_checkpoint
            .map(|ch| ch.sequence_number as u64)
            .unwrap_or_default())
    }

    async fn save(
        &mut self,
        task_name: String,
        checkpoint_number: CheckpointSequenceNumber,
    ) -> anyhow::Result<()> {
        let mut conn = self.pool.get_connection()?;

        let value = LastCheckpointSync {
            sequence_number: checkpoint_number as i64,
            task_id: task_name,
        };

        diesel::insert_into(last_checkpoint_sync)
            .values(&value)
            .on_conflict(task_id)
            .do_update()
            .set(&value)
            .execute(&mut conn)?;

        METRICS
            .get()
            .expect("Indexer metrics not initialized")
            .last_checkpoint_checked
            .set(checkpoint_number as i64);

        Ok(())
    }
}
