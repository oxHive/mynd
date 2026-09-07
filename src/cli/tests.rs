use super::*;
use std::fs;
use std::path::Path;

#[test]
fn status_plain_flag_parses() {
    let cli = Cli::try_parse_from(["mynd", "status", "--plain"]).unwrap();
    match cli.command {
        Some(Command::Status { plain }) => assert!(plain),
        _ => panic!("expected Status command"),
    }
}

#[test]
fn parses_matrix_login_subcommand() {
    let cli = Cli::parse_from(["mynd", "matrix", "login"]);
    assert!(matches!(
        cli.command,
        Some(Command::Matrix {
            action: MatrixAction::Login
        })
    ));
}

#[test]
fn parses_matrix_run_subcommand() {
    let cli = Cli::parse_from(["mynd", "matrix", "run"]);
    assert!(matches!(
        cli.command,
        Some(Command::Matrix {
            action: MatrixAction::Run { debug: false }
        })
    ));
}

#[test]
fn parses_matrix_send_subcommand() {
    let cli = Cli::parse_from(["mynd", "matrix", "send", "@oxgrad:matrix.org", "hi"]);
    assert!(matches!(
        cli.command,
        Some(Command::Matrix {
            action: MatrixAction::Send { user_id, message }
        }) if user_id == "@oxgrad:matrix.org" && message == "hi"
    ));
}

#[test]
fn parses_matrix_status_subcommand() {
    let cli = Cli::parse_from(["mynd", "matrix", "status"]);
    assert!(matches!(
        cli.command,
        Some(Command::Matrix {
            action: MatrixAction::Status
        })
    ));
}

#[test]
fn up_plain_flag_parses() {
    let cli = Cli::try_parse_from(["mynd", "up", "--plain"]).unwrap();
    match cli.command {
        Some(Command::Up { headless, plain }) => {
            assert!(!headless);
            assert!(plain);
        }
        _ => panic!("expected Up command"),
    }
}

/// Default server settings, built the same way `hivemind status` does when
/// no global config exists.
fn default_settings() -> crate::config::ServerSettings {
    crate::config::load_server_settings(Path::new("/nonexistent/mynd-global.toml")).unwrap()
}

fn sample_result(loaded: bool, skipped: bool) -> crate::session::SessionStartResult {
    use crate::session::{LoadedEntry, SkipReason, SkippedEntry};
    use crate::store::MemoryEntry;

    let loaded_vec = if loaded {
        vec![LoadedEntry {
            entry: MemoryEntry {
                id: "mem_1".to_string(),
                title: "pref a".to_string(),
                content: "short content a".to_string(),
                tags: vec![],
                created_at: 0,
                updated_at: 0,
                token_count: None,
                layer: "workspace".to_string(),
                memory_type: "project".to_string(),
            },
            tokens: 5,
            source: crate::config::RecallSource::Project,
        }]
    } else {
        vec![]
    };
    let skipped_vec = if skipped {
        vec![SkippedEntry {
            query: "missing".to_string(),
            reason: SkipReason::NotFound,
        }]
    } else {
        vec![]
    };
    crate::session::SessionStartResult {
        project: "test-proj".to_string(),
        loaded: loaded_vec,
        skipped: skipped_vec,
        used_tokens: 5,
        max_tokens: 2000,
        memories_recalled: if loaded { 1 } else { 0 },
    }
}

#[test]
fn render_session_start_text_wraps_in_mynd_context_tags() {
    let result = sample_result(true, false);
    let out = render_session_start(&result, false);
    assert!(out.contains("<mynd-context"));
    assert!(out.contains("pref a"));
    assert!(out.contains("short content a"));
}

#[test]
fn render_session_start_text_empty_when_nothing_loaded_or_skipped() {
    let result = sample_result(false, false);
    let out = render_session_start(&result, false);
    assert!(out.is_empty());
}

#[test]
fn render_session_start_json_parses_and_matches_shape() {
    let result = sample_result(true, true);
    let out = render_session_start(&result, true);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["project"], "test-proj");
    assert_eq!(v["context_loaded"][0]["title"], "pref a");
    assert_eq!(v["skipped"][0]["query"], "missing");
    assert_eq!(v["budget"]["max_tokens"], 2000);
}

#[test]
fn write_atomic_creates_file_with_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.txt");
    write_atomic(&path, "hello world").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "hello world");
}

#[test]
fn write_atomic_overwrites_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.txt");
    write_atomic(&path, "first").unwrap();
    write_atomic(&path, "second").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "second");
}

#[test]
fn write_if_absent_creates_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.txt");
    let (p, status) = write_if_absent(&path, "content").unwrap();
    assert_eq!(status, "created");
    assert_eq!(p, path);
    assert_eq!(fs::read_to_string(&path).unwrap(), "content");
}

#[test]
fn write_if_absent_skips_when_present() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("existing.txt");
    fs::write(&path, "original").unwrap();
    let (_, status) = write_if_absent(&path, "new content").unwrap();
    assert_eq!(status, "exists");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "original",
        "must not overwrite"
    );
}

#[test]
fn ensure_line_appends_to_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".gitignore");
    let (_, status) = ensure_line(&path, "*.log").unwrap();
    assert_eq!(status, "created");
    assert!(fs::read_to_string(&path).unwrap().contains("*.log"));
}

#[test]
fn ensure_line_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".gitignore");
    fs::write(&path, "*.log\n").unwrap();
    let (_, status) = ensure_line(&path, "*.log").unwrap();
    assert_eq!(status, "exists");
    assert_eq!(
        fs::read_to_string(&path).unwrap().matches("*.log").count(),
        1
    );
}

#[test]
fn ensure_line_appends_to_existing_file_without_trailing_newline() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".gitignore");
    fs::write(&path, "node_modules").unwrap();
    ensure_line(&path, "*.log").unwrap();
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("node_modules"));
    assert!(content.contains("*.log"));
}

#[test]
fn append_block_if_absent_appends_when_marker_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("CLAUDE.md");
    fs::write(&path, "# My rules\n").unwrap();
    let (_, status) =
        append_block_if_absent(&path, "# HiveMind", "# HiveMind\nsome block\n").unwrap();
    assert_eq!(status, "created");
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("My rules"));
    assert!(content.contains("# HiveMind"));
}

#[test]
fn append_block_if_absent_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("CLAUDE.md");
    fs::write(&path, "# HiveMind\nexisting block\n").unwrap();
    let (_, status) =
        append_block_if_absent(&path, "# HiveMind", "# HiveMind\nnew block\n").unwrap();
    assert_eq!(status, "exists");
    assert_eq!(
        fs::read_to_string(&path)
            .unwrap()
            .matches("# HiveMind")
            .count(),
        1
    );
}

const SAMPLE_LEGACY_BLOCK: &str = "# HiveMind Memory System\n\
\n\
You have access to HiveMind via MCP tools: memory_store, hivemind_session_start.\n\
\n\
1. Check if .hivemind.toml exists in the project root.\n\
2. If it exists, call `hivemind_session_start` immediately.\n\
\n\
## Suggest storing — never auto-store\n\
\n\
Wait for explicit confirmation before calling memory_store.\n";

#[test]
fn migrate_global_claude_block_rewrites_legacy_block_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("CLAUDE.md");
    fs::write(&path, SAMPLE_LEGACY_BLOCK).unwrap();

    let changed = migrate_global_claude_block(&path).unwrap();

    assert!(changed);
    let gc = fs::read_to_string(&path).unwrap();
    assert!(gc.contains("# Mynd Memory System"));
    assert!(!gc.contains("# HiveMind Memory System"));
    assert!(gc.contains("mynd_session_start"));
    assert!(!gc.contains("hivemind_session_start"));
}

#[test]
fn migrate_global_claude_block_preserves_user_content_around_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("CLAUDE.md");
    fs::write(
        &path,
        format!("# My rules\n\nAlways write tests first.\n\n{SAMPLE_LEGACY_BLOCK}\n# After\n\nkeep me\n"),
    )
    .unwrap();

    assert!(migrate_global_claude_block(&path).unwrap());

    let gc = fs::read_to_string(&path).unwrap();
    assert!(gc.contains("Always write tests first."));
    assert!(gc.contains("# After\n\nkeep me"));
    assert!(gc.contains("# Mynd Memory System"));
    assert!(!gc.contains("HiveMind"));
}

#[test]
fn migrate_global_claude_block_is_noop_when_current_or_absent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("CLAUDE.md");

    // no block at all: file untouched, not created
    assert!(!migrate_global_claude_block(&path).unwrap());
    assert!(!path.exists());

    // current block already present
    fs::write(&path, GLOBAL_CLAUDE_BLOCK).unwrap();
    assert!(!migrate_global_claude_block(&path).unwrap());
}

#[test]
fn scaffold_migrates_an_existing_legacy_global_block() {
    let proj = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let global = home.path().join(".claude").join("CLAUDE.md");
    fs::create_dir_all(global.parent().unwrap()).unwrap();
    fs::write(&global, SAMPLE_LEGACY_BLOCK).unwrap();

    scaffold(proj.path(), home.path(), cfg.path()).unwrap();

    let gc = fs::read_to_string(&global).unwrap();
    assert_eq!(gc.matches("Mynd Memory System").count(), 1);
    assert!(!gc.contains("HiveMind"));
}

#[test]
fn scaffold_creates_all_files() {
    let proj = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let report = scaffold(proj.path(), home.path(), cfg.path()).unwrap();

    assert!(proj.path().join(".mynd.toml").is_file());
    assert!(proj.path().join(".mynd.local.toml").is_file());
    assert!(proj.path().join("CLAUDE.md").is_file());
    assert!(proj.path().join(".gitignore").is_file());
    assert!(home.path().join(".claude").join("CLAUDE.md").is_file());
    assert!(cfg.path().join("config.toml").is_file());

    let gi = fs::read_to_string(proj.path().join(".gitignore")).unwrap();
    assert!(gi.contains(".mynd.local.toml"));
    let gc = fs::read_to_string(home.path().join(".claude").join("CLAUDE.md")).unwrap();
    assert!(gc.contains("Mynd Memory System"));
    let pj = fs::read_to_string(proj.path().join(".mynd.toml")).unwrap();
    let dirname = proj.path().file_name().unwrap().to_string_lossy();
    assert!(pj.contains(&*dirname));

    assert!(report.iter().all(|(_, status)| *status == "created"));
}

#[test]
fn scaffold_is_idempotent_and_does_not_duplicate_global_block() {
    let proj = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();

    scaffold(proj.path(), home.path(), cfg.path()).unwrap();
    let report2 = scaffold(proj.path(), home.path(), cfg.path()).unwrap();

    assert!(report2.iter().all(|(_, status)| *status == "exists"));

    let gc = fs::read_to_string(home.path().join(".claude").join("CLAUDE.md")).unwrap();
    assert_eq!(gc.matches("# Mynd Memory System").count(), 1);
    let gi = fs::read_to_string(proj.path().join(".gitignore")).unwrap();
    assert_eq!(gi.matches(".mynd.local.toml").count(), 1);
}

#[test]
fn scaffold_preserves_existing_user_claude_md() {
    let proj = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();

    // The user already has a customized global CLAUDE.md.
    let global = home.path().join(".claude").join("CLAUDE.md");
    fs::create_dir_all(global.parent().unwrap()).unwrap();
    fs::write(&global, "# My personal rules\nAlways write tests first.\n").unwrap();

    scaffold(proj.path(), home.path(), cfg.path()).unwrap();

    let gc = fs::read_to_string(&global).unwrap();
    assert!(
        gc.contains("My personal rules"),
        "user content must be preserved"
    );
    assert!(
        gc.contains("Always write tests first."),
        "user content must be preserved"
    );
    assert!(
        gc.contains("# Mynd Memory System"),
        "hook block appended"
    );
}

#[tokio::test]
async fn render_status_previews_injection() {
    use crate::{config::SyncSettings, db, store::SqliteStore};

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, db_path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    let store = SqliteStore::new(conn);
    let id = format!("mem_{}", uuid::Uuid::new_v4().simple());
    store
        .store(&crate::store::NewMemoryRow {
            id: &id,
            title: "golang preferences",
            content: "uber/zap, sqlc, pgx v5",
            tags: &["golang".to_string()],
            token_count: None,
            layer: "workspace",
            memory_type: "project",
        })
        .await
        .unwrap();

    let proj = tempfile::tempdir().unwrap();
    std::fs::write(
        proj.path().join(".mynd.toml"),
        "[project]\nname=\"demo\"\n[hooks.on_session_start]\nmax_tokens=2000\nrecalls=[\"golang preferences\"]\n",
    ).unwrap();
    let missing_global = proj.path().join("no-global.toml");

    let out = render_status(
        proj.path(),
        &missing_global,
        &store,
        "/tmp/x.db",
        &[],
        &default_settings(),
        false,
    )
    .await
    .unwrap();
    assert!(out.contains("demo"), "shows project name");
    assert!(
        out.contains("golang preferences"),
        "lists the injected memory"
    );
    assert!(out.contains("Budget:"), "shows the budget line");
    assert!(
        out.contains("1 memories") || out.contains("1 memorie"),
        "shows memory count"
    );
    assert!(
        out.contains("AI clients: none"),
        "shows no registered clients"
    );
}

#[tokio::test]
async fn render_status_shows_registered_clients() {
    use crate::{config::SyncSettings, db, store::SqliteStore};

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, db_path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    let store = SqliteStore::new(conn);

    let proj = tempfile::tempdir().unwrap();
    let missing_global = proj.path().join("no-global.toml");
    let out = render_status(
        proj.path(),
        &missing_global,
        &store,
        "/tmp/x.db",
        &["claude", "cursor"],
        &default_settings(),
        false,
    )
    .await
    .unwrap();
    assert!(
        out.contains("AI clients: claude, cursor"),
        "lists registered clients"
    );
}

#[tokio::test]
async fn render_status_without_config_reports_missing() {
    use crate::{config::SyncSettings, db, store::SqliteStore};

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, db_path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    let store = SqliteStore::new(conn);

    let proj = tempfile::tempdir().unwrap();
    let missing_global = proj.path().join("no-global.toml");
    let out = render_status(
        proj.path(),
        &missing_global,
        &store,
        "/tmp/x.db",
        &[],
        &default_settings(),
        false,
    )
    .await
    .unwrap();
    assert!(
        out.contains("mynd init"),
        "suggests init when no config"
    );
}

#[tokio::test]
async fn build_status_data_matches_render_status_text() {
    use crate::{config::SyncSettings, db, store::SqliteStore};

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, db_path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    let store = SqliteStore::new(conn);
    let id = format!("mem_{}", uuid::Uuid::new_v4().simple());
    store
        .store(&crate::store::NewMemoryRow {
            id: &id,
            title: "golang preferences",
            content: "uber/zap, sqlc, pgx v5",
            tags: &["golang".to_string()],
            token_count: None,
            layer: "workspace",
            memory_type: "project",
        })
        .await
        .unwrap();

    let proj = tempfile::tempdir().unwrap();
    std::fs::write(
        proj.path().join(".mynd.toml"),
        "[project]\nname=\"test-proj\"\n[hooks.on_session_start]\nrecalls=[\"golang preferences\"]\n",
    )
    .unwrap();
    let global_path = dir.path().join("no-global.toml");
    let settings = crate::config::ServerSettings {
        host: "127.0.0.1".into(),
        port: 3456,
        dashboard_port: 3457,
        api_url: "http://127.0.0.1:3456".into(),
        cors_origin: "http://127.0.0.1:3457".into(),
        sync: SyncSettings::default(),
        update: crate::config::UpdateSettings::default(),
        agent: crate::config::AgentSettings::default(),
        guard_predefined_namespaces: true,
    };

    let via_text = render_status(
        proj.path(),
        &global_path,
        &store,
        "test.db",
        &["claude"],
        &settings,
        true,
    )
    .await
    .unwrap();

    let data = build_status_data(
        proj.path(),
        &global_path,
        &store,
        "test.db",
        &["claude"],
        &settings,
        true,
    )
    .await
    .unwrap();
    let via_struct = format_status_text(&data);

    assert_eq!(
        via_text, via_struct,
        "render_status() must stay byte-for-byte identical after the struct extraction"
    );
    assert_eq!(data.memory_count, 1);
    assert_eq!(data.project.as_ref().unwrap().project_name, "test-proj");
    assert_eq!(data.project.as_ref().unwrap().loaded.len(), 1);
    assert_eq!(
        data.project.as_ref().unwrap().loaded[0].title,
        "golang preferences"
    );

    // Pin down the actual rendered format so a regression in
    // format_status_text is caught, not just disagreement between
    // build_status_data and render_status.
    assert!(via_struct.contains("Mynd v"));
    assert!(via_struct.contains(" — test-proj")); // em dash preserved from the original literal output
    assert!(via_struct.contains("Server:     running at http://127.0.0.1:3456 (mynd up)"));
    assert!(via_struct.contains("Sync:       disabled (local only)"));
    assert!(via_struct.contains("AI clients: claude"));
    assert!(via_struct.contains("Project:    test-proj"));
    assert!(via_struct.contains("golang preferences"));
    // "Remaining" line: verify the saturating_sub substitution in format_status_text
    // matches the real budget arithmetic (used_tokens, max_tokens from the loaded config),
    // not just that build_status_data and render_status agree with each other.
    let project = data.project.as_ref().unwrap();
    let expected_remaining = project.max_tokens.saturating_sub(project.used_tokens);
    assert!(via_struct.contains(&format!("Remaining:  ~{expected_remaining} tokens")));
}

// ── detect_registered_clients ────────────────────────────────────────────

#[test]
fn detect_registered_clients_empty_when_no_configs() {
    let home = tempfile::tempdir().unwrap();
    let result = detect_registered_clients(home.path());
    assert!(result.is_empty());
}

#[test]
fn detect_registered_clients_claude_via_mcp_json() {
    let home = tempfile::tempdir().unwrap();
    let claude_dir = home.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(
        claude_dir.join("mcp.json"),
        r#"{"mcpServers":{"hivemind":{"command":"hivemind"}}}"#,
    )
    .unwrap();
    let result = detect_registered_clients(home.path());
    assert!(result.contains(&"claude"));
}

#[test]
fn detect_registered_clients_claude_via_settings_json() {
    let home = tempfile::tempdir().unwrap();
    let claude_dir = home.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(
        claude_dir.join("settings.json"),
        r#"{"mcpServers":{"hivemind":{"command":"hivemind"}}}"#,
    )
    .unwrap();
    let result = detect_registered_clients(home.path());
    assert!(result.contains(&"claude"));
}

#[test]
fn detect_registered_clients_claude_via_user_scope_claude_json() {
    let home = tempfile::tempdir().unwrap();
    fs::write(
        home.path().join(".claude.json"),
        r#"{"mcpServers":{"hivemind":{"command":"/x/hivemind"}}}"#,
    )
    .unwrap();
    let result = detect_registered_clients(home.path());
    assert!(result.contains(&"claude"));
}

#[test]
fn detect_registered_clients_ignores_claude_files_without_hivemind() {
    let home = tempfile::tempdir().unwrap();
    let claude_dir = home.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(claude_dir.join("mcp.json"), r#"{"mcpServers":{}}"#).unwrap();
    let result = detect_registered_clients(home.path());
    assert!(!result.contains(&"claude"));
}

#[test]
fn detect_registered_clients_cursor() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".cursor");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("mcp.json"),
        r#"{"mcpServers":{"hivemind":{"command":"hivemind"}}}"#,
    )
    .unwrap();
    let result = detect_registered_clients(home.path());
    assert!(result.contains(&"cursor"));
}

#[test]
fn detect_registered_clients_kimi() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".kimi");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("mcp.json"),
        r#"{"mcpServers":{"hivemind":{"command":"hivemind"}}}"#,
    )
    .unwrap();
    let result = detect_registered_clients(home.path());
    assert!(result.contains(&"kimi"));
}

#[test]
fn detect_registered_clients_windsurf() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".codeium").join("windsurf");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("mcp_config.json"),
        r#"{"mcpServers":{"hivemind":{"command":"hivemind"}}}"#,
    )
    .unwrap();
    let result = detect_registered_clients(home.path());
    assert!(result.contains(&"windsurf"));
}

#[test]
fn detect_registered_clients_codex() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".codex");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("config.toml"),
        "\n[mcp_servers.hivemind]\ncommand = \"hivemind\"\nargs = []\n",
    )
    .unwrap();
    let result = detect_registered_clients(home.path());
    assert!(result.contains(&"codex"));
}

#[test]
fn detect_registered_clients_opencode_via_config_home() {
    // detect_registered_clients reads XDG_CONFIG_HOME; hold the mutex so
    // other tests that set that env var don't interfere.
    let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".config").join("opencode");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("opencode.json"),
        r#"{"mcp":{"hivemind":{"type":"local","command":"hivemind"}}}"#,
    )
    .unwrap();
    let result = detect_registered_clients(home.path());
    assert!(result.contains(&"opencode"));
}

#[test]
fn detect_registered_clients_multiple() {
    let home = tempfile::tempdir().unwrap();

    let claude_dir = home.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(
        claude_dir.join("mcp.json"),
        r#"{"mcpServers":{"hivemind":{}}}"#,
    )
    .unwrap();

    let cursor_dir = home.path().join(".cursor");
    fs::create_dir_all(&cursor_dir).unwrap();
    fs::write(
        cursor_dir.join("mcp.json"),
        r#"{"mcpServers":{"hivemind":{}}}"#,
    )
    .unwrap();

    let result = detect_registered_clients(home.path());
    assert!(result.contains(&"claude"));
    assert!(result.contains(&"cursor"));
    assert_eq!(result.len(), 2);
}

#[test]
fn append_block_if_absent_no_trailing_newline_in_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("CLAUDE.md");
    fs::write(&path, "# Existing content").unwrap();
    let (_, status) = append_block_if_absent(&path, "# HiveMind", "# HiveMind\nblock\n").unwrap();
    assert_eq!(status, "created");
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("Existing content"));
    assert!(content.contains("# HiveMind"));
}

// ── upsert_json_mcp ─────────────────────────────────────────────────────

#[test]
fn upsert_json_mcp_creates_new_file_with_mcp_servers_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mcp.json");
    upsert_json_mcp(
        &path,
        "mynd",
        serde_json::json!({"command": "mynd"}),
    )
    .unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(val["mcpServers"]["mynd"]["command"] == "mynd");
}

#[test]
fn upsert_json_mcp_uses_mcp_key_when_entry_has_type_field() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opencode.json");
    upsert_json_mcp(
        &path,
        "mynd",
        serde_json::json!({"type": "local", "command": "mynd", "args": []}),
    )
    .unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(val["mcp"]["mynd"]["type"] == "local");
    assert!(
        val.get("mcpServers").is_none(),
        "should use 'mcp' not 'mcpServers'"
    );
}

#[test]
fn upsert_json_mcp_updates_existing_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mcp.json");
    fs::write(&path, r#"{"mcpServers":{"other":{"command":"other"}}}"#).unwrap();
    upsert_json_mcp(
        &path,
        "mynd",
        serde_json::json!({"command": "mynd"}),
    )
    .unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(
        val["mcpServers"]["other"]["command"] == "other",
        "must preserve existing"
    );
    assert!(val["mcpServers"]["mynd"]["command"] == "mynd");
}

#[test]
fn upsert_json_mcp_creates_parent_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("deep").join("mcp.json");
    upsert_json_mcp(
        &path,
        "mynd",
        serde_json::json!({"command": "mynd"}),
    )
    .unwrap();
    assert!(path.exists());
}

#[test]
fn upsert_json_mcp_detects_mcp_key_from_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opencode.json");
    fs::write(&path, r#"{"mcp":{"existing":{"type":"local"}}}"#).unwrap();
    upsert_json_mcp(
        &path,
        "mynd",
        serde_json::json!({"command": "mynd"}),
    )
    .unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    let val: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(val["mcp"]["mynd"]["command"] == "mynd");
    assert!(val.get("mcpServers").is_none());
}

#[test]
fn upsert_json_mcp_drops_a_stale_hivemind_entry_when_writing_mynd() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mcp.json");
    fs::write(
        &path,
        r#"{"mcpServers":{"hivemind":{"command":"hivemind"},"other":{"command":"o"}}}"#,
    )
    .unwrap();

    upsert_json_mcp(&path, "mynd", serde_json::json!({"command": "mynd"})).unwrap();

    let val: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(val["mcpServers"]["mynd"]["command"] == "mynd");
    assert!(val["mcpServers"]["other"]["command"] == "o", "unrelated entry kept");
    assert!(val["mcpServers"].get("hivemind").is_none(), "stale entry dropped");
}

#[test]
fn strip_toml_table_removes_only_the_named_table() {
    let doc = "[a]\nx = 1\n\n[mcp_servers.hivemind]\ncommand = \"hivemind\"\nargs = []\n\n[b]\ny = 2\n";
    let out = strip_toml_table(doc, "[mcp_servers.hivemind]");
    assert!(!out.contains("mcp_servers.hivemind"));
    assert!(!out.contains("command = \"hivemind\""));
    assert!(out.contains("[a]\nx = 1"));
    assert!(out.contains("[b]\ny = 2"));
}

#[test]
fn already_registered_matches_either_server_key() {
    assert!(already_registered(r#"{"mcpServers":{"mynd":{}}}"#));
    assert!(already_registered(r#"{"mcpServers":{"hivemind":{}}}"#));
    assert!(!already_registered(r#"{"mcpServers":{"other":{}}}"#));
}

#[test]
fn home_dir_returns_a_path() {
    let h = home_dir();
    assert!(!h.as_os_str().is_empty());
}

#[test]
fn exe_path_returns_non_empty_string() {
    let p = exe_path();
    assert!(!p.is_empty());
}

// ── render_status extra paths ────────────────────────────────────────────

#[tokio::test]
async fn render_status_shows_nothing_when_no_recalls_resolve() {
    use crate::{config::SyncSettings, db, store::SqliteStore};

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, db_path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    let store = SqliteStore::new(conn);

    let proj = tempfile::tempdir().unwrap();
    std::fs::write(
        proj.path().join(".mynd.toml"),
        "[project]\nname=\"empty\"\n[hooks.on_session_start]\nmax_tokens=2000\nrecalls=[\"nonexistent memory\"]\n",
    ).unwrap();
    let missing_global = proj.path().join("no-global.toml");

    let out = render_status(
        proj.path(),
        &missing_global,
        &store,
        "/tmp/x.db",
        &[],
        &default_settings(),
        false,
    )
    .await
    .unwrap();
    assert!(
        out.contains("nothing"),
        "should show nothing when no recalls resolve"
    );
    assert!(
        out.contains("skipped") || out.contains("[skipped]"),
        "should show skipped entry"
    );
}

#[tokio::test]
async fn render_status_shows_local_toml_indicator() {
    use crate::{config::SyncSettings, db, store::SqliteStore};

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, db_path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    let store = SqliteStore::new(conn);

    let proj = tempfile::tempdir().unwrap();
    std::fs::write(
        proj.path().join(".mynd.toml"),
        "[project]\nname=\"local-test\"\n",
    )
    .unwrap();
    std::fs::write(proj.path().join(".mynd.local.toml"), "").unwrap();
    let missing_global = proj.path().join("no-global.toml");

    let out = render_status(
        proj.path(),
        &missing_global,
        &store,
        "/tmp/x.db",
        &[],
        &default_settings(),
        false,
    )
    .await
    .unwrap();
    assert!(
        out.contains(".mynd.local.toml"),
        "should mention local toml"
    );
}

// ── do_migrate_copy ──────────────────────────────────────────────────────

#[test]
fn do_migrate_copy_copies_file_and_creates_dirs() {
    let src_dir = tempfile::tempdir().unwrap();
    let dst_dir = tempfile::tempdir().unwrap();

    let legacy = src_dir.path().join("memories.db");
    fs::write(&legacy, b"sqlite data").unwrap();

    let new_path = dst_dir.path().join("sub").join("memories.db");
    do_migrate_copy(&legacy, &new_path).unwrap();

    assert!(new_path.exists());
    assert_eq!(fs::read(&new_path).unwrap(), b"sqlite data");
}

#[test]
fn do_migrate_copy_fails_when_source_missing() {
    let dst_dir = tempfile::tempdir().unwrap();
    let legacy = dst_dir.path().join("nonexistent.db");
    let new_path = dst_dir.path().join("new.db");
    assert!(do_migrate_copy(&legacy, &new_path).is_err());
}

// ── cmd_migrate_inner ────────────────────────────────────────────────────

#[test]
fn cmd_migrate_inner_nothing_to_migrate_when_legacy_missing() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join("memories.db"); // does not exist
    let new_path = dir.path().join("new").join("memories.db");
    let result = cmd_migrate_inner(&legacy, &new_path, &mut std::io::Cursor::new(b""));
    assert!(result.is_ok());
    assert!(!new_path.exists(), "new path should not be created");
}

#[test]
fn cmd_migrate_inner_new_already_exists() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join("legacy.db");
    fs::write(&legacy, b"data").unwrap();
    let new_path = dir.path().join("new.db");
    fs::write(&new_path, b"existing").unwrap();
    let result = cmd_migrate_inner(&legacy, &new_path, &mut std::io::Cursor::new(b""));
    assert!(result.is_ok());
    assert_eq!(
        fs::read(&new_path).unwrap(),
        b"existing",
        "should not overwrite"
    );
}

#[test]
fn cmd_migrate_inner_cancelled_on_n_input() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join("legacy.db");
    fs::write(&legacy, b"data").unwrap();
    let new_path = dir.path().join("new.db");
    let result = cmd_migrate_inner(&legacy, &new_path, &mut std::io::Cursor::new(b"N\n"));
    assert!(result.is_ok());
    assert!(!new_path.exists(), "should not copy when cancelled");
}

#[test]
fn cmd_migrate_inner_proceeds_on_y_input() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join("legacy.db");
    fs::write(&legacy, b"sqlite data").unwrap();
    let new_path = dir.path().join("subdir").join("memories.db");
    let result = cmd_migrate_inner(&legacy, &new_path, &mut std::io::Cursor::new(b"y\n"));
    assert!(result.is_ok());
    assert_eq!(fs::read(&new_path).unwrap(), b"sqlite data");
}

// ── ensure_claude_settings_hook ──────────────────────────────────────────

#[test]
fn scaffold_writes_claude_session_start_hook() {
    let proj = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    scaffold(proj.path(), home.path(), cfg.path()).unwrap();
    let settings = fs::read_to_string(proj.path().join(".claude").join("settings.json")).unwrap();
    assert!(settings.contains("SessionStart"));
    assert!(settings.contains("mynd session-start"));
    // idempotent
    scaffold(proj.path(), home.path(), cfg.path()).unwrap();
    let again = fs::read_to_string(proj.path().join(".claude").join("settings.json")).unwrap();
    assert_eq!(again.matches("mynd session-start").count(), 1);
}

#[test]
fn hook_merge_preserves_existing_settings() {
    let proj = tempfile::tempdir().unwrap();
    let dir = proj.path().join(".claude");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("settings.json"),
        r#"{"permissions":{"allow":["Bash(ls:*)"]}}"#,
    )
    .unwrap();
    ensure_claude_settings_hook(proj.path()).unwrap();
    let merged = fs::read_to_string(dir.join("settings.json")).unwrap();
    assert!(merged.contains("Bash(ls:*)"), "existing keys preserved");
    assert!(merged.contains("mynd session-start"));
}

#[test]
fn hook_merge_refuses_to_overwrite_malformed_settings() {
    let proj = tempfile::tempdir().unwrap();
    let dir = proj.path().join(".claude");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("settings.json"), "{oops").unwrap();
    let result = ensure_claude_settings_hook(proj.path());
    assert!(result.is_err(), "malformed JSON must be an error");
    assert_eq!(
        fs::read_to_string(dir.join("settings.json")).unwrap(),
        "{oops",
        "malformed file must be left untouched"
    );
}

// ── ensure_global_config ─────────────────────────────────────────────────

#[test]
fn ensure_global_config_creates_file_when_missing() {
    let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
    let cfg_dir = tempfile::tempdir().unwrap();
    // SAFETY: test-only env mutation; serialised by ENV_MUTEX.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", cfg_dir.path()) };
    ensure_global_config();
    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    assert!(cfg_dir.path().join("mynd").join("config.toml").exists());
}

#[test]
fn ensure_global_config_is_idempotent() {
    let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
    let cfg_dir = tempfile::tempdir().unwrap();
    let config_file = cfg_dir.path().join("mynd").join("config.toml");
    fs::create_dir_all(config_file.parent().unwrap()).unwrap();
    fs::write(&config_file, "original").unwrap();
    unsafe { std::env::set_var("XDG_CONFIG_HOME", cfg_dir.path()) };
    ensure_global_config();
    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    assert_eq!(fs::read_to_string(&config_file).unwrap(), "original");
}

// ── warn_if_not_initialized ──────────────────────────────────────────────

#[test]
fn warn_if_not_initialized_no_config_prints_hint() {
    let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
    let cfg_dir = tempfile::tempdir().unwrap();
    // SAFETY: test-only env mutation; serialised by ENV_MUTEX.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", cfg_dir.path()) };
    warn_if_not_initialized(); // exercises the "no config" branch
    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
}

#[test]
fn warn_if_not_initialized_config_but_no_clients_prints_hint() {
    let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
    let cfg_dir = tempfile::tempdir().unwrap();
    let home_dir_tmp = tempfile::tempdir().unwrap();
    let config_file = cfg_dir.path().join("mynd").join("config.toml");
    fs::create_dir_all(config_file.parent().unwrap()).unwrap();
    fs::write(&config_file, "[server]\n").unwrap();
    // SAFETY: test-only env mutation; serialised by ENV_MUTEX.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", cfg_dir.path()) };
    unsafe { std::env::set_var("HOME", home_dir_tmp.path()) };
    warn_if_not_initialized(); // exercises the "config found, no clients" branch
    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    unsafe { std::env::remove_var("HOME") };
}
