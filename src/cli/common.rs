use anyhow::Result;

use crate::store::SqliteStore;

/// Opens the primary store using the real configured sync settings (same
/// wiring `mynd up`/`session-start` use), minus the background sync
/// loop — CLI subcommands are one-shot, so there's nothing to keep alive
/// after the command returns.
pub(crate) async fn open_store() -> Result<SqliteStore> {
    let settings = crate::config::load_server_settings(&crate::config::global_config_path())?;
    let db_path = crate::db::resolve_db_path();
    let database = crate::db::open_database(&settings.sync, &db_path).await?;
    let conn = database.connect()?;
    crate::db::run_migrations(&conn).await?;
    Ok(SqliteStore::new(conn))
}

/// Opens the org-layer store, if `[org_sync]` is configured. Mirrors the
/// server's org wiring in `main.rs`: a missing or unreachable org db never
/// fails the caller, it just means org-layer lookups are skipped.
pub(crate) async fn open_org_store() -> Option<SqliteStore> {
    let settings =
        crate::config::load_server_settings(&crate::config::global_config_path()).ok()?;
    let org_sync = settings.org_sync.as_ref()?;
    let org_db_path = crate::db::resolve_org_db_path();
    match crate::db::open_database(org_sync, &org_db_path).await {
        Ok(db) => match db.connect() {
            Ok(conn) => match crate::db::run_migrations(&conn).await {
                Ok(()) => Some(SqliteStore::new(conn)),
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

/// Tries the primary store first, then the org store — mirrors
/// `api::find_owning_store`. An org-store lookup failure degrades to "not
/// found in org" rather than propagating.
pub(crate) async fn find_owning<'a>(
    store: &'a SqliteStore,
    org_store: &'a Option<SqliteStore>,
    id: &str,
) -> Result<Option<&'a SqliteStore>> {
    if store.recall_by_id(id).await?.is_some() {
        return Ok(Some(store));
    }
    if let Some(org) = org_store {
        match org.recall_by_id(id).await {
            Ok(Some(_)) => return Ok(Some(org)),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(
                    "org store lookup failed for id {id}: {e:#}; treating as not found in org"
                );
            }
        }
    }
    Ok(None)
}

/// Runs an async block on a fresh current-thread runtime — the same pattern
/// `cmd_status`/`cmd_session_start` use so each CLI subcommand stays a
/// synchronous `fn` callable straight from `main`'s `match`.
pub(crate) fn block_on<T>(fut: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(fut)
}

/// Confirms a destructive action unless `--yes` was passed. Returns `Ok(true)`
/// to proceed.
pub(crate) fn confirm(prompt: &str, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    use std::io::Write as _;
    print!("{prompt} [y/N] ");
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

pub(crate) fn print_json(v: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
    );
}
