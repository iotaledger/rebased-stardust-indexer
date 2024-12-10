// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::net::SocketAddr;

use axum::{Extension, Router, async_trait, response::IntoResponse};
use tokio::task::JoinHandle;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};
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
) -> RestApiHandle {
    let token = cancel_token.child_token();
    let handle = tokio::spawn(async move {
        let app = build_app(connection_pool);

        let listener = tokio::net::TcpListener::bind(socket_addr)
            .await
            .expect("failed to bind to socket");

        info!("Listening on: {}", socket_addr);

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                token.cancelled().await;
                info!("Shutdown signal received.");
            })
            .await
            .inspect_err(|e| error!("Server encountered an error: {e}"))
            .ok();
    });

    RestApiHandle {
        handle,
        token: cancel_token,
    }
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

pub struct RestApiHandle {
    pub handle: JoinHandle<()>,
    pub token: CancellationToken,
}

impl RestApiHandle {
    /// Sends a Shutdown Signal to the RestApi server and wait for the task
    /// to finish, this will block the execution
    #[tracing::instrument(name = "RestApiHandle", skip(self), err)]
    pub async fn graceful_shutdown(self) -> anyhow::Result<()> {
        tracing::info!("Received shutdown Signal");
        self.token.cancel();
        tracing::info!("Wait for task to shutdown");
        self.handle
            .await
            .map_err(Into::into)
            .inspect(|_| tracing::info!("Task shutdown successfully"))
    }
}

#[async_trait]
impl IntoSubsystem<anyhow::Error> for RestApiHandle {
    async fn run(self, subsys: SubsystemHandle) -> anyhow::Result<()> {
        subsys.on_shutdown_requested().await;
        self.graceful_shutdown().await
    }
}
