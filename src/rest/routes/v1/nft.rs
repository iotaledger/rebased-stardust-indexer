// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::{Extension, Router, routing::get};
use diesel::{JoinOnDsl, prelude::*};
use iota_types::stardust::output::nft::NftOutput;
use serde::Serialize;
use tracing::error;

use crate::{
    impl_into_response,
    models::StoredObject,
    rest::{error::ApiError, extension::StardustExtension, extractors::custom_path::ExtractPath},
    schema::{expiration_unlock_conditions::dsl as conditions_dsl, objects::dsl as objects_dsl},
};

pub(crate) fn router() -> Router {
    Router::new().route("/nft/:address", get(nft))
}

async fn nft(
    ExtractPath(extracted_address): ExtractPath<iota_types::base_types::IotaAddress>,
    Extension(state): Extension<StardustExtension>,
) -> Result<NftResponse, ApiError> {
    let mut conn = state.connection_pool.get_connection().map_err(|e| {
        error!("Failed to get connection: {}", e);
        ApiError::ServiceUnavailable(format!("Failed to get connection: {}", e))
    })?;

    // Query to find objects with matching expiration_unlock_conditions
    let stored_objects = objects_dsl::objects
        .inner_join(
            conditions_dsl::expiration_unlock_conditions
                .on(objects_dsl::id.eq(conditions_dsl::object_id)),
        )
        .select((
            objects_dsl::id,
            objects_dsl::object_type,
            objects_dsl::contents,
        ))
        .filter(
            conditions_dsl::owner
                .eq(extracted_address.to_vec())
                .or(conditions_dsl::return_address.eq(extracted_address.to_vec())),
        )
        .load::<StoredObject>(&mut conn)
        .map_err(|err| {
            error!("Failed to load stored objects: {}", err);
            ApiError::InternalServerError
        })?;

    let nft_outputs: Vec<NftOutput> = stored_objects
        .into_iter()
        .filter_map(|stored_object| NftOutput::try_from(stored_object.clone()).ok())
        .collect();

    Ok(NftResponse(nft_outputs))
}

#[derive(Clone, Debug, Serialize)]
struct NftResponse(Vec<NftOutput>);

impl_into_response!(NftResponse);

#[cfg(test)]
mod tests {
    use diesel::{RunQueryDsl, insert_into};
    use iota_types::{
        balance::Balance, base_types::ObjectID, collection_types::Bag, id::UID,
        stardust::output::nft::NftOutput,
    };
    use tokio::sync::oneshot;
    use tracing::Level;
    use tracing_subscriber::FmtSubscriber;

    use crate::{
        db::ConnectionPool,
        models::{ExpirationUnlockCondition, IotaAddress, StoredObject},
        rest::{config::RestApiConfig, spawn_rest_server},
        schema::{
            expiration_unlock_conditions::dsl::expiration_unlock_conditions, objects::dsl::*,
        },
    };

    #[tokio::test]
    async fn get_nft_object() -> Result<(), anyhow::Error> {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .finish();

        let _ = tracing::subscriber::set_default(subscriber);

        let test_db = "stored_nft_object_round_trip.db";
        let pool = ConnectionPool::new_with_url(test_db, Default::default()).unwrap();
        pool.run_migrations().unwrap();
        let mut connection = pool.get_connection().unwrap();

        // Populate the database with an NFT object
        let owner_address: iota_types::base_types::IotaAddress = ObjectID::random().into();
        let nft_object_id = ObjectID::random();
        let nft_output = NftOutput {
            id: UID::new(nft_object_id),
            balance: Balance::new(100),
            native_tokens: Bag::default(),
            expiration: Some(
                iota_types::stardust::output::unlock_conditions::ExpirationUnlockCondition {
                    owner: owner_address.clone(),
                    return_address: owner_address.clone(),
                    unix_time: 100,
                },
            ),
            storage_deposit_return: None,
            timelock: None,
        };

        let stored_object = StoredObject::new_nft_for_testing(nft_output.clone())?;

        let rows_inserted = insert_into(objects)
            .values(&vec![stored_object.clone()])
            .execute(&mut connection)
            .unwrap();
        assert_eq!(rows_inserted, 1);

        // Insert the corresponding entry into the `expiration_unlock_conditions` table
        let unlock_condition = ExpirationUnlockCondition {
            owner: IotaAddress(owner_address.clone()),
            return_address: IotaAddress(owner_address),
            unix_time: 100,
            object_id: IotaAddress(nft_object_id.into()),
        };

        let rows_inserted_conditions = insert_into(expiration_unlock_conditions)
            .values(&unlock_condition)
            .execute(&mut connection)
            .unwrap();
        assert_eq!(rows_inserted_conditions, 1);

        drop(connection);

        // Spawn the REST server
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let join_handle = spawn_rest_server(
            RestApiConfig {
                bind_port: 3002,
                ..Default::default()
            },
            pool,
            shutdown_rx,
        );

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let resp = reqwest::get(format!(
            "http://127.0.0.1:3002/v1/nft/{}",
            owner_address.to_string()
        ))
        .await?;

        // Parse all NftOutput objects from the response
        let nft_outputs: Vec<NftOutput> = resp.json().await?;
        assert_eq!(nft_outputs.len(), 1);

        // Check if the NftOutput object is the same as the one we inserted
        assert_eq!(nft_outputs[0], nft_output);

        shutdown_tx.send(()).unwrap();

        join_handle.await.unwrap();

        // Clean up the test database
        std::fs::remove_file(test_db).unwrap();

        Ok(())
    }
}
