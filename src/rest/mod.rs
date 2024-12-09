// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::net::SocketAddr;

use axum::{Extension, Router, response::IntoResponse};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::{
    db::ConnectionPool,
    rest::{error::ApiError, routes::router_all},
};

mod error;
mod extractors;
mod routes;

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
    Router::new()
        .merge(router_all())
        .layer(Extension(State { connection_pool }))
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
