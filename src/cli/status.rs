use anyhow::Result;
use std::io::Write as _;
use std::path::Path;

use super::init::{
    GLOBAL_CONFIG, detect_registered_clients, home_dir, with_spinner, write_if_absent,
};

/// Create the global config file with defaults on first run if it doesn't exist yet.
/// Prints a note to stderr so the user knows where it landed.
pub fn ensure_global_config() {
    let config_path = crate::config::global_config_path();
    if config_path.exists() {
        return;
    }
    match write_if_absent(&config_path, GLOBAL_CONFIG) {
        Ok(_) => {
            eprintln!("note: created default config at {}", config_path.display());
        }
        Err(e) => {
            eprintln!("warning: could not create global config: {e}");
        }
    }
}

/// Print a one-time hint when `mynd init` has never been run.
/// Called from commands that work without init but benefit from it.
pub fn warn_if_not_initialized() {
    let config_path = crate::config::global_config_path();
    let home = home_dir();

    if !config_path.exists() {
        eprintln!("hint: looks like you haven't run `mynd init` yet.");
        eprintln!("      Run it in your project directory to create config files and");
        eprintln!("      register Mynd with your AI coding client.");
        eprintln!();
        return;
    }

    // init was run but no AI client has been registered yet
    if detect_registered_clients(&home).is_empty() {
        eprintln!("hint: no AI client is registered with Mynd yet.");
        eprintln!("      The server will start, but your AI client won't connect to it.");
        eprintln!("      Register once with:  mynd mcp install claude");
        eprintln!("      (or cursor, windsurf, opencode, kimi, codex)");
        eprintln!();
    }
}

/// TCP-probes whether a Mynd server is currently listening on
/// `settings`'s host:port. 0.0.0.0/:: are redirected to 127.0.0.1 since you
/// can't dial a wildcard bind address directly.
pub fn probe_server_up(settings: &crate::config::ServerSettings) -> bool {
    let probe_host = match settings.host.as_str() {
        "0.0.0.0" | "::" => "127.0.0.1",
        h => h,
    };
    format!("{probe_host}:{}", settings.port)
        .parse::<std::net::SocketAddr>()
        .ok()
        .map(|addr| {
            std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300))
                .is_ok()
        })
        .unwrap_or(false)
}

pub fn cmd_status(plain: bool) -> Result<()> {
    let home = home_dir();
    let cwd = std::env::current_dir()?;
    let db_path = crate::db::resolve_db_path();

    if crate::tui::is_interactive(plain) {
        return tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(async {
                let clients = detect_registered_clients(&home);
                let settings =
                    crate::config::load_server_settings(&crate::config::global_config_path())?;
                let server_up = probe_server_up(&settings);
                let sync = crate::config::SyncSettings::default();
                let database = crate::db::open_database(&sync, &db_path).await?;
                let conn = database.connect()?;
                crate::db::run_migrations(&conn).await?;
                let store = crate::store::SqliteStore::new(conn);
                crate::tui::status_view::run(
                    &cwd,
                    &crate::config::global_config_path(),
                    &store,
                    &db_path,
                    &clients,
                    &settings,
                    server_up,
                )
                .await
            });
    }

    let (out, clients) = with_spinner("checking status...", || {
        let clients = detect_registered_clients(&home);
        let settings = crate::config::load_server_settings(&crate::config::global_config_path())?;
        let server_up = probe_server_up(&settings);
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(async {
                let sync = crate::config::SyncSettings::default();
                let database = crate::db::open_database(&sync, &db_path).await?;
                let conn = database.connect()?;
                crate::db::run_migrations(&conn).await?;
                let store = crate::store::SqliteStore::new(conn);
                render_status(
                    &cwd,
                    &crate::config::global_config_path(),
                    &store,
                    &db_path,
                    &clients,
                    &settings,
                    server_up,
                )
                .await
            })?;
        Ok::<_, anyhow::Error>((result, clients))
    })?;
    println!("{out}");
    if clients.is_empty() {
        eprintln!("hint: no AI client is registered with Mynd yet.");
        eprintln!("      Register once with:  mynd mcp install claude");
        eprintln!("      (or cursor, windsurf, opencode, kimi, codex)");
    }
    Ok(())
}

pub fn cmd_session_start(json: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    if crate::config::discover_project_root(&cwd).is_none() {
        return Ok(()); // no project config: stay silent so hooks can run unconditionally
    }
    let db_path = crate::db::resolve_db_path();
    let out = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let (sync_settings, org_sync_settings) =
                match crate::config::load_server_settings(&crate::config::global_config_path()) {
                    Ok(s) => (s.sync, s.org_sync),
                    Err(_) => (crate::config::SyncSettings::default(), None),
                };
            let database = crate::db::open_database(&sync_settings, &db_path).await?;
            let conn = database.connect()?;
            crate::db::run_migrations(&conn).await?;
            let store = crate::store::SqliteStore::new(conn);

            let org_store = match &org_sync_settings {
                Some(org_sync) => {
                    let org_db_path = crate::db::resolve_org_db_path();
                    match crate::db::open_database(org_sync, &org_db_path).await {
                        Ok(org_database) => match org_database.connect() {
                            Ok(org_conn) => match crate::db::run_migrations(&org_conn).await {
                                Ok(()) => Some(crate::store::SqliteStore::new(org_conn)),
                                Err(e) => {
                                    tracing::warn!("org db migration failed: {e:#}");
                                    None
                                }
                            },
                            Err(e) => {
                                tracing::warn!("org db connect failed: {e:#}");
                                None
                            }
                        },
                        Err(e) => {
                            tracing::warn!("could not open org database: {e:#}");
                            None
                        }
                    }
                }
                None => None,
            };

            let config = crate::config::load_config(&cwd)?;
            let result =
                crate::session::execute_session_start(&config, &store, org_store.as_ref()).await?;
            if let Err(e) = store
                .log_session_start(&cwd.to_string_lossy(), &result)
                .await
            {
                tracing::warn!("failed to write session_start_log entry: {e:#}");
            }
            Ok::<_, anyhow::Error>(render_session_start(&result, json))
        })?;
    if !out.is_empty() {
        println!("{out}");
    }
    Ok(())
}

pub(crate) fn render_session_start(
    result: &crate::session::SessionStartResult,
    json: bool,
) -> String {
    if json {
        return serde_json::to_string_pretty(&result.to_json()).unwrap_or_default();
    }
    if result.loaded.is_empty() && result.skipped.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "<mynd-context project=\"{}\" tokens=\"{}/{}\">\n",
        result.project, result.used_tokens, result.max_tokens
    );
    for l in &result.loaded {
        out.push_str(&format!("\n## {}\n{}\n", l.entry.title, l.entry.content));
    }
    out.push_str("</mynd-context>\n");
    for s in &result.skipped {
        out.push_str(&format!(
            "mynd: skipped recall \"{}\" ({})\n",
            s.query,
            s.reason.as_str()
        ));
    }
    out
}

pub(crate) fn do_migrate_copy(legacy: &Path, new_path: &Path) -> Result<()> {
    if let Some(dir) = new_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::copy(legacy, new_path)?;
    Ok(())
}

pub fn cmd_migrate() -> Result<()> {
    let legacy = crate::db::legacy_db_path();
    let new_path = crate::db::xdg_data_dir().join("memories.db");
    let stdin = std::io::stdin();
    cmd_migrate_inner(&legacy, &new_path, &mut stdin.lock())
}

pub(crate) fn cmd_migrate_inner(
    legacy: &Path,
    new_path: &Path,
    stdin: &mut dyn std::io::BufRead,
) -> Result<()> {
    if !legacy.exists() {
        println!(
            "Nothing to migrate: legacy database not found at {}",
            legacy.display()
        );
        println!("New location: {}", new_path.display());
        return Ok(());
    }

    if new_path.exists() {
        println!("New database already exists at {}.", new_path.display());
        println!("Remove it first if you want to replace it with the legacy database.");
        return Ok(());
    }

    println!("Migrating database:");
    println!("  from: {}", legacy.display());
    println!("    to: {}", new_path.display());
    print!("Proceed? [y/N] ");
    std::io::stdout().flush()?;

    let mut input = String::new();
    stdin.read_line(&mut input)?;
    if !input.trim().eq_ignore_ascii_case("y") {
        println!("Cancelled.");
        return Ok(());
    }

    do_migrate_copy(legacy, new_path)?;
    println!(
        "Done. You can now delete the old directory: rm -rf {}",
        legacy.parent().unwrap().display()
    );
    Ok(())
}

/// Build the `mynd status` report. `global_path` is injectable for testing.
pub struct LoadedEntrySummary {
    pub title: String,
    pub tokens: usize,
    pub is_local: bool,
}

pub struct SkippedEntrySummary {
    pub query: String,
    pub reason: &'static str,
}

pub struct ProjectStatus {
    pub project_name: String,
    pub has_local_config: bool,
    pub file_open_rule_count: usize,
    pub mention_trigger_count: usize,
    pub loaded: Vec<LoadedEntrySummary>,
    pub skipped: Vec<SkippedEntrySummary>,
    pub used_tokens: usize,
    pub max_tokens: usize,
    pub truncated: bool,
}

pub struct StatusData {
    pub version: &'static str,
    pub project_label: Option<String>,
    pub server_up: bool,
    pub server_host: String,
    pub server_port: u16,
    pub db_path: String,
    pub memory_count: i64,
    pub sync_enabled: bool,
    pub sync_remote_url: String,
    pub registered_clients: Vec<String>,
    pub project: Option<ProjectStatus>,
    pub matrix: MatrixStatusLine,
    pub discord: DiscordStatusLine,
}

pub enum MatrixStatusLine {
    /// No `[matrix]` section in the global config — matrix isn't set up.
    NotConfigured,
    /// Configured, but `mynd matrix run` isn't currently up.
    NotRunning,
    Running {
        user_id: String,
        sync_state: String,
        room_count: usize,
        active_sessions: usize,
    },
}

pub enum DiscordStatusLine {
    /// No `[discord]` section in the global config — discord isn't set up.
    NotConfigured,
    /// Configured, but `hivemind discord run` isn't currently up.
    NotRunning,
    Running {
        application_id: String,
        sync_state: String,
        channel_count: usize,
        active_sessions: usize,
    },
}

pub async fn build_status_data(
    cwd: &Path,
    global_path: &Path,
    store: &crate::store::SqliteStore,
    db_path: &str,
    registered_clients: &[&str],
    settings: &crate::config::ServerSettings,
    server_up: bool,
) -> Result<StatusData> {
    let version = env!("CARGO_PKG_VERSION");
    let memory_count = store.count().await?;

    let root = crate::config::discover_project_root(cwd);
    let config = match &root {
        Some(r) => Some(crate::config::load_config_with_global(r, global_path)?),
        None => None,
    };
    let project_label = config.as_ref().map(|c| c.project_name.clone());

    let probe_host = match settings.host.as_str() {
        "0.0.0.0" | "::" => "127.0.0.1",
        h => h,
    }
    .to_string();

    let matrix = match crate::config::load_matrix_settings(global_path)? {
        None => MatrixStatusLine::NotConfigured,
        Some(_) => {
            let socket_path = crate::matrix::status::socket_path();
            match crate::matrix::status::query_status(&socket_path).await {
                Ok(reply) => MatrixStatusLine::Running {
                    user_id: reply.user_id,
                    sync_state: reply.sync_state,
                    room_count: reply.rooms.len(),
                    active_sessions: reply.rooms.iter().filter(|r| r.active_session).count(),
                },
                Err(_) => MatrixStatusLine::NotRunning,
            }
        }
    };

    let discord = match crate::config::load_discord_settings(global_path)? {
        None => DiscordStatusLine::NotConfigured,
        Some(_) => {
            let socket_path = crate::discord::status::socket_path();
            match crate::discord::status::query_status(&socket_path).await {
                Ok(reply) => DiscordStatusLine::Running {
                    application_id: reply.application_id,
                    sync_state: reply.sync_state,
                    channel_count: reply.channels.len(),
                    active_sessions: reply.channels.iter().filter(|c| c.active_session).count(),
                },
                Err(_) => DiscordStatusLine::NotRunning,
            }
        }
    };

    let mut data = StatusData {
        version,
        project_label,
        server_up,
        server_host: probe_host,
        server_port: settings.port,
        db_path: db_path.to_string(),
        memory_count,
        sync_enabled: settings.sync.enabled,
        sync_remote_url: settings.sync.remote_url.clone(),
        registered_clients: registered_clients.iter().map(|s| s.to_string()).collect(),
        project: None,
        matrix,
        discord,
    };

    let (Some(root), Some(config)) = (root, config) else {
        return Ok(data);
    };

    let org_store = match &settings.org_sync {
        Some(org_sync) => {
            let org_db_path = crate::db::resolve_org_db_path();
            match crate::db::open_database(org_sync, &org_db_path).await {
                Ok(org_database) => match org_database.connect() {
                    Ok(org_conn) => match crate::db::run_migrations(&org_conn).await {
                        Ok(()) => Some(crate::store::SqliteStore::new(org_conn)),
                        Err(e) => {
                            tracing::warn!("org db migration failed: {e:#}");
                            None
                        }
                    },
                    Err(e) => {
                        tracing::warn!("org db connect failed: {e:#}");
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!("could not open org database: {e:#}");
                    None
                }
            }
        }
        None => None,
    };

    let result = crate::session::execute_session_start(&config, store, org_store.as_ref()).await?;

    data.project = Some(ProjectStatus {
        project_name: config.project_name.clone(),
        has_local_config: crate::config::PROJECT_LOCAL_CONFIG_NAMES
            .iter()
            .any(|n| root.join(n).is_file()),
        file_open_rule_count: config.file_open_rule_count,
        mention_trigger_count: config.mention_trigger_count,
        loaded: result
            .loaded
            .iter()
            .map(|l| LoadedEntrySummary {
                title: l.entry.title.clone(),
                tokens: l.tokens,
                is_local: matches!(l.source, crate::config::RecallSource::Local),
            })
            .collect(),
        skipped: result
            .skipped
            .iter()
            .map(|s| SkippedEntrySummary {
                query: s.query.clone(),
                reason: s.reason.as_str(),
            })
            .collect(),
        used_tokens: result.used_tokens,
        max_tokens: result.max_tokens,
        truncated: result.truncated(),
    });

    Ok(data)
}

pub fn format_status_text(data: &StatusData) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();

    match &data.project_label {
        Some(label) => writeln!(out, "Mynd v{} — {label}", data.version).unwrap(),
        None => writeln!(out, "Mynd v{}", data.version).unwrap(),
    }
    writeln!(out, "─────────────────────────────────────────────────────").unwrap();
    if data.server_up {
        writeln!(
            out,
            "Server:     running at http://{}:{} (mynd up)",
            data.server_host, data.server_port
        )
        .unwrap();
    } else {
        writeln!(
            out,
            "Server:     not running (stdio instance is spawned per session)"
        )
        .unwrap();
    }
    writeln!(
        out,
        "Storage:    {} ({} memories)",
        data.db_path, data.memory_count
    )
    .unwrap();
    if data.sync_enabled {
        writeln!(out, "Sync:       enabled \u{2192} {}", data.sync_remote_url).unwrap();
    } else {
        writeln!(out, "Sync:       disabled (local only)").unwrap();
    }
    if data.registered_clients.is_empty() {
        writeln!(out, "AI clients: none registered").unwrap();
    } else {
        writeln!(out, "AI clients: {}", data.registered_clients.join(", ")).unwrap();
    }
    match &data.matrix {
        MatrixStatusLine::NotConfigured => {}
        MatrixStatusLine::NotRunning => {
            writeln!(out, "Matrix:     configured, not running (mynd matrix run)").unwrap();
        }
        MatrixStatusLine::Running {
            user_id,
            sync_state,
            room_count,
            active_sessions,
        } => {
            writeln!(
                out,
                "Matrix:     {user_id} ({sync_state}), {room_count} room(s), \
                 {active_sessions} active session(s)"
            )
            .unwrap();
        }
    }
    match &data.discord {
        DiscordStatusLine::NotConfigured => {}
        DiscordStatusLine::NotRunning => {
            writeln!(
                out,
                "Discord:    configured, not running (hivemind discord run)"
            )
            .unwrap();
        }
        DiscordStatusLine::Running {
            application_id,
            sync_state,
            channel_count,
            active_sessions,
        } => {
            writeln!(
                out,
                "Discord:    {application_id} ({sync_state}), {channel_count} channel(s), \
                 {active_sessions} active session(s)"
            )
            .unwrap();
        }
    }
    writeln!(out).unwrap();

    let Some(project) = &data.project else {
        writeln!(out, "No .mynd.toml found in this directory tree.").unwrap();
        writeln!(
            out,
            "Run `mynd init` to set up memory hooks for this project."
        )
        .unwrap();
        return out;
    };

    writeln!(out, "Project:    {}", project.project_name).unwrap();
    writeln!(
        out,
        "Config:     .mynd.toml{}",
        if project.has_local_config {
            " + .mynd.local.toml"
        } else {
            ""
        }
    )
    .unwrap();
    writeln!(out).unwrap();
    writeln!(out, "On session start will inject:").unwrap();
    if project.loaded.is_empty() {
        writeln!(out, "  (nothing — no recalls configured or none resolved)").unwrap();
    }
    for entry in &project.loaded {
        let local = if entry.is_local { "  (local)" } else { "" };
        writeln!(
            out,
            "  {:<40} ~{} tokens{}",
            entry.title, entry.tokens, local
        )
        .unwrap();
    }
    for skip in &project.skipped {
        writeln!(out, "  [skipped]   {:<40} ({})", skip.query, skip.reason).unwrap();
    }
    writeln!(
        out,
        "  ──────────────────────────────────────────────────────────"
    )
    .unwrap();
    writeln!(out, "  Total:      ~{} tokens", project.used_tokens).unwrap();
    writeln!(out, "  Budget:     {} tokens", project.max_tokens).unwrap();
    let headroom = if project.truncated { "⚠" } else { "✓" };
    let remaining = project.max_tokens.saturating_sub(project.used_tokens);
    writeln!(out, "  Remaining:  ~{remaining} tokens  {headroom}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "On file open rules:    {} configured (reserved, not yet active)",
        project.file_open_rule_count
    )
    .unwrap();
    writeln!(
        out,
        "On mention triggers:   {} (reserved, not yet active)",
        project.mention_trigger_count
    )
    .unwrap();

    out
}

pub async fn render_status(
    cwd: &Path,
    global_path: &Path,
    store: &crate::store::SqliteStore,
    db_path: &str,
    registered_clients: &[&str],
    settings: &crate::config::ServerSettings,
    server_up: bool,
) -> Result<String> {
    let data = build_status_data(
        cwd,
        global_path,
        store,
        db_path,
        registered_clients,
        settings,
        server_up,
    )
    .await?;
    Ok(format_status_text(&data))
}
