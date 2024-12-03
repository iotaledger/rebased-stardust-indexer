use axum::{Extension, Router, routing::get};
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl, SelectableHelper};
use serde::Serialize;
use tracing::error;

use crate::{
    impl_into_response,
    models::{IotaAddress, StoredObject},
    rest::{error::ApiError, extension::StardustExtension, extractors::custom_path::ExtractPath},
    schema::objects::{dsl::objects, id},
};
pub(crate) fn filter() -> Router {
    Router::new().route("/nft/:address", get(nft))
}

async fn nft(
    ExtractPath(extracted_id): ExtractPath<iota_types::base_types::IotaAddress>,
    Extension(state): Extension<StardustExtension>,
) -> Result<NftResponse, ApiError> {
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
    let nft = iota_types::stardust::output::nft::NftOutput::try_from(stored_object[0].clone())
        .map_err(|err| ApiError::ServiceUnavailable(err.to_string()))?;

    Ok(NftResponse { nft })
}

#[derive(Clone, Debug, Serialize)]
struct NftResponse {
    #[serde(flatten)]
    nft: iota_types::stardust::output::nft::NftOutput,
}

impl_into_response!(NftResponse);

#[cfg(test)]
mod tests {
    use diesel::{RunQueryDsl, insert_into};
    use serde_json::Value;
    use tokio::sync::oneshot;
    use tracing::Level;
    use tracing_subscriber::FmtSubscriber;

    use crate::{
        db::ConnectionPool,
        models::StoredObject,
        rest::{config::RestApiConfig, spawn_rest_server},
        schema::objects::dsl::*,
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

        // Populate the database with a basic object
        let stored_object = StoredObject::new_nft_for_testing();

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
                bind_port: 3002,
                ..Default::default()
            },
            pool,
            shutdown_rx,
        );

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let resp = reqwest::get(format!(
            "http://127.0.0.1:3002/v1/nft/{}",
            stored_object.id.0.to_string()
        ))
        .await?;

        println!("{:?}", resp.json::<Value>().await.unwrap());

        shutdown_tx.send(()).unwrap();

        join_handle.await.unwrap();

        // clean-up test db
        std::fs::remove_file(test_db).unwrap();

        Ok(())
    }
}
