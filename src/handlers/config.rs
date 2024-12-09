use std::path::PathBuf;

use clap::Args;
use iota_types::{STARDUST_PACKAGE_ID, base_types::ObjectID};
use url::Url;

/// Max queue size of checkpoints for the Indexer to process
const DOWNLOAD_QUEUE_SIZE: usize = 200;
/// Limit indexing parallelism on big checkpoints to avoid OOM,
/// by limiting the total size of batch checkpoints to ~20MB
const CHECKPOINT_PROCESSING_BATCH_DATA_LIMIT: u64 = 20000000;

#[derive(Args, Debug, Clone)]
pub struct IndexerConfig {
    #[arg(long, value_parser = parse_url, default_value = "https://checkpoints.mainnet.iota.io",)]
    /// Option to to synchronize data from a remote Fullnode trough REST API
    pub remote_store_url: Url,
    #[arg(long)]
    /// Option to synchronize data from the Fullnode using a shared path,
    /// provided the Indexer operates on the same machine as the Fullnode.
    pub data_ingestion_path: Option<PathBuf>,
    #[arg(long, default_value = "checkpoint_progress.json")]
    /// File where the Indexer saves the latest synchronization checkpoint,
    /// which is necessary for restarts.
    pub checkpoint_progress_file: PathBuf,
    /// Max queue size of checkpoints for the Indexer to process
    #[arg(long, default_value = DOWNLOAD_QUEUE_SIZE.to_string())]
    #[arg(env = "DOWNLOAD_QUEUE_SIZE")]
    pub download_queue_size: usize,
    /// Limit indexing parallelism on big checkpoints to avoid OOM,
    /// by limiting the total size of batch checkpoints to ~20MB.
    #[arg(long, default_value = CHECKPOINT_PROCESSING_BATCH_DATA_LIMIT.to_string())]
    #[arg(env = "CHECKPOINT_PROCESSING_BATCH_DATA_LIMIT")]
    pub checkpoint_porcessing_batch_data_limit: usize,
    /// Reset the current dabatase
    #[arg(long)]
    pub reset_db: bool,
    #[clap(short, long, value_delimiter = ' ', num_args = 1.., default_value = STARDUST_PACKAGE_ID.to_string())]
    pub package_id: Vec<ObjectID>,
}

/// Validate the provided raw string into a Url
fn parse_url(value: &str) -> anyhow::Result<Url> {
    Url::parse(value).map_err(Into::into)
}
