// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::{Extension, Router, extract::Query, routing::get};
use diesel::{JoinOnDsl, prelude::*};
use iota_types::stardust::output::nft::NftOutput;
use serde::Serialize;
use tracing::error;

use crate::{
    impl_into_response,
    models::StoredObject,
    rest::{
        error::ApiError, extension::StardustExtension, extractors::custom_path::ExtractPath,
        routes::v1::PaginationParams,
    },
    schema::{expiration_unlock_conditions::dsl as conditions_dsl, objects::dsl as objects_dsl},
};

pub(crate) fn router() -> Router {
    Router::new().route("/nft/:address", get(nft))
}

async fn nft(
    ExtractPath(extracted_address): ExtractPath<iota_types::base_types::IotaAddress>,
    Query(pagination): Query<PaginationParams>,
    Extension(state): Extension<StardustExtension>,
) -> Result<NftResponse, ApiError> {
    let mut conn = state.connection_pool.get_connection().map_err(|e| {
        error!("Failed to get connection: {}", e);
        ApiError::ServiceUnavailable(format!("Failed to get connection: {}", e))
    })?;

    // Set default values for pagination if not provided
    let page = pagination.page.unwrap_or(1);
    let page_size = pagination.page_size.unwrap_or(10);

    // Calculate the offset
    let offset = (page - 1) * page_size;

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
        .limit(page_size as i64) // Limit the number of results
        .offset(offset as i64) // Skip the results for previous pages
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
        rest::{
            config::RestApiConfig, routes::v1::get_free_port_for_testing_only, spawn_rest_server,
        },
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
        let bind_port = get_free_port_for_testing_only().unwrap();
        let join_handle = spawn_rest_server(
            RestApiConfig {
                bind_port,
                ..Default::default()
            },
            pool,
            shutdown_rx,
        );

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/nft/{}",
            bind_port,
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

    #[tokio::test]
    async fn get_nft_objects_by_address() -> Result<(), anyhow::Error> {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .finish();

        let _ = tracing::subscriber::set_default(subscriber);

        let test_db = "stored_nft_object_address_filter_test.db";
        let pool = ConnectionPool::new_with_url(test_db, Default::default()).unwrap();
        pool.run_migrations().unwrap();
        let mut connection = pool.get_connection().unwrap();

        let owner_address: iota_types::base_types::IotaAddress = ObjectID::random().into();
        let other_address: iota_types::base_types::IotaAddress = ObjectID::random().into();

        // Populate the database with NFTs for two different addresses
        let mut inserted_nfts = vec![];

        for i in 0..2 {
            let nft_object_id = ObjectID::random();
            let nft_output = NftOutput {
                id: UID::new(nft_object_id),
                balance: Balance::new(100 + i),
                native_tokens: Bag::default(),
                expiration: Some(
                    iota_types::stardust::output::unlock_conditions::ExpirationUnlockCondition {
                        owner: owner_address.clone(),
                        return_address: owner_address.clone(),
                        unix_time: 100 + i as u32,
                    },
                ),
                storage_deposit_return: None,
                timelock: None,
            };

            let stored_object = StoredObject::new_nft_for_testing(nft_output.clone())?;

            insert_into(objects)
                .values(&stored_object)
                .execute(&mut connection)
                .unwrap();

            let unlock_condition = ExpirationUnlockCondition {
                owner: IotaAddress(owner_address.clone()),
                return_address: IotaAddress(owner_address.clone()),
                unix_time: 100 + i as i32,
                object_id: IotaAddress(nft_object_id.into()),
            };

            insert_into(expiration_unlock_conditions)
                .values(&unlock_condition)
                .execute(&mut connection)
                .unwrap();

            inserted_nfts.push(nft_output);
        }

        // Insert NFTs for the other address
        for i in 0..5 {
            let nft_object_id = ObjectID::random();
            let nft_output = NftOutput {
                id: UID::new(nft_object_id),
                balance: Balance::new(200 + i),
                native_tokens: Bag::default(),
                expiration: Some(
                    iota_types::stardust::output::unlock_conditions::ExpirationUnlockCondition {
                        owner: other_address.clone(),
                        return_address: other_address.clone(),
                        unix_time: 200 + i as u32,
                    },
                ),
                storage_deposit_return: None,
                timelock: None,
            };

            let stored_object = StoredObject::new_nft_for_testing(nft_output.clone())?;

            insert_into(objects)
                .values(&stored_object)
                .execute(&mut connection)
                .unwrap();

            let unlock_condition = ExpirationUnlockCondition {
                owner: IotaAddress(other_address.clone()),
                return_address: IotaAddress(other_address.clone()),
                unix_time: 200 + i as i32,
                object_id: IotaAddress(nft_object_id.into()),
            };

            insert_into(expiration_unlock_conditions)
                .values(&unlock_condition)
                .execute(&mut connection)
                .unwrap();
        }

        drop(connection);

        // Spawn the REST server
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let bind_port = get_free_port_for_testing_only().unwrap();
        let join_handle = spawn_rest_server(
            RestApiConfig {
                bind_port,
                ..Default::default()
            },
            pool,
            shutdown_rx,
        );

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // Fetch NFTs for `owner_address`
        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/nft/{}",
            bind_port,
            owner_address.to_string()
        ))
        .await?;

        let nft_outputs: Vec<NftOutput> = resp.json().await?;
        assert_eq!(nft_outputs.len(), 2);

        for (i, output) in nft_outputs.iter().enumerate() {
            assert_eq!(output, &inserted_nfts[i]);
        }

        // Fetch NFTs for `other_address`
        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/nft/{}",
            bind_port,
            other_address.to_string()
        ))
        .await?;

        let other_nft_outputs: Vec<NftOutput> = resp.json().await?;
        assert_eq!(other_nft_outputs.len(), 5);

        for output in other_nft_outputs {
            assert!(output.balance.value() >= 200); // Validate range for "other_address" NFTs
        }

        shutdown_tx.send(()).unwrap();
        join_handle.await.unwrap();

        // Clean up the test database
        std::fs::remove_file(test_db).unwrap();

        Ok(())
    }

    #[tokio::test]
    async fn test_pagination() -> Result<(), anyhow::Error> {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .finish();

        let _ = tracing::subscriber::set_default(subscriber);

        let test_db = "stored_nft_object_pagination_test.db";
        let pool = ConnectionPool::new_with_url(test_db, Default::default()).unwrap();
        pool.run_migrations().unwrap();
        let mut connection = pool.get_connection().unwrap();

        let owner_address: iota_types::base_types::IotaAddress = ObjectID::random().into();

        // Populate the database with multiple NFT objects
        let mut inserted_objects = vec![];
        for i in 0..15 {
            let nft_object_id = ObjectID::random();
            let nft_output = NftOutput {
                id: UID::new(nft_object_id),
                balance: Balance::new(100 + i),
                native_tokens: Bag::default(),
                expiration: Some(
                    iota_types::stardust::output::unlock_conditions::ExpirationUnlockCondition {
                        owner: owner_address.clone(),
                        return_address: owner_address.clone(),
                        unix_time: 100 + i as u32,
                    },
                ),
                storage_deposit_return: None,
                timelock: None,
            };

            let stored_object = StoredObject::new_nft_for_testing(nft_output.clone())?;

            insert_into(objects)
                .values(&stored_object)
                .execute(&mut connection)
                .unwrap();

            let unlock_condition = ExpirationUnlockCondition {
                owner: IotaAddress(owner_address.clone()),
                return_address: IotaAddress(owner_address.clone()),
                unix_time: 100 + i as i32,
                object_id: IotaAddress(nft_object_id.into()),
            };

            insert_into(expiration_unlock_conditions)
                .values(&unlock_condition)
                .execute(&mut connection)
                .unwrap();

            inserted_objects.push(nft_output);
        }

        drop(connection);

        // Spawn the REST server
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let bind_port = get_free_port_for_testing_only().unwrap();
        let join_handle = spawn_rest_server(
            RestApiConfig {
                bind_port,
                ..Default::default()
            },
            pool,
            shutdown_rx,
        );

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // Test first page
        let page = 1;
        let page_size = 5;
        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/nft/{}?page={}&page_size={}",
            bind_port,
            owner_address.to_string(),
            page,
            page_size
        ))
        .await?;

        let nft_outputs: Vec<NftOutput> = resp.json().await?;
        assert_eq!(nft_outputs.len(), page_size);
        assert_eq!(nft_outputs, inserted_objects[..page_size]);

        // Test second page
        let page = 2;
        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/nft/{}?page={}&page_size={}",
            bind_port,
            owner_address.to_string(),
            page,
            page_size
        ))
        .await?;

        let nft_outputs: Vec<NftOutput> = resp.json().await?;
        assert_eq!(nft_outputs.len(), page_size);
        assert_eq!(nft_outputs, inserted_objects[page_size..2 * page_size]);

        // Test third page (remaining items)
        let page = 3;
        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/nft/{}?page={}&page_size={}",
            bind_port,
            owner_address.to_string(),
            page,
            page_size
        ))
        .await?;

        let nft_outputs: Vec<NftOutput> = resp.json().await?;
        assert_eq!(nft_outputs.len(), 5); // Remaining items
        assert_eq!(nft_outputs, inserted_objects[2 * page_size..]);

        shutdown_tx.send(()).unwrap();

        join_handle.await.unwrap();

        // Clean up the test database
        std::fs::remove_file(test_db).unwrap();

        Ok(())
    }
}
