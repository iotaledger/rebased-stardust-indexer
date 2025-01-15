// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::net::SocketAddr;

use axum::{Extension, Router, http, response::IntoResponse};
use http::{HeaderValue, Method};
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
mod routes;

#[derive(OpenApi)]
#[openapi(
    paths(
        routes::v1::basic::basic,
        routes::v1::basic::resolved,
        routes::v1::nft::nft,
        routes::v1::nft::resolved
    ),
    servers((url = "http://127.0.0.1:3000"))
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
    let cors = CorsLayer::new()
        .allow_origin("http://0.0.0.0".parse::<HeaderValue>().unwrap())
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
