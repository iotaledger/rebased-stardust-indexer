// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! Checkpoint syncing Handlers for the Indexer

use std::collections::HashMap;

use iota_data_ingestion_core::{DataIngestionMetrics, IndexerExecutor, ReaderOptions, WorkerPool};
use iota_types::messages_checkpoint::CheckpointSequenceNumber;
use tokio::{sync::oneshot, task::JoinHandle};
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
    // TODO: This should be replaced with a CancellationToken
    // https://github.com/iotaledger/iota/issues/4383
    shutdown_tx: oneshot::Sender<()>,
    handle: JoinHandle<anyhow::Result<ExecutorProgress>>,
    prometheus_handle: JoinHandle<anyhow::Result<()>>,
    prom_cancel_token: CancellationToken,
}

impl Indexer {
    /// Init the Checkpoint synchronization from a Fullnode
    pub async fn init(
        pool: ConnectionPool,
        pool_progress_store: ConnectionPool,
        indexer_config: Box<IndexerConfig>,
    ) -> Result<Self, anyhow::Error> {
        // Set up the Prometheus metrics service
        let prom_cancel_token = CancellationToken::new();
        let (registry, prom_handle) = spawn_prometheus_server(
            indexer_config.metrics_address.clone(),
            prom_cancel_token.clone(),
        )?;

        // Notify the IndexerExecutor to gracefully shutdown
        // NOTE: this will be replaced by a CancellationToken once this issue will be
        // resolved: https://github.com/iotaledger/iota/issues/4383
        let (exit_sender, exit_receiver) = tokio::sync::oneshot::channel();

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
        );

        // Register the CheckpointWorker which will handle the CheckpointData once
        // fetched by the CheckpointReader
        let worker = WorkerPool::new(
            CheckpointWorker::new(pool, indexer_config.package_id),
            "primary".to_owned(),
            indexer_config.download_queue_size,
        );
        executor.register(worker).await?;

        let data_ingestion_path = tempfile::tempdir()?.into_path();

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
            exit_receiver,
        ));

        Ok(Self {
            shutdown_tx: exit_sender,
            handle,
            prometheus_handle: prom_handle,
            prom_cancel_token,
        })
    }

    /// Sends a Shutdown Signal to the `IndexerExecutor` and wait for the task
    /// to finish, this will block the execution
    #[tracing::instrument(name = "Indexer", skip(self), err)]
    pub async fn graceful_shutdown(self) -> anyhow::Result<()> {
        tracing::info!("Received shutdown Signal");
        _ = self.shutdown_tx.send(());
        self.prom_cancel_token.cancel();
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
