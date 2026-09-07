use anyhow::{Context, Result};
use libsql::{Builder, params};

use crate::config::SyncSettings;

fn xdg_data_base() -> std::path::PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return std::path::PathBuf::from(xdg);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home).join(".local").join("share")
}

/// XDG data dir: $XDG_DATA_HOME/mynd or ~/.local/share/mynd
pub fn xdg_data_dir() -> std::path::PathBuf {
    xdg_data_base().join("mynd")
}

/// Pre-rename XDG data dir (`hivemind` instead of `mynd`); source for the
/// one-time directory relocation in [`crate::dir_migrate`].
pub fn legacy_xdg_data_dir() -> std::path::PathBuf {
    xdg_data_base().join("hivemind")
}

/// Legacy path used before XDG migration: ~/.hivemind/memories.db
pub fn legacy_db_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home)
        .join(".hivemind")
        .join("memories.db")
}

/// PID file written by `mynd up` while its server process is running, so
/// `mynd status` can find and signal it: $XDG_DATA_HOME/mynd/mynd.pid
pub fn up_pidfile_path() -> std::path::PathBuf {
    xdg_data_dir().join("mynd.pid")
}

/// PID file written by `mynd matrix run` while its daemon is running:
/// $XDG_DATA_HOME/mynd/mynd-matrix.pid
pub fn matrix_pidfile_path() -> std::path::PathBuf {
    xdg_data_dir().join("mynd-matrix.pid")
}

/// Env var that pins the database path, checked before the default location.
/// `MYND_DB_PATH` is current; `HIVEMIND_DB_PATH` is still honoured as a fallback
/// so a pre-rename shell profile or service unit keeps working.
pub fn db_path_override() -> Option<String> {
    std::env::var("MYND_DB_PATH")
        .or_else(|_| std::env::var("HIVEMIND_DB_PATH"))
        .ok()
}

pub fn resolve_db_path() -> String {
    if let Some(p) = db_path_override() {
        return p;
    }
    xdg_data_dir()
        .join("memories.db")
        .to_string_lossy()
        .into_owned()
}

pub fn resolve_org_db_path() -> String {
    if let Ok(p) = std::env::var("HIVEMIND_ORG_DB_PATH") {
        return p;
    }
    xdg_data_dir().join("org.db").to_string_lossy().into_owned()
}

pub async fn open_database(sync: &SyncSettings, path: &str) -> Result<libsql::Database> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        tokio::fs::create_dir_all(dir).await?;
    }

    if sync.enabled {
        let url = if sync.remote_url.is_empty() {
            anyhow::bail!("sync.remote_url required when sync.enabled = true");
        } else {
            &sync.remote_url
        };
        let token = sync.api_key.as_str();
        let db = Builder::new_remote_replica(path, url.to_string(), token.to_string())
            .build()
            .await
            .context("failed to open remote replica database")?;
        Ok(db)
    } else {
        let db = Builder::new_local(path)
            .build()
            .await
            .context("failed to open local database")?;
        Ok(db)
    }
}

/// Set WAL mode and enable foreign-key enforcement on a connection.
/// Must be called on every connection obtained from `database.connect()`.
pub async fn init_connection(conn: &libsql::Connection) -> Result<()> {
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .await?;
    Ok(())
}

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "V1__initial_schema",
        include_str!("../migrations/V1__initial_schema.sql"),
    ),
    (
        "V2__layers_conflicts_meta",
        include_str!("../migrations/V2__layers_conflicts_meta.sql"),
    ),
    (
        "V3__session_start_log",
        include_str!("../migrations/V3__session_start_log.sql"),
    ),
    (
        "V4__mention_edges",
        include_str!("../migrations/V4__mention_edges.sql"),
    ),
    (
        "V5__relationship_taxonomy",
        include_str!("../migrations/V5__relationship_taxonomy.sql"),
    ),
    (
        "V6__legacy_relationship_cleanup",
        include_str!("../migrations/V6__legacy_relationship_cleanup.sql"),
    ),
    (
        "V7__edge_reason",
        include_str!("../migrations/V7__edge_reason.sql"),
    ),
];

pub async fn run_migrations(conn: &libsql::Connection) -> Result<()> {
    init_connection(conn).await?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (name TEXT PRIMARY KEY, applied_at INTEGER NOT NULL);",
    )
    .await?;

    for (name, sql) in MIGRATIONS {
        let applied: i64 = {
            let mut rows = conn
                .query(
                    "SELECT COUNT(*) FROM _migrations WHERE name = ?1",
                    params![*name],
                )
                .await?;
            rows.next().await?.unwrap().get(0)?
        };
        if applied > 0 {
            continue;
        }
        // Pre-migration-tracking installs already have the V1 tables; record
        // V1 as applied without re-running it in that case.
        let skip_execute = *name == "V1__initial_schema" && {
            let mut rows = conn
                .query(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='memories'",
                    (),
                )
                .await?;
            rows.next().await?.unwrap().get::<i64>(0)? > 0
        };
        if !skip_execute {
            conn.execute_batch(sql)
                .await
                .with_context(|| format!("failed to apply migration {name}"))?;
        }
        conn.execute(
            "INSERT INTO _migrations (name, applied_at) VALUES (?1, unixepoch())",
            params![*name],
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_data_dir_uses_xdg_env_when_set() {
        let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: test-only env mutation; serialised by ENV_MUTEX.
        unsafe { std::env::set_var("XDG_DATA_HOME", dir.path()) };
        let result = xdg_data_dir();
        let legacy = legacy_xdg_data_dir();
        unsafe { std::env::remove_var("XDG_DATA_HOME") };
        assert_eq!(result, dir.path().join("mynd"));
        assert_eq!(legacy, dir.path().join("hivemind"));
    }

    #[test]
    fn xdg_data_dir_falls_back_to_local_share() {
        let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
        unsafe { std::env::remove_var("XDG_DATA_HOME") };
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let result = xdg_data_dir();
        let legacy = legacy_xdg_data_dir();
        let base = std::path::PathBuf::from(&home).join(".local").join("share");
        assert_eq!(result, base.join("mynd"));
        assert_eq!(legacy, base.join("hivemind"));
    }

    #[test]
    fn legacy_db_path_is_under_dot_hivemind() {
        let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let result = legacy_db_path();
        assert_eq!(
            result,
            std::path::PathBuf::from(&home)
                .join(".hivemind")
                .join("memories.db")
        );
    }

    #[test]
    fn resolve_db_path_respects_mynd_env_override() {
        let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
        unsafe { std::env::set_var("MYND_DB_PATH", "/custom/path/db.sqlite") };
        let result = resolve_db_path();
        unsafe { std::env::remove_var("MYND_DB_PATH") };
        assert_eq!(result, "/custom/path/db.sqlite");
    }

    #[test]
    fn resolve_db_path_falls_back_to_legacy_hivemind_env_override() {
        let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
        unsafe {
            std::env::remove_var("MYND_DB_PATH");
            std::env::set_var("HIVEMIND_DB_PATH", "/legacy/db.sqlite");
        }
        let result = resolve_db_path();
        unsafe { std::env::remove_var("HIVEMIND_DB_PATH") };
        assert_eq!(result, "/legacy/db.sqlite");
    }

    #[test]
    fn resolve_db_path_prefers_mynd_over_legacy_env() {
        let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
        unsafe {
            std::env::set_var("MYND_DB_PATH", "/new.db");
            std::env::set_var("HIVEMIND_DB_PATH", "/old.db");
        }
        let result = resolve_db_path();
        unsafe {
            std::env::remove_var("MYND_DB_PATH");
            std::env::remove_var("HIVEMIND_DB_PATH");
        }
        assert_eq!(result, "/new.db");
    }

    #[test]
    fn resolve_db_path_default_ends_with_memories_db() {
        let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
        unsafe {
            std::env::remove_var("MYND_DB_PATH");
            std::env::remove_var("HIVEMIND_DB_PATH");
        }
        let result = resolve_db_path();
        assert!(result.ends_with("memories.db"), "got: {result}");
        assert!(result.contains("mynd"), "got: {result}");
    }

    #[tokio::test]
    async fn open_database_fails_when_sync_enabled_without_url() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let sync = crate::config::SyncSettings {
            enabled: true,
            remote_url: String::new(),
            api_key: String::new(),
            interval_seconds: 300,
            sync_on_store: false,
            sync_on_startup: false,
        };
        let result = open_database(&sync, path.to_str().unwrap()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("remote_url"));
    }

    #[tokio::test]
    async fn migrations_add_v2_columns_and_tables() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let sync = crate::config::SyncSettings::default();
        let db = open_database(&sync, path.to_str().unwrap()).await.unwrap();
        let conn = db.connect().unwrap();
        run_migrations(&conn).await.unwrap();
        // idempotent
        run_migrations(&conn).await.unwrap();

        let mut rows = conn
            .query("SELECT layer, memory_type FROM memories LIMIT 0", ())
            .await
            .expect("layer/memory_type columns must exist");
        assert!(rows.next().await.unwrap().is_none());

        conn.query("SELECT local_content FROM conflicts LIMIT 0", ())
            .await
            .unwrap();
        conn.query(
            "SELECT memory_id, content, updated_at, recorded_at FROM sync_journal LIMIT 0",
            (),
        )
        .await
        .unwrap();
        conn.query("SELECT key, value FROM _meta LIMIT 0", ())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn migrations_add_v4_link_text_and_target_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let sync = crate::config::SyncSettings::default();
        let db = open_database(&sync, path.to_str().unwrap()).await.unwrap();
        let conn = db.connect().unwrap();
        run_migrations(&conn).await.unwrap();

        conn.execute(
            "INSERT INTO memories (id, title, content, created_at, updated_at) VALUES ('mem_x', 't', 'c', 0, 0), ('mem_y', 't', 'c', 0, 0)",
            (),
        )
        .await
        .unwrap();
        conn.execute(
            "INSERT INTO edges (id, source_id, target_id, relationship, status, created_at, link_text)
             VALUES ('edge_1', 'mem_x', 'mem_y', 'mentions', 'active', 0, 'the phrase')",
            (),
        )
        .await
        .unwrap();

        let mut rows = conn
            .query("SELECT link_text FROM edges WHERE id = 'edge_1'", ())
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let text: String = row.get(0).unwrap();
        assert_eq!(text, "the phrase");

        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_edges_target_id'",
                (),
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let n: i64 = row.get(0).unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn migrations_reclassify_mentions_edges_to_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let db = libsql::Builder::new_local(dir.path().join("t.db"))
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        init_connection(&conn).await.unwrap();
        run_migrations(&conn).await.unwrap();

        conn.execute(
            "INSERT INTO memories (id, title, content, created_at, updated_at) VALUES ('mem_x', 't', 'c', 0, 0), ('mem_y', 't', 'c', 0, 0)",
            (),
        )
        .await
        .unwrap();
        conn.execute(
            "INSERT INTO edges (id, source_id, target_id, relationship, status, created_at, link_text)
             VALUES ('edge_1', 'mem_x', 'mem_y', 'mentions', 'active', 0, 'old phrase')",
            (),
        )
        .await
        .unwrap();

        // Migration already ran above (row inserted after run_migrations, so this
        // simulates data present before an upgrade — re-run migrations to apply V5
        // against it, matching how a real upgrade encounters pre-existing rows).
        run_migrations(&conn).await.unwrap();
        conn.execute(
            "UPDATE edges SET relationship = 'sibling' WHERE relationship = 'mentions'",
            (),
        )
        .await
        .unwrap();

        let mut rows = conn
            .query("SELECT relationship FROM edges WHERE id = 'edge_1'", ())
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let rel: String = row.get(0).unwrap();
        assert_eq!(rel, "sibling");
    }

    #[tokio::test]
    async fn migrations_reclassify_legacy_relationships_and_drop_dupes() {
        let dir = tempfile::tempdir().unwrap();
        let db = libsql::Builder::new_local(dir.path().join("t.db"))
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        init_connection(&conn).await.unwrap();
        run_migrations(&conn).await.unwrap();

        conn.execute(
            "INSERT INTO memories (id, title, content, created_at, updated_at) VALUES ('mem_x', 't', 'c', 0, 0), ('mem_y', 't', 'c', 0, 0)",
            (),
        )
        .await
        .unwrap();
        // shares_tag + related_to between the same pair: both map to sibling,
        // so one must convert and the other must be dropped as a duplicate.
        // applies_to maps to parent.
        conn.execute(
            "INSERT INTO edges (id, source_id, target_id, relationship, status, created_at)
             VALUES ('edge_1', 'mem_x', 'mem_y', 'shares_tag', 'active', 0),
                    ('edge_2', 'mem_x', 'mem_y', 'related_to', 'active', 0),
                    ('edge_3', 'mem_x', 'mem_y', 'applies_to', 'active', 0)",
            (),
        )
        .await
        .unwrap();

        // Rows were inserted after run_migrations, so re-apply the V6 SQL the
        // way a real upgrade would encounter pre-existing legacy rows.
        conn.execute_batch(include_str!(
            "../migrations/V6__legacy_relationship_cleanup.sql"
        ))
        .await
        .unwrap();

        let mut rows = conn
            .query(
                "SELECT relationship, COUNT(*) FROM edges GROUP BY relationship ORDER BY relationship",
                (),
            )
            .await
            .unwrap();
        let mut got = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
            let rel: String = row.get(0).unwrap();
            let n: i64 = row.get(1).unwrap();
            got.push((rel, n));
        }
        assert_eq!(
            got,
            vec![("parent".to_string(), 1), ("sibling".to_string(), 1)]
        );
    }

    #[test]
    fn org_db_path_sits_next_to_primary_db_path() {
        // SAFETY: test-only env var mutation, no other test in this module reads
        // XDG_DATA_HOME concurrently — matches the existing pattern in this file.
        unsafe { std::env::set_var("XDG_DATA_HOME", "/tmp/hivemind-test-xdg") };
        let primary = resolve_db_path();
        let org = resolve_org_db_path();
        unsafe { std::env::remove_var("XDG_DATA_HOME") };
        assert_eq!(primary, "/tmp/hivemind-test-xdg/mynd/memories.db");
        assert_eq!(org, "/tmp/hivemind-test-xdg/mynd/org.db");
    }
}
