//! Workers are responsible for synching data from Fullnode into the Indexer, It
//! can apply filtering logic to sotre only the desired data if necessary into a
//! local or remote storage

use std::ops::Deref;

use async_trait::async_trait;
use diesel::{Connection, RunQueryDsl, insert_into};
use iota_data_ingestion_core::Worker;
use iota_types::{
    base_types::ObjectID,
    full_checkpoint_content::{CheckpointData, CheckpointTransaction},
    object::Object,
    stardust::output::{BasicOutput, NftOutput},
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

    /// Check if the provided object suffice the following requirements in order
    /// to be stored
    /// - Is a shared object
    /// - Its struct tag does match the targeted package id
    pub fn object_belongs_to_package(&self, obj: &Object) -> bool {
        self.package_ids.iter().any(|package_id| {
            obj.is_shared()
                && (obj
                    .struct_tag()
                    .map(|struct_tag| package_id.deref() == &struct_tag.address)
                    .unwrap_or_default())
        })
    }

    /// Insert multiple `ExpirationUnlockCondition` and `StoredObject` objects
    /// from a `CheckpointData` and wrap the queries into a database transaction
    fn multi_insert_transaction(
        &self,
        expiration: impl AsRef<[ExpirationUnlockCondition]>,
        stored_objects: impl AsRef<[StoredObject]>,
    ) -> anyhow::Result<()> {
        let mut pool = self.pool.get_connection()?;

        pool.transaction::<_, anyhow::Error, _>(|conn| {
            insert_into(expiration_unlock_conditions)
                .values(expiration.as_ref())
                .execute(conn)?;

            insert_into(objects)
                .values(stored_objects.as_ref())
                .execute(conn)?;

            Ok(())
        })
    }
}

#[async_trait]
impl Worker for CheckpointWorker {
    async fn process_checkpoint(&self, checkpoint: CheckpointData) -> anyhow::Result<()> {
        // Genesis transaction have 0 input objects and N objects as output ones,
        // for implementation simplicity we verify in transactions's output objects
        // vector if there are any objects related to interested package

        let (expiration, stored_objects) = checkpoint
            .transactions
            .into_iter()
            .flat_map(|tx: CheckpointTransaction| {
                tx.output_objects
                    .into_iter()
                    .filter_map(|obj: Object| {
                        self.object_belongs_to_package(&obj)
                            .then(|| StoredObject::try_from(obj))
                    })
                    .collect::<anyhow::Result<Vec<StoredObject>>>()
            })
            .try_fold(
                (Vec::new(), Vec::new()),
                |(mut expiration_acc, mut stored_obj_acc), stored_objects| {
                    stored_objects
                        .into_iter()
                        .try_for_each(|stored_obj: StoredObject| {
                            match stored_obj.object_type {
                                ObjectType::Basic => {
                                    let basic = BasicOutput::try_from(stored_obj.clone())?;
                                    if basic.expiration.is_some() {
                                        stored_obj_acc.push(stored_obj);
                                        expiration_acc
                                            .push(ExpirationUnlockCondition::try_from(basic)?);
                                    }
                                }
                                ObjectType::Nft => {
                                    let nft = NftOutput::try_from(stored_obj.clone())?;
                                    if nft.expiration.is_some() {
                                        stored_obj_acc.push(stored_obj);
                                        expiration_acc
                                            .push(ExpirationUnlockCondition::try_from(nft)?);
                                    }
                                }
                            };
                            Ok::<_, anyhow::Error>(())
                        })
                        .map(|_| (expiration_acc, stored_obj_acc))
                },
            )?;

        self.multi_insert_transaction(expiration, stored_objects)?;

        Ok(())
    }
}
