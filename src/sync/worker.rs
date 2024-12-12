//! Workers are responsible for syncing data from Fullnode into the Indexer, It
//! can apply filtering logic to store only the desired data if necessary into a
//! local or remote storage

use axum::async_trait;
use diesel::{RunQueryDsl, insert_into};
use iota_data_ingestion_core::Worker;
use iota_types::{
    base_types::ObjectID,
    full_checkpoint_content::{CheckpointData, CheckpointTransaction},
    object::Object,
    stardust::output::{BasicOutput, NftOutput},
    transaction::TransactionDataAPI,
};

use crate::{
    db::ConnectionPool,
    models::{ExpirationUnlockCondition, ObjectType, StoredObject},
    schema::{expiration_unlock_conditions::dsl::*, objects::dsl::*},
};

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

    /// Check if the provided `Object` does belong to the package id
    fn object_belongs_to_package(&self, obj: &Object) -> bool {
        obj.is_package() && (obj.id() == self.package_id)
    }

    // Check if the `CheckpointTransaction` is a genesis transaction or contains
    // input objects belonging to the package ID.
    fn tx_contains_relevant_objects(&self, checkpoint_tx: &CheckpointTransaction) -> bool {
        checkpoint_tx
            .transaction
            .intent_message()
            .value
            .is_genesis_tx()
            || checkpoint_tx
                .input_objects
                .iter()
                .any(|obj| self.object_belongs_to_package(obj))
    }

    /// Insert multiple `StoredObject` into database
    fn insert_stored_objects(&self, stored_objects: &[StoredObject]) -> anyhow::Result<()> {
        let mut pool = self.pool.get_connection()?;

        insert_into(objects)
            .values(stored_objects)
            .execute(&mut pool)?;

        Ok(())
    }

    /// Insert multiple `ExpirationUnlockCondition` into database
    fn insert_expiration_unlock_conditions(
        &self,
        expiration: &[ExpirationUnlockCondition],
    ) -> anyhow::Result<()> {
        let mut pool = self.pool.get_connection()?;

        insert_into(expiration_unlock_conditions)
            .values(expiration)
            .execute(&mut pool)?;

        Ok(())
    }
}

#[async_trait]
impl Worker for CheckpointWorker {
    async fn process_checkpoint(&self, checkpoint: CheckpointData) -> anyhow::Result<()> {
        let mut stored_objects = Vec::new();
        for checkpoint_tx in checkpoint.transactions.into_iter() {
            if self.tx_contains_relevant_objects(&checkpoint_tx) {
                stored_objects.extend(
                    checkpoint_tx
                        .output_objects
                        .into_iter()
                        .filter(|obj| obj.is_shared())
                        .filter_map(|obj| StoredObject::try_from(obj).ok()),
                );
            }
        }

        self.insert_stored_objects(&stored_objects)?;

        let expiration_uc = stored_objects
            .into_iter()
            .map(|stored_object| match stored_object.object_type {
                ObjectType::Basic => BasicOutput::try_from(stored_object)
                    .and_then(ExpirationUnlockCondition::try_from),
                ObjectType::Nft => {
                    NftOutput::try_from(stored_object).and_then(ExpirationUnlockCondition::try_from)
                }
            })
            .collect::<anyhow::Result<Vec<ExpirationUnlockCondition>>>()?;

        self.insert_expiration_unlock_conditions(&expiration_uc)?;

        Ok(())
    }
}
