CREATE TABLE IF NOT EXISTS hive_peer_status (
    device_id             TEXT PRIMARY KEY,
    address               TEXT,
    online                INTEGER NOT NULL DEFAULT 0,
    last_synced_at        INTEGER,
    pending_conflict_count INTEGER NOT NULL DEFAULT 0
);
