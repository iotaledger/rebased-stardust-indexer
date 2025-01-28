// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::{Extension, Router, extract::Query, routing::get};
use tracing::error;

use crate::{
    models::{ObjectType, StoredObject},
    rest::{
        State,
        error::ApiError,
        extractors::Path,
        routes::v1::{
            PaginationParams, fetch_stored_objects,
            responses::{NftOutput, NftOutputVec},
        },
    },
};

pub(crate) fn router() -> Router {
    Router::new()
        .route("/nft/:address", get(nft))
        .route("/nft/resolved/:address", get(resolved))
}

/// Get the `BasicOutput`s owned by the address
#[utoipa::path(
    get,
    path = "/v1/nft/{address}",
    responses(
        (status = 200, description = "Successful request", body = NftOutputVec),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal server error"),
        (status = 503, description = "Service unavailable"),
        (status = 403, description = "Forbidden")
    ),
    params(
        ("address" = String, Path, description = "The hex address to fetch the NFT outputs for"),
        ("page" = Option<u32>, Query, description = "Page number for pagination"),
        ("limit" = Option<u32>, Query, description = "Number of items per page for pagination")
    )
)]
async fn nft(
    Path(address): Path<iota_types::base_types::IotaAddress>,
    Query(pagination): Query<PaginationParams>,
    Extension(state): Extension<State>,
) -> Result<NftOutputVec, ApiError> {
    let stored_objects = fetch_stored_objects(address, pagination, state, ObjectType::Nft, false)?;
    let nft_outputs = stored_objects_to_nft_outputs(stored_objects)?;
    Ok(NftOutputVec(nft_outputs))
}

/// Get the `NftOutput`s owned by the address considering resolved expiration
/// unlock condition.
#[utoipa::path(
    get,
    path = "/v1/nft/resolved/{address}",
    responses(
        (status = 200, description = "Successful request", body = NftOutputVec),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal server error"),
        (status = 503, description = "Service unavailable"),
        (status = 403, description = "Forbidden")
    ),
    params(
        ("address" = String, Path, description = "The hex address to fetch the NFT outputs for"),
        ("page" = Option<u32>, Query, description = "Page number for pagination"),
        ("limit" = Option<u32>, Query, description = "Number of items per page for pagination")
    )
)]
async fn resolved(
    Path(address): Path<iota_types::base_types::IotaAddress>,
    Query(pagination): Query<PaginationParams>,
    Extension(state): Extension<State>,
) -> Result<NftOutputVec, ApiError> {
    let stored_objects = fetch_stored_objects(address, pagination, state, ObjectType::Nft, true)?;
    let nft_outputs = stored_objects_to_nft_outputs(stored_objects)?;
    Ok(NftOutputVec(nft_outputs))
}

fn stored_objects_to_nft_outputs(
    stored_objects: Vec<StoredObject>,
) -> Result<Vec<NftOutput>, ApiError> {
    stored_objects
        .into_iter()
        .map(|stored_object| {
            iota_types::stardust::output::nft::NftOutput::try_from(stored_object)
                .map(NftOutput::from)
                .map_err(|e| {
                    error!("failed to convert stored object to NFT output: {}", e);
                    ApiError::InternalServerError
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diesel::{RunQueryDsl, insert_into};
    use iota_types::{balance::Balance, base_types::ObjectID, collection_types::Bag, id::UID};
    use prometheus::Registry;
    use tracing::Level;
    use tracing_subscriber::FmtSubscriber;

    use crate::{
        db::{ConnectionPool, Name, PoolConnection},
        models::{ExpirationUnlockCondition, IotaAddress, StoredObject},
        rest::{
            routes::{
                get_free_port_for_testing_only,
                v1::{ensure_checkpoint_is_set, nft::NftOutput},
            },
            spawn_rest_server,
        },
        schema::{
            expiration_unlock_conditions::dsl::expiration_unlock_conditions, objects::dsl::*,
        },
    };

    #[tokio::test]
    async fn get_nft_objects_by_address() -> Result<(), anyhow::Error> {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .finish();

        let _ = tracing::subscriber::set_default(subscriber);

        let test_db = "stored_nft_object_address_filter_test.db";
        let pool =
            ConnectionPool::new_with_url(test_db, Default::default(), Name::Objects).unwrap();
        pool.run_migrations().unwrap();
        let mut connection = pool.get_connection().unwrap();

        let owner_address: iota_types::base_types::IotaAddress = ObjectID::random().into();
        let other_address: iota_types::base_types::IotaAddress = ObjectID::random().into();

        // Populate the database with NFTs for two different addresses
        let mut inserted_nfts = vec![];

        for i in 0..2 {
            let nft_output = create_and_insert_nft_output(
                &mut connection,
                owner_address.clone(),
                100 + i,
                100 + i as u32,
            )?;
            let serialized_nft_output = NftOutput::from(nft_output);
            inserted_nfts.push(serialized_nft_output);
        }

        // Insert NFTs for the other address
        for i in 0..5 {
            let _ = create_and_insert_nft_output(
                &mut connection,
                other_address.clone(),
                200 + i,
                200 + i as u32,
            )?;
        }

        drop(connection);

        // Spawn the REST server
        let cancel_token = tokio_util::sync::CancellationToken::new();
        let bind_port = get_free_port_for_testing_only().unwrap();
        let handle = spawn_rest_server(
            format!("127.0.0.1:{}", bind_port).parse().unwrap(),
            pool,
            cancel_token.clone(),
            Arc::new(Registry::default()),
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
            assert!(output.balance.value >= 200); // Validate range for "other_address" NFTs
        }

        cancel_token.cancel();
        handle.await.unwrap();

        // Clean up the test database
        std::fs::remove_file(test_db).unwrap();

        Ok(())
    }

    #[tokio::test]
    async fn get_nft_objects_by_address_resolved() -> Result<(), anyhow::Error> {
        ensure_checkpoint_is_set();

        let sub = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .finish();
        let _ = tracing::subscriber::set_default(sub);

        let test_db = "stored_nft_object_address_filter_resolved_test.db";
        let pool =
            ConnectionPool::new_with_url(test_db, Default::default(), Name::Objects).unwrap();
        pool.run_migrations().unwrap();
        let mut conn = pool.get_connection().unwrap();

        // two different addresses
        let owner_addr: iota_types::base_types::IotaAddress = ObjectID::random().into();
        let return_addr: iota_types::base_types::IotaAddress = ObjectID::random().into();

        // outputs unexpired for owner
        let mut unexpired = vec![];
        let big_ts = 999_999_999;
        for i in 0..3 {
            let out = create_and_insert_nft_output(&mut conn, owner_addr.clone(), 100 + i, big_ts)?;
            unexpired.push(NftOutput::from(out));
        }

        // outputs expired for return address
        let mut expired = vec![];
        let small_ts = 100;
        for i in 0..2 {
            let out =
                create_and_insert_nft_output(&mut conn, return_addr.clone(), 200 + i, small_ts)?;
            expired.push(NftOutput::from(out));
        }

        // irrelevant outputs
        let third_addr: iota_types::base_types::IotaAddress = ObjectID::random().into();
        for i in 0..3 {
            let _ = create_and_insert_nft_output(&mut conn, third_addr.clone(), 300 + i, big_ts)?;
        }

        drop(conn);

        let cancel_token = tokio_util::sync::CancellationToken::new();
        let port = get_free_port_for_testing_only().unwrap();
        let handle = spawn_rest_server(
            format!("127.0.0.1:{port}").parse().unwrap(),
            pool,
            cancel_token.clone(),
            Arc::new(Registry::default()),
        );

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // check unexpired
        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/nft/resolved/{}",
            port, owner_addr
        ))
        .await?;
        let list: Vec<NftOutput> = resp.json().await?;
        assert_eq!(list.len(), unexpired.len());
        assert_eq!(list, unexpired);

        // check expired
        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/nft/resolved/{}",
            port, return_addr
        ))
        .await?;
        let list: Vec<NftOutput> = resp.json().await?;
        assert_eq!(list.len(), expired.len());
        assert_eq!(list, expired);

        cancel_token.cancel();
        handle.await.unwrap();
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
        let pool =
            ConnectionPool::new_with_url(test_db, Default::default(), Name::Objects).unwrap();
        pool.run_migrations().unwrap();
        let mut connection = pool.get_connection().unwrap();

        let owner_address: iota_types::base_types::IotaAddress = ObjectID::random().into();

        // Populate the database with multiple NFT objects
        let mut inserted_objects = vec![];
        for i in 0..15 {
            let nft_output = create_and_insert_nft_output(
                &mut connection,
                owner_address.clone(),
                100 + i,
                100 + i as u32,
            )?;
            let serialized_nft_output = NftOutput::from(nft_output);
            inserted_objects.push(serialized_nft_output);
        }

        drop(connection);

        // Spawn the REST server
        let cancel_token = tokio_util::sync::CancellationToken::new();
        let bind_port = get_free_port_for_testing_only().unwrap();
        let handle = spawn_rest_server(
            format!("127.0.0.1:{}", bind_port).parse().unwrap(),
            pool,
            cancel_token.clone(),
            Arc::new(Registry::default()),
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

        cancel_token.cancel();

        handle.await.unwrap();

        // Clean up the test database
        std::fs::remove_file(test_db).unwrap();

        Ok(())
    }
    fn create_and_insert_nft_output(
        connection: &mut PoolConnection,
        owner_address: iota_types::base_types::IotaAddress,
        balance: u64,
        unix_time: u32,
    ) -> Result<iota_types::stardust::output::nft::NftOutput, anyhow::Error> {
        let nft_object_id = ObjectID::random();
        let nft_output = iota_types::stardust::output::nft::NftOutput {
            id: UID::new(nft_object_id),
            balance: Balance::new(balance),
            native_tokens: Bag::default(),
            expiration: Some(
                iota_types::stardust::output::unlock_conditions::ExpirationUnlockCondition {
                    owner: owner_address.clone(),
                    return_address: owner_address.clone(),
                    unix_time,
                },
            ),
            storage_deposit_return: None,
            timelock: None,
        };

        let stored_object = StoredObject::new_nft_for_testing(nft_output.clone())?;

        insert_into(objects)
            .values(&stored_object)
            .execute(connection)
            .unwrap();

        let unlock_condition = ExpirationUnlockCondition {
            owner: IotaAddress(owner_address.clone()),
            return_address: IotaAddress(owner_address.clone()),
            unix_time: unix_time as i64,
            object_id: IotaAddress(nft_object_id.into()),
        };

        insert_into(expiration_unlock_conditions)
            .values(&unlock_condition)
            .execute(connection)
            .unwrap();

        Ok(nft_output)
    }
}
