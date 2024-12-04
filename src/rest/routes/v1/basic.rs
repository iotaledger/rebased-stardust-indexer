// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::{Extension, Router, routing::get};
use diesel::{JoinOnDsl, prelude::*};
use iota_types::stardust::output::basic::BasicOutput;
use serde::Serialize;
use tracing::error;

use crate::{
    impl_into_response,
    models::StoredObject,
    rest::{error::ApiError, extension::StardustExtension, extractors::custom_path::ExtractPath},
    schema::{
        expiration_unlock_conditions::dsl as conditions_dsl,
        objects::{dsl as objects_dsl, dsl::*},
    },
};
pub(crate) fn router() -> Router {
    Router::new().route("/basic/:address", get(basic))
}

async fn basic(
    ExtractPath(extracted_address): ExtractPath<iota_types::base_types::IotaAddress>,
    Extension(state): Extension<StardustExtension>,
) -> Result<BasicResponse, ApiError> {
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

    let basic_outputs: Vec<BasicOutput> = stored_objects
        .into_iter()
        .filter_map(|stored_object| BasicOutput::try_from(stored_object.clone()).ok())
        .collect();

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
    use tokio::sync::oneshot;
    use tracing::Level;
    use tracing_subscriber::FmtSubscriber;

    use super::*;
    use crate::{
        db::ConnectionPool,
        models::{ExpirationUnlockCondition, IotaAddress, StoredObject},
        rest::{config::RestApiConfig, spawn_rest_server},
        schema::expiration_unlock_conditions::dsl::expiration_unlock_conditions,
    };

    #[tokio::test]
    async fn get_basic_object() -> Result<(), anyhow::Error> {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .finish();

        let _ = tracing::subscriber::set_default(subscriber);

        let test_db = "stored_basic_object_round_trip.db";
        let pool = ConnectionPool::new_with_url(test_db, Default::default()).unwrap();
        pool.run_migrations().unwrap();
        let mut connection = pool.get_connection().unwrap();

        // Populate the database with a basic object
        let owner_address: iota_types::base_types::IotaAddress = ObjectID::random().into();
        let basic_object_id = ObjectID::random();
        let basic_output = BasicOutput {
            id: UID::new(basic_object_id),
            balance: Balance::new(100),
            native_tokens: Bag::default(),
            storage_deposit_return: None,
            timelock: None,
            expiration: Some(
                iota_types::stardust::output::unlock_conditions::ExpirationUnlockCondition {
                    owner: owner_address.clone(),
                    return_address: owner_address.clone(),
                    unix_time: 100,
                },
            ),
            metadata: None,
            tag: None,
            sender: None,
        };

        let stored_object = StoredObject::new_basic_for_testing(basic_output.clone())?;

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
            object_id: IotaAddress(basic_object_id.into()),
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
                bind_port: 3001,
                ..Default::default()
            },
            pool,
            shutdown_rx,
        );

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let resp = reqwest::get(format!(
            "http://127.0.0.1:3001/v1/basic/{}",
            owner_address.to_string()
        ))
        .await?;

        // parse all BasicOutput objects from the response
        let basic_outputs: Vec<BasicOutput> = resp.json().await?;
        assert_eq!(basic_outputs.len(), 1);

        // check if the BasicOutput object is the same as the one we inserted
        assert_eq!(basic_outputs[0], basic_output);

        shutdown_tx.send(()).unwrap();

        join_handle.await.unwrap();

        // clean-up test db
        std::fs::remove_file(test_db).unwrap();

        Ok(())
    }
}
