// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use std::{
    net::SocketAddr,
    sync::{Arc, OnceLock},
};

use axum::{Extension, Router, routing::get};
use http::StatusCode;
use prometheus::{
    IntCounter, IntGauge, Registry, register_int_counter_with_registry,
    register_int_gauge_with_registry,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::info;

/// Metrics for the service.
#[derive(Clone)]
pub struct Metrics {
    pub last_checkpoint_received: IntGauge,
    pub last_checkpoint_indexed: IntGauge,
    pub indexed_basic_outputs_count: IntCounter,
    pub indexed_nft_outputs_count: IntCounter,
}

impl Metrics {
    pub fn new(registry: &Registry) -> Self {
        Self {
            last_checkpoint_received: register_int_gauge_with_registry!(
                "last_checkpoint_received",
                "The last checkpoint received from the remote store",
                registry,
            )
            .unwrap(),
            last_checkpoint_indexed: register_int_gauge_with_registry!(
                "last_checkpoint_indexed",
                "The last checkpoint that was indexed",
                registry,
            )
            .unwrap(),
            indexed_basic_outputs_count: register_int_counter_with_registry!(
                "indexed_basic_outputs_count",
                "The total number of basic outputs indexed",
                registry,
            )
            .unwrap(),
            indexed_nft_outputs_count: register_int_counter_with_registry!(
                "indexed_nft_outputs_count",
                "The total number of NFT outputs indexed",
                registry,
            )
            .unwrap(),
        }
    }
}

/// Global metrics registry.
pub(crate) static METRICS: OnceLock<Arc<Metrics>> = OnceLock::new();
const METRICS_ROUTE: &str = "/metrics";

/// Start the Prometheus metrics service.
pub(crate) fn spawn_prometheus_server(
    socket_addr: SocketAddr,
    cancel_token: CancellationToken,
) -> Result<(Registry, JoinHandle<Result<(), anyhow::Error>>), anyhow::Error> {
    let registry = Registry::default();
    METRICS.get_or_init(|| Arc::new(Metrics::new(&registry)));

    let extension = registry.clone();
    let handle = tokio::spawn(async move {
        // Attempt to bind the socket
        let listener = tokio::net::TcpListener::bind(socket_addr)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to bind to socket {socket_addr}: {e}"))?;

        info!("Listening on: {socket_addr}");

        let app = Router::new()
            .route(METRICS_ROUTE, get(metrics))
            .layer(Extension(extension));

        // Run the server with graceful shutdown
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                cancel_token.cancelled().await;
                info!("Shutdown signal received.");
            })
            .await
            .map_err(|e| anyhow::anyhow!("Server encountered an error: {e}"))?;

        Ok(())
    });

    Ok((registry, handle))
}

/// Retrieve the Prometheus metrics of the service.
pub(crate) async fn metrics(Extension(registry): Extension<Registry>) -> (StatusCode, String) {
    let metrics_families = registry.gather();
    match prometheus::TextEncoder::new().encode_to_string(&metrics_families) {
        Ok(metrics) => (StatusCode::OK, metrics),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unable to encode metrics: {error}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use tracing::Level;
    use tracing_subscriber::FmtSubscriber;

    use super::*;
    use crate::rest::routes::test_utils::get_free_port_for_testing_only;

    #[tokio::test]
    async fn test_metrics() {
        let sub = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .finish();
        let _ = tracing::subscriber::set_default(sub);

        let bind_port = get_free_port_for_testing_only().unwrap();
        let cancel_token = CancellationToken::new();

        // Start the Prometheus server in a separate task and capture the join handle
        let (_registry, server_task) = spawn_prometheus_server(
            format!("127.0.0.1:{}", bind_port).parse().unwrap(),
            cancel_token.clone(),
        )
        .unwrap();

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        METRICS.get().unwrap().last_checkpoint_received.set(42);
        METRICS.get().unwrap().last_checkpoint_indexed.set(42);
        METRICS.get().unwrap().indexed_basic_outputs_count.inc();
        METRICS.get().unwrap().indexed_nft_outputs_count.inc();

        let resp = reqwest::get(format!("http://127.0.0.1:{}/metrics", bind_port))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let body = resp.text().await.unwrap();

        fn parse_metric_value(metrics: &str, metric_name: &str) -> Option<u64> {
            metrics
                .lines()
                .find(|line| line.starts_with(metric_name))
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|value| value.parse::<u64>().ok())
        }

        assert_eq!(
            parse_metric_value(&body, "last_checkpoint_received"),
            Some(42)
        );
        assert_eq!(
            parse_metric_value(&body, "last_checkpoint_indexed"),
            Some(42)
        );
        assert_eq!(
            parse_metric_value(&body, "indexed_basic_outputs_count"),
            Some(1)
        );
        assert_eq!(
            parse_metric_value(&body, "indexed_nft_outputs_count"),
            Some(1)
        );

        cancel_token.cancel();
        let _ = server_task.await.unwrap();
    }
}
