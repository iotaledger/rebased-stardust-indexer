// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::{Router, routing::get};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::rest::{
    ApiDoc,
    routes::{health::health, metrics::metrics},
};

pub(crate) mod health;
pub(crate) mod metrics;
pub(crate) mod v1;

pub(crate) fn router_all() -> Router {
    Router::new().merge(v1::router()).merge(
        Router::new()
            .route("/health", get(health))
            .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
            .merge(Router::new().route("/metrics", get(metrics))),
    )
}
