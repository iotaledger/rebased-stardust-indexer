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
use tracing::info;

/// Metrics for the service.
#[derive(Clone)]
pub struct Metrics {
    pub last_checkpoint_checked: IntGauge,
    pub last_checkpoint_indexed: IntGauge,
    pub indexed_basic_outputs_count: IntCounter,
    pub indexed_nft_outputs_count: IntCounter,
}

impl Metrics {
    pub fn new(registry: &Registry) -> Self {
        Self {
            last_checkpoint_checked: register_int_gauge_with_registry!(
                "last_checkpoint_checked",
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
pub(crate) fn start_prometheus_server(addr: SocketAddr) -> Result<Registry, anyhow::Error> {
    info!("Starting prometheus server with label: Rebased Indexer Metrics");

    let registry = Registry::default();
    METRICS.get_or_init(|| Arc::new(Metrics::new(&registry)));

    let app = Router::new()
        .route(METRICS_ROUTE, get(metrics))
        .layer(Extension(registry.clone()));

    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    Ok(registry)
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
        let _ = start_prometheus_server(format!("127.0.0.1:{}", bind_port).parse().unwrap());
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        METRICS.get().unwrap().last_checkpoint_checked.set(42);
        METRICS.get().unwrap().last_checkpoint_indexed.set(42);
        METRICS.get().unwrap().indexed_basic_outputs_count.inc();
        METRICS.get().unwrap().indexed_nft_outputs_count.inc();

        let resp = reqwest::get(format!("http://127.0.0.1:{}/metrics", bind_port))
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);

        let body = resp.text().await.unwrap();

        fn parse_metric_value(metrics: &str, metric_name: &str) -> Option<f64> {
            metrics
                .lines()
                .find(|line| line.starts_with(metric_name))
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|value| value.parse::<f64>().ok())
        }

        assert_eq!(
            parse_metric_value(&body, "last_checkpoint_checked"),
            Some(42.0)
        );
        assert_eq!(
            parse_metric_value(&body, "last_checkpoint_indexed"),
            Some(42.0)
        );
        assert_eq!(
            parse_metric_value(&body, "indexed_basic_outputs_count"),
            Some(1.0)
        );
        assert_eq!(
            parse_metric_value(&body, "indexed_nft_outputs_count"),
            Some(1.0)
        );
    }
}
