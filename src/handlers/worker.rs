//! Workers are responsible for syncing data from Fullnode into the Indexer, It
//! can apply filtering logic to store only the desired data if necessary into a
//! local or remote storage

use axum::async_trait;
use diesel::{RunQueryDsl, insert_into};
use iota_data_ingestion_core::Worker;
use iota_types::{
    base_types::ObjectID,
    full_checkpoint_content::CheckpointData,
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
    package_ids: Vec<ObjectID>,
}

impl CheckpointWorker {
    pub(crate) fn new(pool: ConnectionPool, package_ids: Vec<ObjectID>) -> Self {
        Self { pool, package_ids }
    }

    /// Insert multiple `StoredObject` into database
    fn insert_stored_objects(
        &self,
        stored_objects: impl AsRef<[StoredObject]>,
    ) -> anyhow::Result<()> {
        let mut pool = self.pool.get_connection()?;

        insert_into(objects)
            .values(stored_objects.as_ref())
            .execute(&mut pool)?;

        Ok(())
    }

    /// Insert multiple `ExpirationUnlockCondition` into database
    fn insert_expiration_unlock_conditions(
        &self,
        expiration: impl AsRef<[ExpirationUnlockCondition]>,
    ) -> anyhow::Result<()> {
        let mut pool = self.pool.get_connection()?;

        insert_into(expiration_unlock_conditions)
            .values(expiration.as_ref())
            .execute(&mut pool)?;

        Ok(())
    }
}

#[async_trait]
impl Worker for CheckpointWorker {
    async fn process_checkpoint(&self, checkpoint: CheckpointData) -> anyhow::Result<()> {
        let stored_objects =
            checkpoint
                .transactions
                .into_iter()
                .fold(Vec::new(), |mut stored_objects, tx| {
                    let object_belongs_to_package =
                        tx.transaction.intent_message().value.is_genesis_tx()
                            || tx.input_objects.iter().any(|obj| {
                                obj.is_package()
                                    && self
                                        .package_ids
                                        .iter()
                                        .any(|package_id| &obj.id() == package_id)
                            });

                    if object_belongs_to_package {
                        stored_objects.extend(
                            tx.output_objects
                                .into_iter()
                                .filter(|obj| obj.is_shared())
                                .filter_map(|obj| StoredObject::try_from(obj).ok()),
                        );
                    }

                    stored_objects
                });

        self.insert_stored_objects(&stored_objects)?;

        let expiration_uc = stored_objects
            .into_iter()
            .filter_map(|stored_object| match stored_object.object_type {
                ObjectType::Basic => BasicOutput::try_from(stored_object)
                    .ok()
                    .and_then(|basic| ExpirationUnlockCondition::try_from(basic).ok()),
                ObjectType::Nft => NftOutput::try_from(stored_object)
                    .ok()
                    .and_then(|nft| ExpirationUnlockCondition::try_from(nft).ok()),
            })
            .collect::<Vec<ExpirationUnlockCondition>>();

        self.insert_expiration_unlock_conditions(expiration_uc)?;

        Ok(())
    }
}
