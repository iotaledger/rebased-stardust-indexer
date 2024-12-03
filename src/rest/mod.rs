use axum::{Extension, Router, response::IntoResponse};
use tokio::{sync::oneshot, task::JoinHandle};
use tracing::{error, info};

use crate::{
    db::ConnectionPool,
    rest::{
        config::RestApiConfig, error::ApiError, extension::StardustExtension, routes::filter_all,
    },
};

pub mod config;
pub mod error;
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
            .map_err(|e| {
                error!("Server encountered an error: {}", e);
                e
            })
            .ok();
    })
}

fn build_app(connection_pool: ConnectionPool) -> Router {
    Router::new()
        .merge(filter_all())
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
