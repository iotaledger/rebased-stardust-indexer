// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::net::SocketAddr;

use axum::{http, response::IntoResponse, Extension, Router};
use http::Method;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};
use utoipa::OpenApi;

use crate::{
    db::ConnectionPool,
    rest::{error::ApiError, routes::router_all},
};

mod error;
mod extractors;
pub(crate) mod routes;

#[derive(OpenApi)]
#[openapi(
    paths(
        routes::health::health,
        routes::v1::basic::basic,
        routes::v1::basic::resolved,
        routes::v1::nft::nft,
        routes::v1::nft::resolved
    ),
    servers((url = "/"))
)]
pub struct ApiDoc;

#[derive(Clone)]
pub(crate) struct State {
    pub(crate) connection_pool: ConnectionPool,
}

pub(crate) fn spawn_rest_server(
    socket_addr: SocketAddr,
    connection_pool: ConnectionPool,
    cancel_token: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let app = build_app(connection_pool);

        let listener = tokio::net::TcpListener::bind(socket_addr)
            .await
            .expect("failed to bind to socket");

        info!("Listening on: {}", socket_addr);

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                cancel_token.cancelled().await;
                info!("Shutdown signal received.");
            })
            .await
            .inspect_err(|e| error!("Server encountered an error: {e}"))
            .ok();
    })
}

fn build_app(connection_pool: ConnectionPool) -> Router {
    // Allow all origins (CORS policy) - This is safe because the API is public and
    // does not require authentication. CORS is a browser-enforced mechanism
    // that restricts cross-origin requests, but since the API is already accessible
    // without credentials or sensitive data, there is no additional security risk.
    // Abuse should be mitigated via backend protections such as rate-limiting.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Method::GET)
        .allow_headers(Any);

    Router::new()
        .merge(router_all())
        .layer(Extension(State { connection_pool }))
        .layer(cors)
        .fallback(fallback)
}

async fn fallback() -> impl IntoResponse {
    ApiError::Forbidden
}

#[macro_export]
macro_rules! impl_into_response {
    ($($t:ty),*) => {
        $(
            impl axum::response::IntoResponse for $t {
                fn into_response(self) -> axum::response::Response {
                    axum::Json(self).into_response()
                }
            }
        )*
    };
}
