// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use axum::{Extension, Router, response::IntoResponse};
use tokio::{sync::oneshot, task::JoinHandle};
use tracing::{error, info};

use crate::{
    db::ConnectionPool,
    rest::{
        config::RestApiConfig, error::ApiError, extension::StardustExtension, routes::router_all,
    },
};

pub(crate) mod config;
mod error;
mod extension;
mod extractors;
mod routes;

pub(crate) fn spawn_rest_server(
    config: RestApiConfig,
    connection_pool: ConnectionPool,
    shutdown: oneshot::Receiver<()>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let app = build_app(connection_pool);

        let listener = tokio::net::TcpListener::bind(config.socket_addr())
            .await
            .expect("Failed to bind to socket");

        info!("Listening on: {}", config.socket_addr());

        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                shutdown.await.ok();
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
        .layer(Extension(StardustExtension { connection_pool }))
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
