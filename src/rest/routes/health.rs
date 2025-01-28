use axum::Extension;
use diesel::prelude::*;
use serde::Serialize;
use tracing::error;
use utoipa::ToSchema;

use crate::{
    impl_into_response,
    models::ObjectType,
    rest::{State, error::ApiError},
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

#[derive(Clone, Debug, Serialize, ToSchema)]
pub(crate) struct HealthResponse {
    pub objects_count: i64,
    pub basic_objects_count: i64,
    pub nft_objects_count: i64,
}
impl_into_response!(HealthResponse);
