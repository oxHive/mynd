use anyhow::Result;
use clap::Parser;
use oxmynd::cli::{self, Cli, Command, McpAction, ServiceAction};
use oxmynd::{config, db, http, server, store, sync};
use rmcp::ServiceExt;
use server::Mynd;
use std::sync::Arc;
use store::SqliteStore;
use tokio::sync::Notify;

fn main() -> Result<()> {
    let cli = Cli::parse();
    oxmynd::dir_migrate::run_startup_migration();
    match cli.command {
        None => run_server(),
        Some(Command::Init) => cli::cmd_init(),
        Some(Command::Status { plain }) => cli::cmd_status(plain),
        Some(Command::Up { headless, plain }) => run_up(headless, plain),
        Some(Command::Dashboard { open }) => run_dashboard(open),
        Some(Command::Mcp { action }) => match action {
            McpAction::Install { client } => cli::cmd_mcp_install(&client),
        },
        Some(Command::Service { action }) => match action {
            ServiceAction::Install {
                dashboard,
                matrix,
                discord,
            } => cli::cmd_service_install(dashboard, matrix, discord),
            ServiceAction::Uninstall => cli::cmd_service_uninstall(),
            ServiceAction::Status => cli::cmd_service_status(),
        },
        Some(Command::Matrix { action }) => match action {
            cli::MatrixAction::Login => cli::cmd_matrix_login(),
            cli::MatrixAction::Run { debug } => run_matrix(debug),
            cli::MatrixAction::Status => cli::cmd_matrix_status(),
            cli::MatrixAction::Send { user_id, message } => run_matrix_send(user_id, message),
        },
        Some(Command::Discord { action }) => match action {
            cli::DiscordAction::Login => cli::cmd_discord_login(),
            cli::DiscordAction::Run { debug } => run_discord(debug),
            cli::DiscordAction::Status => cli::cmd_discord_status(),
            cli::DiscordAction::Send { user_id, message } => run_discord_send(user_id, message),
        },
        Some(Command::Migrate) => cli::cmd_migrate(),
        Some(Command::SessionStart { json }) => cli::cmd_session_start(json),
    }
}

struct LocalTimer;

impl tracing_subscriber::fmt::time::FormatTime for LocalTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(
            w,
            "{}",
            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.6f%:z")
        )
    }
}

fn init_tracing() {
    init_tracing_with_default("mynd=info,oxmynd=info");
}

fn init_tracing_with_default(default_filter: &str) {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_timer(LocalTimer)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| default_filter.into()),
        )
        .init();
}

async fn open_store(
    sync_settings: &config::SyncSettings,
    db_path: &str,
) -> Result<(Arc<SqliteStore>, libsql::Database)> {
    tracing::info!("opening database at {db_path}");
    let database = db::open_database(sync_settings, db_path).await?;
    let conn = database.connect()?;
    db::run_migrations(&conn).await?;
    let store = Arc::new(SqliteStore::new(conn));
    Ok((store, database))
}

#[tokio::main]
async fn run_server() -> Result<()> {
    init_tracing();
    let settings =
        config::load_server_settings(&config::global_config_path()).unwrap_or_else(|e| {
            tracing::warn!("could not load global config ({e:#}); using defaults");
            config::ServerSettings {
                host: "127.0.0.1".into(),
                port: 3456,
                dashboard_port: 3457,
                api_url: "http://127.0.0.1:3456".into(),
                cors_origin: "http://127.0.0.1:3457".into(),
                sync: config::SyncSettings::default(),
                org_sync: None,
                update: config::UpdateSettings::default(),
                agent: config::AgentSettings::default(),
                guard_predefined_namespaces: true,
            }
        });
    let (store, database) = open_store(&settings.sync, &db::resolve_db_path()).await?;
    // Holds the DB handle so it lives past `server.waiting()` when no sync loop owns it.
    let mut _db_guard: Option<libsql::Database> = None;
    let mut service = if settings.sync.enabled {
        let trigger = Arc::new(Notify::new());
        tokio::spawn(sync::run_sync_loop(
            Arc::new(database),
            store.clone(),
            settings.sync.interval_seconds,
            settings.sync.sync_on_startup,
            trigger.clone(),
        ));
        if settings.sync.sync_on_store {
            Mynd::with_sync(store, trigger)
        } else {
            Mynd::with_store(store)
        }
    } else {
        _db_guard = Some(database);
        Mynd::with_store(store)
    };

    // Org layer is entirely optional — absence of [org_sync] must never stop
    // the server from starting on personal/workspace alone. Unlike the
    // primary store, org's database handle never needs a bare guard variable:
    // `ServerSettings.org_sync` is only ever `Some` when already enabled
    // (Task 2), so this branch always spawns a sync loop that owns the handle.
    if let Some(org_sync) = &settings.org_sync {
        match open_store(org_sync, &db::resolve_org_db_path()).await {
            Ok((org_store, org_database)) => {
                let org_trigger = Arc::new(Notify::new());
                tokio::spawn(sync::run_sync_loop(
                    Arc::new(org_database),
                    org_store.clone(),
                    org_sync.interval_seconds,
                    org_sync.sync_on_startup,
                    org_trigger.clone(),
                ));
                service = if org_sync.sync_on_store {
                    service
                        .with_org_store(org_store)
                        .with_org_sync_trigger(org_trigger)
                } else {
                    service.with_org_store(org_store)
                };
            }
            Err(e) => {
                tracing::warn!(
                    "could not open org database ({e:#}); org layer unavailable this session"
                );
            }
        }
    }

    tracing::info!("Mynd MCP server starting on stdio");
    let server = service
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await?;
    server.waiting().await?;
    Ok(())
}

#[tokio::main]
async fn run_up(headless: bool, plain: bool) -> Result<()> {
    cli::warn_if_not_initialized();
    init_tracing();
    let settings = config::load_server_settings(&config::global_config_path())?;
    let (store, database) = open_store(&settings.sync, &db::resolve_db_path()).await?;

    // Holds the DB handle so it lives past `http::run_up` when no sync loop owns it.
    let mut _db_guard: Option<libsql::Database> = None;
    let mut notify_on_store = None;
    if settings.sync.enabled {
        let trigger = Arc::new(Notify::new());
        tokio::spawn(sync::run_sync_loop(
            Arc::new(database),
            store.clone(),
            settings.sync.interval_seconds,
            settings.sync.sync_on_startup,
            trigger.clone(),
        ));
        if settings.sync.sync_on_store {
            notify_on_store = Some(trigger);
        }
    } else {
        _db_guard = Some(database);
    }

    // Org layer is entirely optional here too — mirrors run_server's wiring.
    // Unlike the primary store, org's database handle never needs a bare
    // guard variable: ServerSettings.org_sync is only ever Some when already
    // enabled, so this branch always spawns a sync loop that owns the handle.
    let mut org_store = None;
    if let Some(org_sync) = &settings.org_sync {
        match open_store(org_sync, &db::resolve_org_db_path()).await {
            Ok((org_store_handle, org_database)) => {
                let org_trigger = Arc::new(Notify::new());
                tokio::spawn(sync::run_sync_loop(
                    Arc::new(org_database),
                    org_store_handle.clone(),
                    org_sync.interval_seconds,
                    org_sync.sync_on_startup,
                    org_trigger,
                ));
                org_store = Some(org_store_handle);
            }
            Err(e) => {
                tracing::warn!(
                    "could not open org database ({e:#}); org layer unavailable this session"
                );
            }
        }
    }

    http::run_up(
        store,
        org_store,
        &settings,
        headless,
        plain,
        notify_on_store,
    )
    .await
}

#[tokio::main]
async fn run_dashboard(open: bool) -> Result<()> {
    init_tracing();
    let settings = config::load_server_settings(&config::global_config_path())?;
    http::run_dashboard(&settings, open).await
}

#[tokio::main]
async fn run_matrix(debug: bool) -> Result<()> {
    if debug {
        init_tracing_with_default("mynd=debug,oxmynd=debug");
    } else {
        init_tracing();
    }
    tracing::debug!("loading matrix config");
    let settings =
        config::load_matrix_settings(&config::global_config_path())?.ok_or_else(|| {
            anyhow::anyhow!("no [matrix] config found — run `mynd matrix login` first")
        })?;
    let server_settings = config::load_server_settings(&config::global_config_path())?;
    let mynd_bin = std::env::current_exe()?.to_string_lossy().into_owned();
    tracing::debug!("starting matrix daemon");
    oxmynd::matrix::daemon::run(settings, server_settings.agent, mynd_bin).await
}

#[tokio::main]
async fn run_matrix_send(user_id: String, message: String) -> Result<()> {
    init_tracing_with_default("mynd=debug,oxmynd=debug");
    let settings =
        config::load_matrix_settings(&config::global_config_path())?.ok_or_else(|| {
            anyhow::anyhow!("no [matrix] config found — run `mynd matrix login` first")
        })?;
    oxmynd::matrix::daemon::send_direct_message(&settings, &user_id, &message).await
}

#[tokio::main]
async fn run_discord(debug: bool) -> Result<()> {
    if debug {
        init_tracing_with_default("mynd=debug,oxmynd=debug");
    } else {
        init_tracing();
    }
    tracing::debug!("loading discord config");
    let settings =
        config::load_discord_settings(&config::global_config_path())?.ok_or_else(|| {
            anyhow::anyhow!("no [discord] config found — run `mynd discord login` first")
        })?;
    let server_settings = config::load_server_settings(&config::global_config_path())?;
    let mynd_bin = std::env::current_exe()?.to_string_lossy().into_owned();
    tracing::debug!("starting discord daemon");
    oxmynd::discord::daemon::run(settings, server_settings.agent, mynd_bin).await
}

#[tokio::main]
async fn run_discord_send(user_id: String, message: String) -> Result<()> {
    init_tracing_with_default("mynd=debug,oxmynd=debug");
    let settings =
        config::load_discord_settings(&config::global_config_path())?.ok_or_else(|| {
            anyhow::anyhow!("no [discord] config found — run `mynd discord login` first")
        })?;
    oxmynd::discord::daemon::send_direct_message(&settings, &user_id, &message).await
}
