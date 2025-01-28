use std::sync::Arc;

use axum::Extension;
use http::StatusCode;
use prometheus::Registry;

/// Retrieve the metrics of the service.
#[utoipa::path(
    get,
    path = "/metrics",
    description = "Retrieve the metrics of the service.",
    responses(
        (status = 200, description = "Successful request", body = String),
        (status = 500, description = "Internal server error")
    ),
)]
pub(crate) async fn metrics(Extension(registry): Extension<Arc<Registry>>) -> (StatusCode, String) {
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
    use tokio_util::sync::CancellationToken;
    use tracing::Level;
    use tracing_subscriber::FmtSubscriber;

    use super::*;
    use crate::{
        INDEXER_METRICS,
        db::{ConnectionPool, Name},
        metrics::IndexerMetrics,
        rest::{routes::v1::get_free_port_for_testing_only, spawn_rest_server},
    };

    #[tokio::test]
    async fn test_metrics() {
        let sub = FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .finish();
        let _ = tracing::subscriber::set_default(sub);

        let test_db = "test_metrics.db";
        let pool =
            ConnectionPool::new_with_url(test_db, Default::default(), Name::Objects).unwrap();
        pool.run_migrations().unwrap();

        let registry = Arc::new(Registry::default());

        let cancel_token = CancellationToken::new();
        let bind_port = get_free_port_for_testing_only().unwrap();
        let handle = spawn_rest_server(
            format!("127.0.0.1:{}", bind_port).parse().unwrap(),
            pool,
            cancel_token.clone(),
            registry.clone(),
        );

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        INDEXER_METRICS.get_or_init(|| Arc::new(IndexerMetrics::new(&*registry)));

        INDEXER_METRICS
            .get()
            .unwrap()
            .last_checkpoint_checked
            .set(42);

        INDEXER_METRICS
            .get()
            .unwrap()
            .last_checkpoint_indexed
            .set(42);

        INDEXER_METRICS
            .get()
            .unwrap()
            .indexed_basic_outputs_count
            .inc();

        INDEXER_METRICS
            .get()
            .unwrap()
            .indexed_nft_outputs_count
            .inc();

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

        cancel_token.cancel();
        handle.await.unwrap();
        std::fs::remove_file(test_db).unwrap();
    }
}
