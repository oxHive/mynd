CREATE TABLE IF NOT EXISTS hive_devices (
    device_id TEXT PRIMARY KEY,
    public_key TEXT NOT NULL,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    joined_at INTEGER NOT NULL,
    revoked_at INTEGER,
    revoked_by TEXT,
    join_record TEXT NOT NULL,
    revocation_record TEXT
);

CREATE INDEX IF NOT EXISTS idx_hive_devices_status ON hive_devices(status);
