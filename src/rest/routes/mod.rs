// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use axum::{Extension, Router, routing::get};
use diesel::prelude::*;
use http::StatusCode;
use prometheus::Registry;
use serde::Serialize;
use tracing::error;
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    impl_into_response,
    models::ObjectType,
    rest::{ApiDoc, State, error::ApiError},
    schema::objects::{dsl::objects, object_type},
};

pub(crate) mod v1;

pub(crate) fn router_all() -> Router {
    Router::new().merge(v1::router()).merge(
        Router::new()
            .route("/health", get(health))
            .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
            .merge(Router::new().route("/metrics", get(metrics))),
    )
}

async fn health(Extension(state): Extension<State>) -> Result<HealthResponse, ApiError> {
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
            ApiError::ServiceUnavailable(format!("failed to count basic objects: {}", e))
        })?;

    let nft_objects_count = objects
        .filter(object_type.eq(ObjectType::Nft))
        .count()
        .get_result(&mut conn)
        .map_err(|e| {
            error!("failed to count nft objects: {e}");
            ApiError::ServiceUnavailable(format!("failed to count nft objects: {}", e))
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

async fn metrics(Extension(registry): Extension<Arc<Registry>>) -> (StatusCode, String) {
    let metrics_families = registry.gather();
    match prometheus::TextEncoder::new().encode_to_string(&metrics_families) {
        Ok(metrics) => (StatusCode::OK, metrics),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unable to encode metrics: {error}"),
        ),
    }
}
