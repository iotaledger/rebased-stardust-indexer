//! Checkpoint syncing Handlers for the Indexer to use

use async_trait::async_trait;
pub use config::IndexerConfig;
use iota_data_ingestion_core::{DataIngestionMetrics, IndexerExecutor, ReaderOptions, WorkerPool};
use progress_store::SqliteProgressStore;
use prometheus::Registry;
use tokio::{sync::oneshot, task::JoinHandle};
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};
use worker::CheckpointWorker;

use crate::db::ConnectionPool;

mod config;
mod progress_store;
mod worker;

/// The `IndexerHandler` is the main logic behind the checkpoint
/// synchronization from a Fullnode, it handles the initialization and execution
/// of the `IndexerExecutor` in background in as task and provide an interface
/// to gracefully shutdown it
#[derive(Debug)]
pub struct IndexerHandler {
    // TODO: This should be replaced with a CancellationToken
    // https://github.com/iotaledger/iota/issues/4383
    shutdown_tx: oneshot::Sender<()>,
    handle: JoinHandle<()>,
}

impl IndexerHandler {
    /// Init the Checkpoint synchronization from a Fullnode
    pub async fn init(
        pool: ConnectionPool,
        indexer_config: IndexerConfig,
    ) -> Result<Self, anyhow::Error> {
        if indexer_config.reset_db {
            reset_database(&pool)?;
        }

        // Notify the IndexerExecutor to gracefully shutdown
        // NOTE: this will be replaced by a CancellationToken once this issue will be
        // resolved: https://github.com/iotaledger/iota/issues/4383
        let (tx, rx) = tokio::sync::oneshot::channel();

        // The IndexerExecutor handles the Sync and Fetch of checpoints from a Fullnode
        let mut executor = IndexerExecutor::new(
            // Read from file the latest syned checkpoint and start fetching the next checkpoint
            SqliteProgressStore::new(pool.clone()),
            // Based on ho many workers do we have we may increaee this value, what it does under
            // the hood is to calculate the channel capacity by this formula `number_of_jobs *
            // MAX_CHECKPOINTS_IN_PROGRESS`, where MAX_CHECKPOINTS_IN_PROGRESS = 10000
            1,
            DataIngestionMetrics::new(&Registry::default()),
        );

        // Register the CheckpointWorker which will handle the CheckpointData once
        // fetched by the CheckpointReader
        let worker = WorkerPool::new(
            CheckpointWorker::new(pool, indexer_config.package_id),
            "primary".to_owned(),
            indexer_config.download_queue_size,
        );
        executor.register(worker).await?;

        let data_ingestion_path = indexer_config.data_ingestion_path.map_or_else(
            || tempfile::tempdir().map(|tmp_dir| tmp_dir.into_path()),
            Ok,
        )?;

        // Run the IndexerExecutor in a separate task
        let handle = tokio::spawn(async move {
            executor
                .run(
                    data_ingestion_path,
                    Some(indexer_config.remote_store_url.to_string()),
                    vec![],
                    ReaderOptions {
                        batch_size: indexer_config.download_queue_size,
                        data_limit: indexer_config.checkpoint_porcessing_batch_data_limit,
                        ..Default::default()
                    },
                    rx,
                )
                .await
                .unwrap();
        });

        Ok(Self {
            shutdown_tx: tx,
            handle,
        })
    }

    /// Sends a Shutdown Signal to the `IndexerExecutor` and wait for the task
    /// to finish, this will block the execution
    #[tracing::instrument(skip(self), err)]
    pub async fn graceful_shutdown(self) -> anyhow::Result<()> {
        _ = self.shutdown_tx.send(());
        self.handle.await.map_err(Into::into)
    }
}

/// Reset the database by reverting all migrations
fn reset_database(pool: &ConnectionPool) -> anyhow::Result<()> {
    pool.revert_all_migrations()
        .and_then(|_| pool.run_migrations())
}

#[async_trait]
impl IntoSubsystem<anyhow::Error> for IndexerHandler {
    async fn run(self, subsys: SubsystemHandle) -> anyhow::Result<()> {
        subsys.on_shutdown_requested().await;
        self.graceful_shutdown().await
    }
}
