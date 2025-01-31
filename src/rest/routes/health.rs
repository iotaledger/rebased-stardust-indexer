use axum::Extension;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::error;
use utoipa::ToSchema;

use crate::{
    impl_into_response,
    models::ObjectType,
    rest::{ApiError, State},
    schema::objects::{dsl::objects, object_type},
};

/// Retrieve the health of the service.
#[utoipa::path(
    get,
    path = "/health",
    description = "Retrieve the health of the service. It returns total object count, basic object count, and NFT object count.",
    responses(
        (status = 200, description = "Successful request", body = HealthResponse),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    ),
)]
pub(crate) async fn health(Extension(state): Extension<State>) -> Result<HealthResponse, ApiError> {
    let mut conn = state.connection_pool.get_connection().map_err(|e| {
        error!("failed to get connection: {e}");
        ApiError::ServiceUnavailable(format!("failed to get connection: {}", e))
    })?;

    let objects_count = objects.count().get_result(&mut conn).map_err(|e| {
        error!("failed to count objects: {e}");
        ApiError::ServiceUnavailable(format!("failed to count objects: {}", e))
    })?;

    let basic_objects_count = objects
        .filter(object_type.eq(ObjectType::Basic))
        .count()
        .get_result(&mut conn)
        .map_err(|e| {
            error!("failed to count basic objects: {e}");
            ApiError::InternalServerError
        })?;

    let nft_objects_count = objects
        .filter(object_type.eq(ObjectType::Nft))
        .count()
        .get_result(&mut conn)
        .map_err(|e| {
            error!("failed to count nft objects: {e}");
            ApiError::InternalServerError
        })?;

    Ok(HealthResponse {
        objects_count,
        basic_objects_count,
        nft_objects_count,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub(crate) struct HealthResponse {
    pub objects_count: i64,
    pub basic_objects_count: i64,
    pub nft_objects_count: i64,
}
impl_into_response!(HealthResponse);

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use iota_types::base_types::ObjectID;
    use tokio_util::sync::CancellationToken;
    use tracing::Level;
    use tracing_subscriber::FmtSubscriber;

    use crate::{
        db::{ConnectionPool, Name},
        rest::{
            routes::{
                health::HealthResponse,
                test_utils::{
                    create_and_insert_basic_output, create_and_insert_nft_output,
                    get_free_port_for_testing_only,
                },
            },
            spawn_rest_server,
        },
    };

    #[tokio::test]
    async fn test_health_endpoint() -> Result<(), anyhow::Error> {
        use iota_types::base_types::IotaAddress;

        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .finish();

        let _ = tracing::subscriber::set_default(subscriber);

        let test_db = "test_health_endpoint.db";

        if Path::new(test_db).exists() {
            fs::remove_file(test_db).unwrap();
        }

        let pool =
            ConnectionPool::new_with_url(test_db, Default::default(), Name::Objects).unwrap();
        pool.run_migrations().unwrap();
        let mut connection = pool.get_connection().unwrap();

        // Populate the database with objects
        let owner_address: IotaAddress = ObjectID::random().into();

        // Insert basic objects
        for i in 0..3 {
            let _ = create_and_insert_basic_output(
                &mut connection,
                owner_address.clone(),
                100 + i,
                100 + i as u32,
            )?;
        }

        // Insert NFT objects
        for i in 0..2 {
            let _ = create_and_insert_nft_output(
                &mut connection,
                owner_address.clone(),
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

        // Test the health endpoint
        let resp = reqwest::get(format!("http://127.0.0.1:{}/health", bind_port)).await?;
        assert_eq!(resp.status(), 200);

        let health_response: HealthResponse = resp.json().await?;
        assert_eq!(health_response.objects_count, 5); // 3 basic + 2 NFT
        assert_eq!(health_response.basic_objects_count, 3);
        assert_eq!(health_response.nft_objects_count, 2);

        cancel_token.cancel();
        handle.await.unwrap();

        // Clean up the test database
        std::fs::remove_file(test_db).unwrap();

        Ok(())
    }
}
