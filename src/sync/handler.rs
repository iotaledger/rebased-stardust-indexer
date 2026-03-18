// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! Checkpoint syncing Handlers for the Indexer

use iota_data_ingestion_core::{
    DataIngestionMetrics, IndexerExecutor, ReaderOptions, WorkerPool,
    reader::v2::{CheckpointReaderConfig, RemoteUrl},
};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::{
    db::ConnectionPool,
    metrics::spawn_prometheus_server,
    sync::{IndexerConfig, progress_store::SqliteProgressStore, worker::CheckpointWorker},
};

/// The `Indexer` encapsulates the main logic behind the checkpoint
/// synchronization from a Fullnode.
///
/// It handles the initialization and execution
/// of the `IndexerExecutor` in background in as task and provide an interface
/// to gracefully shutdown it
#[derive(Debug)]
pub struct Indexer {
    tasks: JoinSet<anyhow::Result<()>>,
    cancel_token: CancellationToken,
}

impl Indexer {
    /// Init the Checkpoint synchronization from a Fullnode
    pub async fn init(
        pool: ConnectionPool,
        pool_progress_store: ConnectionPool,
        indexer_config: Box<IndexerConfig>,
    ) -> Result<Self, anyhow::Error> {
        // Set up the Prometheus metrics service
        let cancel_token = CancellationToken::new();
        let mut tasks = JoinSet::new();

        let registry = spawn_prometheus_server(
            indexer_config.metrics_address,
            cancel_token.clone(),
            &mut tasks,
        )?;

        // The IndexerExecutor handles the Sync and Fetch of checkpoints from a Fullnode
        let mut executor = IndexerExecutor::new(
            // Read from sqlite file the latest synced checkpoint and start fetching the next
            // checkpoint
            SqliteProgressStore::new(pool_progress_store),
            // Based on how many workers do we have we may increase this value, what it does under
            // the hood is to calculate the channel capacity by this formula `number_of_jobs *
            // MAX_CHECKPOINTS_IN_PROGRESS`, where MAX_CHECKPOINTS_IN_PROGRESS = 10000
            1,
            DataIngestionMetrics::new(&registry),
            cancel_token.clone(),
        );

        // Register the CheckpointWorker which will handle the CheckpointData once
        // fetched by the CheckpointReader
        let worker = WorkerPool::new(
            CheckpointWorker::new(pool, indexer_config.package_id),
            "primary".to_owned(),
            indexer_config.download_queue_size,
            Default::default(),
        );
        executor.register(worker).await?;

        // Run the IndexerExecutor in a separate task
        tasks.spawn(async move {
            executor
                .run_with_config(CheckpointReaderConfig {
                    remote_store_url: Some(RemoteUrl::Fullnode(
                        indexer_config.remote_store_url.into(),
                    )),
                    reader_options: ReaderOptions {
                        batch_size: indexer_config.download_queue_size,
                        data_limit: indexer_config.checkpoint_processing_batch_data_limit,
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .await?;
            Ok(())
        });

        Ok(Self {
            tasks,
            cancel_token,
        })
    }

    /// Sends a Shutdown Signal to the `IndexerExecutor` and wait for all tasks
    /// to finish, this will block the execution
    #[tracing::instrument(name = "Indexer", skip(self), err)]
    pub async fn graceful_shutdown(mut self) -> anyhow::Result<()> {
        tracing::info!("Received shutdown Signal");
        self.cancel_token.cancel();
        tracing::info!("Waiting for all tasks to shutdown");
        while let Some(result) = self.tasks.join_next().await {
            match result {
                Ok(Ok(_)) => tracing::info!("Task shutdown successfully"),
                Ok(Err(e)) => tracing::error!("Task returned an error: {e}"),
                Err(e) => tracing::error!("Task panicked or was cancelled: {e}"),
            }
        }
        tracing::info!("All tasks shutdown successfully");
        Ok(())
    }
}
