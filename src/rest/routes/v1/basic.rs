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
            responses::{BasicOutput, BasicOutputVec},
        },
    },
};

pub(crate) fn router() -> Router {
    Router::new()
        .route("/basic/:address", get(basic))
        .route("/basic/resolved/:address", get(resolved))
}

/// Get the `BasicOutput`s owned by the address
#[utoipa::path(
get,
path = "/v1/basic/{address}",
description =
    "Fetches basic outputs for a specified address with optional pagination.
    It returns basic outputs with expiration unlock conditions that refer to the given address either as the `owner` or as the `return_address`.
    Results can be paginated by providing optional `page` and `page_size` query parameters.",
    responses(
        (status = 200, description = "Successful request", body = BasicOutputVec),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal server error"),
        (status = 503, description = "Service unavailable"),
        (status = 403, description = "Forbidden")
    ),
    params(
        ("address" = String, Path, description = "The hexadecimal address for which to fetch basic outputs."),
        ("page" = Option<u32>, Query, description = "Page number for pagination. Defaults to 1."),
        ("page_size" = Option<u32>, Query, description = "Number of items per page for pagination. Defaults to 10.")
    )
)]
async fn basic(
    Path(address): Path<iota_types::base_types::IotaAddress>,
    Query(pagination): Query<PaginationParams>,
    Extension(state): Extension<State>,
) -> Result<BasicOutputVec, ApiError> {
    let stored_objects =
        fetch_stored_objects(address, pagination, state, ObjectType::Basic, false)?;
    let basic_outputs = stored_objects_to_basic_outputs(stored_objects)?;
    Ok(BasicOutputVec(basic_outputs))
}

/// Get the `BasicOutput`s owned by the address considering resolved expiration
/// unlock condition.
#[utoipa::path(
get,
path = "/v1/basic/resolved/{address}",
description =
    "Fetches basic outputs for a specified address, considering the resolved expiration unlock conditions.
    The expiration unlock conditions determine access based on whether the latest checkpoint timestamp is
    before or after the expiration time. Results can be paginated by providing optional `page` and `page_size`
    query parameters.

    Before Expiration:
    Objects are accessible to the `owner` if the latest checkpoint UNIX timestamp (in milliseconds)
    is `less than` the expiration time.

    After Expiration:
    Objects become accessible to the `return_address` if the latest checkpoint UNIX timestamp (in milliseconds)
    is `greater than or equal to` the expiration time.",
    responses(
        (status = 200, description = "Successful request", body = BasicOutputVec),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal server error"),
        (status = 503, description = "Service unavailable"),
        (status = 403, description = "Forbidden")
    ),
    params(
        ("address" = String, Path, description = "The hexadecimal address for which to fetch basic outputs."),
        ("page" = Option<u32>, Query, description = "Page number for pagination. Defaults to 1."),
        ("page_size" = Option<u32>, Query, description = "Number of items per page for pagination. Defaults to 10.")
    )
)]
async fn resolved(
    Path(address): Path<iota_types::base_types::IotaAddress>,
    Query(pagination): Query<PaginationParams>,
    Extension(state): Extension<State>,
) -> Result<BasicOutputVec, ApiError> {
    let stored_objects = fetch_stored_objects(address, pagination, state, ObjectType::Basic, true)?;
    let basic_outputs = stored_objects_to_basic_outputs(stored_objects)?;
    Ok(BasicOutputVec(basic_outputs))
}

fn stored_objects_to_basic_outputs(
    stored_objects: Vec<StoredObject>,
) -> Result<Vec<BasicOutput>, ApiError> {
    stored_objects
        .into_iter()
        .map(|stored_object| {
            iota_types::stardust::output::basic::BasicOutput::try_from(stored_object)
                .map(BasicOutput::from)
                .map_err(|e| {
                    error!("failed to convert stored object to basic output: {}", e);
                    ApiError::InternalServerError
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use iota_types::base_types::ObjectID;
    use tokio_util::sync::CancellationToken;
    use tracing::Level;
    use tracing_subscriber::FmtSubscriber;

    use crate::{
        db::{ConnectionPool, Name},
        rest::{
            routes::{
                test_utils::{create_and_insert_basic_output, get_free_port_for_testing_only},
                v1::{basic::BasicOutput, ensure_checkpoint_is_set},
            },
            spawn_rest_server,
        },
    };

    #[tokio::test]
    async fn get_basic_objects_by_address() -> Result<(), anyhow::Error> {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .finish();

        let _ = tracing::subscriber::set_default(subscriber);

        let test_db = "stored_basic_object_address_filter_test.db";

        if Path::new(test_db).exists() {
            std::fs::remove_file(test_db).unwrap();
        }

        let pool =
            ConnectionPool::new_with_url(test_db, Default::default(), Name::Objects).unwrap();
        pool.run_migrations().unwrap();
        let mut connection = pool.get_connection().unwrap();

        let owner_address: iota_types::base_types::IotaAddress = ObjectID::random().into();
        let other_address: iota_types::base_types::IotaAddress = ObjectID::random().into();

        // Populate the database with objects for two different addresses
        let mut inserted_objects = vec![];

        for i in 0..2 {
            let basic_output = create_and_insert_basic_output(
                &mut connection,
                owner_address,
                100 + i,
                100 + i as u32,
            )?;
            let serialized_output = BasicOutput::from(basic_output.clone());
            inserted_objects.push(serialized_output);
        }

        // Insert objects for the other address
        for i in 0..5 {
            let _ = create_and_insert_basic_output(
                &mut connection,
                other_address,
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
            bind_port, owner_address
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
            bind_port, other_address
        ))
        .await?;

        let other_basic_outputs: Vec<BasicOutput> = resp.json().await?;
        assert_eq!(other_basic_outputs.len(), 5);

        for output in other_basic_outputs {
            assert!(output.balance.value >= 200); // Validate range for "other_address" objects
        }

        cancel_token.cancel();
        handle.await.unwrap();

        // Clean up the test database
        std::fs::remove_file(test_db).unwrap();

        Ok(())
    }

    #[tokio::test]
    async fn get_basic_objects_by_address_resolved() -> Result<(), anyhow::Error> {
        ensure_checkpoint_is_set();

        let sub = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .finish();
        let _ = tracing::subscriber::set_default(sub);

        let test_db = "stored_basic_object_address_filter_resolved_test.db";

        if Path::new(test_db).exists() {
            std::fs::remove_file(test_db).unwrap();
        }

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
            let out = create_and_insert_basic_output(&mut conn, owner_addr, 100 + i, big_ts)?;
            unexpired.push(BasicOutput::from(out));
        }

        // outputs expired for return address
        let mut expired = vec![];
        let small_ts = 100;
        for i in 0..2 {
            let out = create_and_insert_basic_output(&mut conn, return_addr, 200 + i, small_ts)?;
            expired.push(BasicOutput::from(out));
        }

        // irrelevant outputs
        let third_addr: iota_types::base_types::IotaAddress = ObjectID::random().into();
        for i in 0..3 {
            let _ = create_and_insert_basic_output(&mut conn, third_addr, 300 + i, big_ts)?;
        }

        drop(conn);

        let cancel_token = tokio_util::sync::CancellationToken::new();
        let port = get_free_port_for_testing_only().unwrap();
        let handle = spawn_rest_server(
            format!("127.0.0.1:{port}").parse().unwrap(),
            pool,
            cancel_token.clone(),
        );
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // check unexpired
        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/basic/resolved/{}",
            port, owner_addr
        ))
        .await?;
        let list: Vec<BasicOutput> = resp.json().await?;
        assert_eq!(list.len(), unexpired.len());
        assert_eq!(list, unexpired);

        // check expired
        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/basic/resolved/{}",
            port, return_addr
        ))
        .await?;
        let list: Vec<BasicOutput> = resp.json().await?;
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

        let test_db = "stored_pagination_test.db";

        if Path::new(test_db).exists() {
            std::fs::remove_file(test_db).unwrap();
        }

        let pool =
            ConnectionPool::new_with_url(test_db, Default::default(), Name::Objects).unwrap();
        pool.run_migrations().unwrap();
        let mut connection = pool.get_connection().unwrap();

        let owner_address: iota_types::base_types::IotaAddress = ObjectID::random().into();

        // Populate the database with multiple basic objects
        let mut inserted_objects = vec![];
        for i in 0..15 {
            let basic_output = create_and_insert_basic_output(
                &mut connection,
                owner_address,
                100 + i,
                100 + i as u32,
            )?;
            let serialized_output = BasicOutput::from(basic_output.clone());
            inserted_objects.push(serialized_output);
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
            bind_port, owner_address, page, page_size
        ))
        .await?;

        let basic_outputs: Vec<BasicOutput> = resp.json().await?;
        assert_eq!(basic_outputs.len(), page_size);
        assert_eq!(basic_outputs, inserted_objects[..page_size]);

        // Test second page
        let page = 2;
        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/basic/{}?page={}&page_size={}",
            bind_port, owner_address, page, page_size
        ))
        .await?;

        let basic_outputs: Vec<BasicOutput> = resp.json().await?;
        assert_eq!(basic_outputs.len(), page_size);
        assert_eq!(basic_outputs, inserted_objects[page_size..2 * page_size]);

        // Test third page (remaining items)
        let page = 3;
        let resp = reqwest::get(format!(
            "http://127.0.0.1:{}/v1/basic/{}?page={}&page_size={}",
            bind_port, owner_address, page, page_size
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
}
