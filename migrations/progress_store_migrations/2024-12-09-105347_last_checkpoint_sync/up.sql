-- Your SQL goes here
CREATE TABLE IF NOT EXISTS last_checkpoint_sync (
    task_id TEXT NOT NULL PRIMARY KEY,
    sequence_number INTEGER NOT NULL
);
