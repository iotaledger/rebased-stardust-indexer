// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! Checkpoint syncing Handlers for the Indexer

use std::collections::HashMap;

use iota_data_ingestion_core::{
    DataIngestionMetrics, IndexerExecutor, IngestionError, ReaderOptions, WorkerPool,
};
use iota_types::messages_checkpoint::CheckpointSequenceNumber;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    db::ConnectionPool,
    metrics::spawn_prometheus_server,
    sync::{IndexerConfig, progress_store::SqliteProgressStore, worker::CheckpointWorker},
};

type ExecutorProgress = HashMap<String, CheckpointSequenceNumber>;

/// The `Indexer` encapsulates the main logic behind the checkpoint
/// synchronization from a Fullnode.
///
/// It handles the initialization and execution
/// of the `IndexerExecutor` in background in as task and provide an interface
/// to gracefully shutdown it
#[derive(Debug)]
pub struct Indexer {
    handle: JoinHandle<Result<ExecutorProgress, IngestionError>>,
    prometheus_handle: JoinHandle<anyhow::Result<()>>,
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
        let (registry, prom_handle) =
            spawn_prometheus_server(indexer_config.metrics_address, cancel_token.clone())?;

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

        let data_ingestion_path = tempfile::tempdir()?.keep();

        // Run the IndexerExecutor in a separate task
        let handle = tokio::spawn(executor.run(
            data_ingestion_path,
            Some(indexer_config.remote_store_url.to_string()),
            vec![],
            ReaderOptions {
                batch_size: indexer_config.download_queue_size,
                data_limit: indexer_config.checkpoint_processing_batch_data_limit,
                ..Default::default()
            },
        ));

        Ok(Self {
            handle,
            prometheus_handle: prom_handle,
            cancel_token,
        })
    }

    /// Sends a Shutdown Signal to the `IndexerExecutor` and wait for the task
    /// to finish, this will block the execution
    #[tracing::instrument(name = "Indexer", skip(self), err)]
    pub async fn graceful_shutdown(self) -> anyhow::Result<()> {
        tracing::info!("Received shutdown Signal");
        self.cancel_token.cancel();
        tracing::info!("Wait for task to shutdown");
        self.handle
            .await?
            .inspect(|_| tracing::info!("Task shutdown successfully"))?;
        self.prometheus_handle
            .await?
            .inspect(|_| tracing::info!("Task shutdown successfully"))?;

        Ok(())
    }
}
