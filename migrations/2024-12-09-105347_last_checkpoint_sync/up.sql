-- Your SQL goes here
CREATE TABLE IF NOT EXISTS last_checkpoint_sync (
    task_id TEXT NOT NULL PRIMARY KEY,
    sequence_number BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS checkpoint_sequence_number ON last_checkpoint_sync (task_id, sequence_number);
