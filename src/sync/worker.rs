//! Workers are responsible for syncing data from Fullnode into the Indexer, It
//! can apply filtering logic to store only the desired data if necessary into a
//! local or remote storage

use std::sync::{OnceLock, atomic::AtomicU64};

use axum::async_trait;
use diesel::{Connection, ExpressionMethods, RunQueryDsl, insert_into};
use iota_data_ingestion_core::Worker;
use iota_types::{
    base_types::ObjectID,
    full_checkpoint_content::{CheckpointData, CheckpointTransaction},
    transaction::{InputObjectKind, TransactionDataAPI},
};

use crate::{
    db::ConnectionPool,
    metrics::METRICS,
    models::{ExpirationUnlockCondition, IotaAddress, ObjectType, StoredObject},
    schema::{expiration_unlock_conditions::dsl::*, objects::dsl::*},
};

/// Stores the latest checkpoint unix timestamp in milliseconds processed by the
/// `CheckpointWorker`.
pub static LATEST_CHECKPOINT_UNIX_TIMESTAMP_MS: OnceLock<AtomicU64> = OnceLock::new();

/// The `CheckpointWorker` is responsible for processing the incoming
/// `CheckpointData` from the `IndexerExecutor`, apply filtering logic if
/// necessary and save into a SQLite database
#[derive(Clone, Debug)]
pub(crate) struct CheckpointWorker {
    pool: ConnectionPool,
    /// Store data only related to the following package ids
    package_id: ObjectID,
}

impl CheckpointWorker {
    pub(crate) fn new(pool: ConnectionPool, package_id: ObjectID) -> Self {
        Self { pool, package_id }
    }

    /// Check if the provided package_id matches the desired one
    fn package_id_matches(&self, input_obj_kind: &InputObjectKind) -> bool {
        matches!(input_obj_kind, InputObjectKind::MovePackage(package_id) if *package_id == self.package_id)
    }

    /// Check if the `CheckpointTransaction` is a genesis transaction or
    /// contains the stardust package.
    fn tx_touches_stardust_objects(
        &self,
        checkpoint_tx: &CheckpointTransaction,
    ) -> anyhow::Result<bool> {
        let tx_data = &checkpoint_tx.transaction.intent_message().value;

        Ok(tx_data.is_genesis_tx()
            || tx_data
                .input_objects()?
                .iter()
                .any(|input_obj_kind| self.package_id_matches(input_obj_kind)))
    }

    /// This function iterates over `StoredObject` and
    /// `ExpirationUnlockCondition` pairs, for each pair it creates a database
    /// transaction, and inserts both the object and its expiration
    /// condition. If a conflict arises during the insertion, the existing
    /// record is updated with the new values.
    fn multi_insert_as_database_transactions(
        &self,
        stored_objects: Vec<StoredObject>,
    ) -> anyhow::Result<()> {
        let mut pool = self.pool.get_connection()?;
        for stored_object in stored_objects {
            pool.transaction::<_, anyhow::Error, _>(|conn| {
                insert_into(objects)
                    .values(&stored_object)
                    .on_conflict(id)
                    .do_update()
                    .set(&stored_object)
                    .execute(conn)?;

                let type_ = stored_object.object_type;
                let eu = ExpirationUnlockCondition::try_from(stored_object)?;

                insert_into(expiration_unlock_conditions)
                    .values(&eu)
                    .on_conflict(object_id)
                    .do_update()
                    .set(&eu)
                    .execute(conn)?;

                match type_ {
                    ObjectType::Basic => METRICS
                        .get()
                        .expect("global should be initialized")
                        .indexed_basic_outputs_count
                        .inc(),
                    ObjectType::Nft => METRICS
                        .get()
                        .expect("global should be initialized")
                        .indexed_nft_outputs_count
                        .inc(),
                }

                Ok(())
            })?;
        }

        Ok(())
    }

    fn delete_objects(&self, addresses: Vec<IotaAddress>) -> anyhow::Result<()> {
        let mut conn = self.pool.get_connection()?;
        diesel::delete(objects)
            .filter(id.eq_any(addresses))
            .execute(&mut conn)?;
        Ok(())
    }
}

#[async_trait]
impl Worker for CheckpointWorker {
    async fn process_checkpoint(&self, checkpoint: CheckpointData) -> anyhow::Result<()> {
        METRICS
            .get()
            .expect("metrics global should be initialized")
            .last_checkpoint_checked
            .set(checkpoint.checkpoint_summary.sequence_number as i64);

        let mut created_objects = Vec::new();
        let mut deleted_addresses = Vec::new();
        for checkpoint_tx in checkpoint.transactions.into_iter() {
            if self.tx_touches_stardust_objects(&checkpoint_tx)? {
                deleted_addresses.extend(
                    checkpoint_tx
                        .removed_objects_pre_version()
                        .filter_map(|obj| obj.is_shared().then_some(IotaAddress::from(obj.id()))),
                );
                created_objects.extend(
                    checkpoint_tx
                        .output_objects
                        .into_iter()
                        .filter(|obj| obj.is_shared())
                        .filter_map(|obj| StoredObject::try_from(obj).ok()),
                );
            }
        }

        let checkpoint_timestamp = checkpoint.checkpoint_summary.timestamp_ms;

        LATEST_CHECKPOINT_UNIX_TIMESTAMP_MS
            .get_or_init(|| AtomicU64::new(0))
            .store(checkpoint_timestamp, std::sync::atomic::Ordering::SeqCst);

        if !created_objects.is_empty() {
            self.multi_insert_as_database_transactions(created_objects)?;
        }

        if !deleted_addresses.is_empty() {
            self.delete_objects(deleted_addresses)?;
        }

        METRICS
            .get()
            .expect("metrics global should be initialized")
            .last_checkpoint_indexed
            .set(checkpoint.checkpoint_summary.sequence_number as i64);

        Ok(())
    }
}
