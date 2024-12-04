// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::{Extension, Router, routing::get};
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl, SelectableHelper};
use iota_types::stardust::output::basic::BasicOutput;
use serde::Serialize;
use tracing::error;

use crate::{
    impl_into_response,
    models::{IotaAddress, StoredObject},
    rest::{error::ApiError, extension::StardustExtension, extractors::custom_path::ExtractPath},
    schema::objects::dsl::*,
};

pub(crate) fn router() -> Router {
    Router::new().route("/basic/:address", get(basic))
}

async fn basic(
    ExtractPath(extracted_id): ExtractPath<iota_types::base_types::IotaAddress>,
    Extension(state): Extension<StardustExtension>,
) -> Result<BasicResponse, ApiError> {
    let mut conn = state.connection_pool.get_connection().map_err(|e| {
        error!("Failed to get connection: {}", e);
        ApiError::ServiceUnavailable(format!("Failed to get connection: {}", e))
    })?;

    let stored_object = objects
        .select(StoredObject::as_select())
        .filter(id.eq(IotaAddress(extracted_id)))
        .load::<StoredObject>(&mut conn)
        .map_err(|err| {
            error!("Failed to load stored object: {}", err);
            ApiError::InternalServerError
        })?;

    if stored_object.is_empty() {
        return Err(ApiError::NotFound);
    }

    let basic_object = BasicOutput::try_from(stored_object[0].clone())
        .map_err(|err| ApiError::BadRequest(err.to_string()))?;

    Ok(BasicResponse(basic_object))
}

#[derive(Clone, Debug, Serialize)]
struct BasicResponse(BasicOutput);

impl_into_response!(BasicResponse);

#[cfg(test)]
mod tests {
    use bcs::from_bytes;
    use diesel::{RunQueryDsl, insert_into};
    use iota_types::stardust::output::basic::BasicOutput;
    use tokio::sync::oneshot;
    use tracing::Level;
    use tracing_subscriber::FmtSubscriber;

    use super::*;
    use crate::{
        db::ConnectionPool,
        models::StoredObject,
        rest::{config::RestApiConfig, spawn_rest_server},
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
        let stored_object = StoredObject::new_basic_for_testing();
        let basic_output: BasicOutput = from_bytes(&stored_object.contents).unwrap();

        let rows_inserted = insert_into(objects)
            .values(&vec![stored_object.clone()])
            .execute(&mut connection)
            .unwrap();
        assert_eq!(rows_inserted, 1);

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
            stored_object.id.0.to_string()
        ))
        .await?;

        assert_eq!(resp.json::<BasicOutput>().await.unwrap(), basic_output);

        shutdown_tx.send(()).unwrap();

        join_handle.await.unwrap();

        // clean-up test db
        std::fs::remove_file(test_db).unwrap();

        Ok(())
    }
}
