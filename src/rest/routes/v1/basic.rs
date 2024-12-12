// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::{Extension, Router, extract::Query, routing::get};
use iota_types::stardust::output::basic::BasicOutput;
use serde::Serialize;
use tracing::error;

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
    Router::new().route("/basic/:address", get(basic))
}

async fn basic(
    Path(address): Path<iota_types::base_types::IotaAddress>,
    Query(pagination): Query<PaginationParams>,
    Extension(state): Extension<State>,
) -> Result<BasicResponse, ApiError> {
    let stored_objects = fetch_stored_objects(address, pagination, state, ObjectType::Basic)?;

    let basic_outputs: Vec<BasicOutput> = stored_objects
        .into_iter()
        .map(BasicOutput::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            error!("failed to convert stored object to NFT output: {}", e);
            ApiError::InternalServerError
        })?;

    Ok(BasicResponse(basic_outputs))
}

#[derive(Clone, Debug, Serialize)]
struct BasicResponse(Vec<BasicOutput>);
impl_into_response!(BasicResponse);

#[cfg(test)]
mod tests {
    use diesel::{RunQueryDsl, insert_into};
    use iota_types::{
        balance::Balance, base_types::ObjectID, collection_types::Bag, id::UID,
        stardust::output::basic::BasicOutput,
    };
    use tokio_util::sync::CancellationToken;
    use tracing::Level;
    use tracing_subscriber::FmtSubscriber;

    use crate::{
        db::{ConnectionPool, PoolConnection},
        models::{ExpirationUnlockCondition, IotaAddress, StoredObject},
        rest::{routes::v1::get_free_port_for_testing_only, spawn_rest_server},
        schema::{
            expiration_unlock_conditions::dsl::expiration_unlock_conditions, objects::dsl::*,
        },
    };

    #[tokio::test]
    async fn get_basic_objects_by_address() -> Result<(), anyhow::Error> {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .finish();

        let _ = tracing::subscriber::set_default(subscriber);

        let test_db = "stored_basic_object_address_filter_test.db";
        let pool = ConnectionPool::new_with_url(test_db, Default::default()).unwrap();
        pool.run_migrations().unwrap();
        let mut connection = pool.get_connection().unwrap();

        let owner_address: iota_types::base_types::IotaAddress = ObjectID::random().into();
        let other_address: iota_types::base_types::IotaAddress = ObjectID::random().into();

        // Populate the database with objects for two different addresses
        let mut inserted_objects = vec![];

        for i in 0..2 {
            let basic_output = create_and_insert_basic_output(
                &mut connection,
                owner_address.clone(),
                100 + i,
                100 + i as u32,
            )?;
            inserted_objects.push(basic_output);
        }

        // Insert objects for the other address
        for i in 0..5 {
            let _ = create_and_insert_basic_output(
                &mut connection,
                other_address.clone(),
                200 + i,
                200 + i as u32,
            )?;
        }

        drop(connection);

        // Spawn the REST server
        let cancel_token = CancellationToken::new();
        let bind_port = get_free_port_for_testing_only().unwrap();
        let handle = spawn_rest_server(
            format!("127.0.0.1:{}", bind_port).parse().unwrap(),
            pool,
            cancel_token.clone(),
        );

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // Fetch objects for `owner_address`
        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/basic/{}",
            bind_port,
            owner_address.to_string()
        ))
        .await?;

        let basic_outputs: Vec<BasicOutput> = resp.json().await?;
        assert_eq!(basic_outputs.len(), 2);

        for (i, output) in basic_outputs.iter().enumerate() {
            assert_eq!(output, &inserted_objects[i]);
        }

        // Fetch objects for `other_address`
        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/basic/{}",
            bind_port,
            other_address.to_string()
        ))
        .await?;

        let other_basic_outputs: Vec<BasicOutput> = resp.json().await?;
        assert_eq!(other_basic_outputs.len(), 5);

        for output in other_basic_outputs {
            assert!(output.balance.value() >= 200); // Validate range for "other_address" objects
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

        let test_db = "stored_pagination_test.db";
        let pool = ConnectionPool::new_with_url(test_db, Default::default()).unwrap();
        pool.run_migrations().unwrap();
        let mut connection = pool.get_connection().unwrap();

        let owner_address: iota_types::base_types::IotaAddress = ObjectID::random().into();

        // Populate the database with multiple basic objects
        let mut inserted_objects = vec![];
        for i in 0..15 {
            let basic_output = create_and_insert_basic_output(
                &mut connection,
                owner_address.clone(),
                100 + i,
                100 + i as u32,
            )?;
            inserted_objects.push(basic_output);
        }

        drop(connection);

        // Spawn the REST server
        let cancel_token = CancellationToken::new();
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
            "http://127.0.0.1:{}/v1/basic/{}?page={}&page_size={}",
            bind_port,
            owner_address.to_string(),
            page,
            page_size
        ))
        .await?;

        let basic_outputs: Vec<BasicOutput> = resp.json().await?;
        assert_eq!(basic_outputs.len(), page_size);
        assert_eq!(basic_outputs, inserted_objects[..page_size]);

        // Test second page
        let page = 2;
        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/basic/{}?page={}&page_size={}",
            bind_port,
            owner_address.to_string(),
            page,
            page_size
        ))
        .await?;

        let basic_outputs: Vec<BasicOutput> = resp.json().await?;
        assert_eq!(basic_outputs.len(), page_size);
        assert_eq!(basic_outputs, inserted_objects[page_size..2 * page_size]);

        // Test third page (remaining items)
        let page = 3;
        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/basic/{}?page={}&page_size={}",
            bind_port,
            owner_address.to_string(),
            page,
            page_size
        ))
        .await?;

        let basic_outputs: Vec<BasicOutput> = resp.json().await?;
        assert_eq!(basic_outputs.len(), 5);
        assert_eq!(basic_outputs, inserted_objects[2 * page_size..]);

        cancel_token.cancel();

        handle.await.unwrap();

        // Clean up the test database
        std::fs::remove_file(test_db).unwrap();

        Ok(())
    }

    fn create_and_insert_basic_output(
        connection: &mut PoolConnection,
        owner_address: iota_types::base_types::IotaAddress,
        balance: u64,
        unix_time: u32,
    ) -> Result<BasicOutput, anyhow::Error> {
        let basic_object_id = ObjectID::random();
        let basic_output = BasicOutput {
            id: UID::new(basic_object_id),
            balance: Balance::new(balance),
            native_tokens: Bag::default(),
            storage_deposit_return: None,
            timelock: None,
            expiration: Some(
                iota_types::stardust::output::unlock_conditions::ExpirationUnlockCondition {
                    owner: owner_address.clone(),
                    return_address: owner_address.clone(),
                    unix_time,
                },
            ),
            metadata: None,
            tag: None,
            sender: None,
        };

        let stored_object = StoredObject::new_basic_for_testing(basic_output.clone())?;

        insert_into(objects)
            .values(&stored_object)
            .execute(connection)
            .unwrap();

        let unlock_condition = ExpirationUnlockCondition {
            owner: IotaAddress(owner_address.clone()),
            return_address: IotaAddress(owner_address.clone()),
            unix_time: unix_time as i64,
            object_id: IotaAddress(basic_object_id.into()),
        };

        insert_into(expiration_unlock_conditions)
            .values(&unlock_condition)
            .execute(connection)
            .unwrap();

        Ok(basic_output)
    }
}
