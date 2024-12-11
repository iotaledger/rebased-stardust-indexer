// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::{Extension, Router, extract::Query, routing::get};
use serde::{Deserialize, Serialize};
use tracing::error;
use utoipa::ToSchema;

use crate::{
    impl_into_response,
    models::ObjectType,
    rest::{
        State,
        error::ApiError,
        extractors::Path,
        routes::v1::{PaginationParams, fetch_stored_objects},
    },
};

pub(crate) fn router() -> Router {
    Router::new().route("/nft/:address", get(nft))
}

/// Get the `BasicOutput`s owned by the address
#[utoipa::path(
    get,
    path = "/v1/nft/{address}",
    responses(
        (status = 200, description = "Successful request", body = NftResponse),
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
) -> Result<NftResponse, ApiError> {
    let stored_objects = fetch_stored_objects(address, pagination, state, ObjectType::Nft)?;

    let nft_outputs: Vec<NftOutput> = stored_objects
        .into_iter()
        .map(|x| {
            iota_types::stardust::output::nft::NftOutput::try_from(x)
                .map(NftOutput::from)
                .map_err(|e| {
                    error!("failed to convert stored object to NFT output: {}", e);
                    ApiError::InternalServerError
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(NftResponse(nft_outputs))
}

#[derive(Clone, Debug, Serialize, ToSchema)]
struct NftResponse(Vec<NftOutput>);
impl_into_response!(NftResponse);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
struct NftOutput {
    id: String,
    balance: Balance,
    native_tokens: Bag,
    storage_deposit_return: Option<StorageDepositReturn>,
    timelock: Option<Timelock>,
    expiration: Option<Expiration>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
struct Balance {
    value: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
struct Bag {
    id: String,
    size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
struct StorageDepositReturn {
    return_address: String,
    return_amount: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
struct Timelock {
    unix_time: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
struct Expiration {
    owner: String,
    return_address: String,
    unix_time: u64,
}

impl From<iota_types::stardust::output::nft::NftOutput> for NftOutput {
    fn from(output: iota_types::stardust::output::nft::NftOutput) -> Self {
        Self {
            id: output.id.object_id().to_string(),
            balance: Balance {
                value: output.balance.value(),
            },
            native_tokens: Bag {
                id: output.native_tokens.id.object_id().to_string(),
                size: output.native_tokens.size,
            },
            storage_deposit_return: output.storage_deposit_return.map(|x| StorageDepositReturn {
                return_address: x.return_address.to_string(),
                return_amount: x.return_amount,
            }),
            timelock: output.timelock.map(|x| Timelock {
                unix_time: x.unix_time as u64,
            }),
            expiration: output.expiration.map(|x| Expiration {
                owner: x.owner.to_string(),
                return_address: x.return_address.to_string(),
                unix_time: x.unix_time as u64,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use diesel::{RunQueryDsl, insert_into};
    use iota_types::{balance::Balance, base_types::ObjectID, collection_types::Bag, id::UID};
    use tracing::Level;
    use tracing_subscriber::FmtSubscriber;

    use crate::{
        db::{ConnectionPool, PoolConnection},
        models::{ExpirationUnlockCondition, IotaAddress, StoredObject},
        rest::{
            routes::v1::{get_free_port_for_testing_only, nft::NftOutput},
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
        let pool = ConnectionPool::new_with_url(test_db, Default::default()).unwrap();
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
