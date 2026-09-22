ALTER TABLE memories ADD COLUMN hive_content_hash TEXT;

CREATE TABLE IF NOT EXISTS hive_tombstones (
    memory_id  TEXT PRIMARY KEY,
    deleted_at INTEGER NOT NULL
);
