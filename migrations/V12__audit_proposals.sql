CREATE TABLE IF NOT EXISTS audit_proposals (
    id         TEXT PRIMARY KEY,
    project    TEXT NOT NULL,
    layer      TEXT NOT NULL DEFAULT 'workspace',
    kind       TEXT NOT NULL,
    summary    TEXT NOT NULL,
    reason     TEXT NOT NULL,
    ops        TEXT NOT NULL,
    base       TEXT NOT NULL,
    status     TEXT NOT NULL DEFAULT 'pending',
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audit_proposals_project_status
    ON audit_proposals(project, status);
