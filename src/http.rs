use crate::{
    api,
    config::{AgentSettings, ServerSettings, SyncSettings},
    server::Mynd,
    store::SqliteStore,
    suggest_session::SuggestSessionManager,
    update::SharedUpdateState,
};
use anyhow::Result;
use axum::{
    Router,
    body::Body,
    http::{StatusCode, header},
    response::Response,
    routing::get,
};
use include_dir::{Dir, include_dir};
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};
use std::sync::Arc;

static DASHBOARD: Dir = include_dir!("$CARGO_MANIFEST_DIR/dashboard/dist");
static PLACEHOLDER_HTML: &str = include_str!("dashboard_placeholder.html");

#[allow(clippy::too_many_arguments)]
pub fn app_router(
    store: Arc<SqliteStore>,
    org_store: Option<Arc<SqliteStore>>,
    sync: SyncSettings,
    org_sync: Option<SyncSettings>,
    notify_on_store: Option<Arc<tokio::sync::Notify>>,
    dashboard_origin: &str,
    events_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
    agent: AgentSettings,
    mcp_url: String,
    update_state: SharedUpdateState,
    guard_predefined_namespaces: bool,
    request_guard: api::guard::GuardConfig,
    shutdown: ShutdownSignal,
) -> Router {
    // Fires whenever a memory or edge is created/updated/deleted, either via
    // an MCP tool call (below) or the REST API (api::router) — the dashboard
    // subscribes to it over SSE to silently refresh in the background.
    let mcp = StreamableHttpService::new(
        {
            let store = store.clone();
            let org_store = org_store.clone();
            let trigger = notify_on_store.clone();
            let events_tx = events_tx.clone();
            move || {
                let mut mynd = match &trigger {
                    Some(t) => Mynd::with_sync(store.clone(), t.clone()),
                    None => Mynd::with_store(store.clone()),
                };
                if let Some(org) = &org_store {
                    mynd = mynd.with_org_store(org.clone());
                }
                Ok(mynd.with_events(events_tx.clone()))
            }
        },
        Arc::new(LocalSessionManager::default()),
        Default::default(),
    );
    let agent_for_status = agent.clone();
    let suggest = SuggestSessionManager::new(store.clone(), events_tx.clone(), agent, mcp_url);
    api::router(
        store,
        org_store,
        sync,
        org_sync,
        dashboard_origin,
        events_tx,
        suggest,
        update_state,
        agent_for_status,
        guard_predefined_namespaces,
    )
    .nest_service("/mcp", mcp)
    // Lets long-lived handlers (the SSE event stream) end when the server
    // is shutting down, so graceful shutdown can actually drain.
    .layer(axum::Extension(shutdown))
    // Outermost: rejects DNS-rebound and cross-site browser requests before
    // they reach either the REST routes or the MCP service. See api::guard.
    .layer(axum::middleware::from_fn_with_state(
        request_guard,
        api::guard::guard,
    ))
}

/// Broadcast "the server is stopping" to every listener, SSE handler and
/// the TUI. `false` until shutdown is requested, then `true` forever.
pub type ShutdownSignal = tokio::sync::watch::Receiver<bool>;

pub fn shutdown_channel() -> (tokio::sync::watch::Sender<bool>, ShutdownSignal) {
    tokio::sync::watch::channel(false)
}

/// Resolves once shutdown has been requested (or the sender is gone, which
/// only happens when the owning `run_up` has already returned).
pub async fn wait_for_shutdown(mut rx: ShutdownSignal) {
    while !*rx.borrow() {
        if rx.changed().await.is_err() {
            return;
        }
    }
}

/// Resolves on SIGTERM or SIGINT. `systemctl stop`, `launchctl unload`,
/// and `mynd status`'s `k` all send SIGTERM; Ctrl+C in `--plain`/headless
/// mode sends SIGINT (in the TUI, raw mode turns Ctrl+C into a key event
/// that `up_view` handles itself).
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let (mut term, mut int) = match (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) {
        (Ok(t), Ok(i)) => (t, i),
        (Err(e), _) | (_, Err(e)) => {
            tracing::warn!("could not install signal handlers ({e}); shutdown will be abrupt");
            std::future::pending::<()>().await;
            unreachable!()
        }
    };
    tokio::select! {
        _ = term.recv() => tracing::info!("received SIGTERM, shutting down"),
        _ = int.recv() => tracing::info!("received SIGINT, shutting down"),
    }
}

/// How long to wait for in-flight connections after shutdown is requested
/// before giving up on them. Long enough for a normal request; short enough
/// that `systemctl stop` never looks hung.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

pub fn dashboard_router(api_url: &str) -> Router {
    let config_js = format!("window.MYND_API = {};\n", serde_json::json!(api_url));
    Router::new()
        .route(
            "/config.js",
            get({
                let body = config_js.clone();
                move || {
                    let b = body.clone();
                    async move {
                        Response::builder()
                            .header(header::CONTENT_TYPE, "application/javascript")
                            .body(Body::from(b))
                            .unwrap()
                    }
                }
            }),
        )
        .fallback(get(|req: axum::extract::Request| async move {
            if !dashboard_is_bundled() {
                return Response::builder()
                    .header(header::CONTENT_TYPE, "text/html")
                    .body(Body::from(PLACEHOLDER_HTML))
                    .unwrap();
            }
            let path = req.uri().path().trim_start_matches('/');
            let path = if path.is_empty() { "index.html" } else { path };
            match DASHBOARD.get_file(path) {
                Some(file) => {
                    let mime = mime_guess::from_path(path).first_or_octet_stream();
                    Response::builder()
                        .header(header::CONTENT_TYPE, mime.as_ref())
                        .body(Body::from(file.contents()))
                        .unwrap()
                }
                None => {
                    // SPA fallback: serve index.html for unknown paths
                    match DASHBOARD.get_file("index.html") {
                        Some(file) => Response::builder()
                            .header(header::CONTENT_TYPE, "text/html")
                            .body(Body::from(file.contents()))
                            .unwrap(),
                        None => Response::builder()
                            .status(StatusCode::NOT_FOUND)
                            .body(Body::from("not found"))
                            .unwrap(),
                    }
                }
            }
        }))
}

/// A real vite build ships an assets/ directory; a source install has an
/// empty dashboard/dist (see build.rs) and falls back to PLACEHOLDER_HTML.
fn dashboard_is_bundled() -> bool {
    DASHBOARD.get_dir("assets").is_some()
}

/// Bridges writes made by other processes (the stdio MCP server the Claude
/// Code plugin spawns) into the dashboard SSE stream. In-process writes
/// already emit directly; data_version only moves on foreign commits.
pub fn spawn_change_poller(
    store: Arc<SqliteStore>,
    events: tokio::sync::broadcast::Sender<serde_json::Value>,
    interval: std::time::Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last: Option<i64> = None;
        loop {
            tokio::time::sleep(interval).await;
            match store.data_version().await {
                Ok(v) => {
                    if let Some(prev) = last
                        && v != prev
                    {
                        let _ = events.send(serde_json::json!({ "type": "changed" }));
                    }
                    last = Some(v);
                }
                Err(e) => tracing::debug!("data_version poll failed: {e:#}"),
            }
        }
    })
}

/// Removes the pidfile when dropped. Held for the lifetime of `run_up` so an
/// early `?` return (e.g. the dashboard listener failing to bind) still
/// cleans up; the Ctrl+C path in `tui::up_view` bypasses Drop entirely (it
/// calls `std::process::exit`) and removes the file itself instead.
struct PidGuard(std::path::PathBuf);

impl Drop for PidGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Binds a `TcpListener`, retrying briefly on `AddrInUse`. After an in-place
/// `exec()` self-update restart, the old process's listening socket (marked
/// `CLOEXEC`) closes the instant `exec()` runs, and the new process image
/// re-binds fresh — this absorbs the small window where the OS hasn't fully
/// released the port yet.
async fn bind_with_retry(host: &str, port: u16) -> Result<tokio::net::TcpListener> {
    let mut attempt = 0;
    loop {
        match tokio::net::TcpListener::bind((host, port)).await {
            Ok(listener) => return Ok(listener),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && attempt < 10 => {
                attempt += 1;
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Records this process's PID so `mynd status`'s `k` shortcut (a
/// separate process, with no other way to identify the server) can find and
/// signal it.
fn write_pidfile() -> Result<PidGuard> {
    let path = crate::db::up_pidfile_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, std::process::id().to_string())?;
    Ok(PidGuard(path))
}

pub async fn run_up(
    store: Arc<SqliteStore>,
    org_store: Option<Arc<SqliteStore>>,
    settings: &ServerSettings,
    headless: bool,
    plain: bool,
    notify_on_store: Option<Arc<tokio::sync::Notify>>,
) -> Result<()> {
    let (events_tx, _) = tokio::sync::broadcast::channel::<serde_json::Value>(16);
    spawn_change_poller(
        store.clone(),
        events_tx.clone(),
        std::time::Duration::from_secs(2),
    );
    let update_state: SharedUpdateState = Arc::new(tokio::sync::RwLock::new(
        crate::update::UpdateState::new_idle(),
    ));
    if settings.update.enabled {
        tokio::spawn(crate::update::run_update_check_loop(
            update_state.clone(),
            Arc::new(crate::update::GitHubVersionSource::new()),
            settings.update.check_interval_seconds,
            events_tx.clone(),
        ));
    }
    let mcp_host = match settings.host.as_str() {
        "0.0.0.0" | "::" => "127.0.0.1",
        h => h,
    };
    let mcp_url = format!("http://{}:{}/mcp", mcp_host, settings.port);
    let (shutdown_tx, shutdown_rx) = shutdown_channel();
    {
        let tx = shutdown_tx.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            let _ = tx.send(true);
        });
    }
    let app = app_router(
        store.clone(),
        org_store,
        settings.sync.clone(),
        settings.org_sync.clone(),
        notify_on_store,
        &settings.cors_origin,
        events_tx.clone(),
        settings.agent.clone(),
        mcp_url.clone(),
        update_state,
        settings.guard_predefined_namespaces,
        api::guard::GuardConfig::from_settings(settings),
        shutdown_rx.clone(),
    );

    if !matches!(settings.host.as_str(), "127.0.0.1" | "localhost" | "::1") {
        tracing::warn!(
            "binding to {}: the REST API and MCP endpoint are UNAUTHENTICATED; \
             anyone who can reach this address can read and modify all memories, \
             and can call POST /api/v1/suggest-sessions to spawn the configured agent command",
            settings.host
        );
    }

    let listener = bind_with_retry(settings.host.as_str(), settings.port).await?;
    let _pid_guard = write_pidfile()?;
    tracing::info!(
        "MCP endpoint:  http://{}:{}/mcp",
        settings.host,
        settings.port
    );
    tracing::info!(
        "REST API:      http://{}:{}/api/v1",
        settings.host,
        settings.port
    );
    if settings.sync.enabled {
        tracing::info!("Sync:          enabled → {}", settings.sync.remote_url);
    }

    let mut dashboard_url = None;
    let mut api_handle = tokio::spawn({
        let rx = shutdown_rx.clone();
        async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(wait_for_shutdown(rx))
                .await
        }
    });

    let dash_handle = if headless {
        None
    } else {
        if !dashboard_is_bundled() {
            tracing::warn!(
                "dashboard assets are not bundled in this build (source install). \
                 The dashboard page will show setup instructions. \
                 Use a prebuilt release binary, or run `bun install && bun run build` in dashboard/ and rebuild."
            );
        }
        let dash = dashboard_router(&settings.api_url);
        let dash_listener =
            bind_with_retry(settings.host.as_str(), settings.dashboard_port).await?;
        tracing::info!(
            "Dashboard:     http://{}:{}",
            settings.host,
            settings.dashboard_port
        );
        dashboard_url = Some(format!(
            "http://{}:{}",
            settings.host, settings.dashboard_port
        ));
        Some(tokio::spawn({
            let rx = shutdown_rx.clone();
            async move {
                axum::serve(dash_listener, dash)
                    .with_graceful_shutdown(wait_for_shutdown(rx))
                    .await
            }
        }))
    };

    let run_tui = !headless && crate::tui::is_interactive(plain);
    if run_tui {
        let data = crate::cli::build_status_data(
            &std::env::current_dir()?,
            &crate::config::global_config_path(),
            &store,
            &crate::db::resolve_db_path(),
            &[],
            settings,
            true,
        )
        .await?;
        let exit = crate::tui::up_view::run(
            data,
            dashboard_url,
            mcp_url,
            events_tx,
            store.clone(),
            shutdown_rx.clone(),
        )
        .await?;
        // Terminal is already restored by up_view's TerminalGuard.
        match exit {
            crate::tui::up_view::UpExit::Detach => {
                // Actually detach: stop this process's listeners so a
                // re-exec'd child can rebind the same port, hand off the
                // pidfile, and exit — the shell gets its prompt back
                // immediately, and the child survives this terminal closing
                // (new session, stdio off the tty).
                api_handle.abort();
                if let Some(h) = dash_handle {
                    h.abort();
                }
                for _ in 0..20 {
                    if !crate::cli::probe_server_up(settings) {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                drop(_pid_guard); // removes the pidfile now; the child writes its own on bind
                spawn_detached_child(headless)?;
                std::process::exit(0);
            }
            crate::tui::up_view::UpExit::Stop => {
                let _ = shutdown_tx.send(true);
            }
        }
    }

    // Run until either the API listener stops on its own (accept error) or
    // shutdown is requested; then give both servers a bounded window to
    // finish in-flight requests. `_pid_guard` drops on return, removing the
    // pidfile in every path except detach (handled above).
    let api_result = tokio::select! {
        r = &mut api_handle => Some(r),
        _ = wait_for_shutdown(shutdown_rx.clone()) => None,
    };
    let _ = shutdown_tx.send(true);
    let drain = async {
        if api_result.is_none() {
            let _ = api_handle.await;
        }
        if let Some(h) = dash_handle {
            let _ = h.await;
        }
    };
    if tokio::time::timeout(SHUTDOWN_GRACE, drain).await.is_err() {
        tracing::warn!(
            "connections did not drain within {}s; exiting anyway",
            SHUTDOWN_GRACE.as_secs()
        );
    }
    if let Some(r) = api_result {
        r??;
    }
    tracing::info!("server stopped");
    Ok(())
}

/// Re-execs this binary as `mynd up [--headless] --plain`, detached from
/// the controlling terminal (new session via `setsid`, stdio redirected to a
/// log file), and does not wait for it. Used by the `up` TUI's `d` (detach)
/// key: the caller aborts its own listeners and exits right after this
/// returns, so the child can bind the now-free port.
fn spawn_detached_child(headless: bool) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let exe = std::env::current_exe()?;
    let log_path = crate::db::xdg_data_dir().join("mynd.detached.log");
    if let Some(dir) = log_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let log_out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let log_err = log_out.try_clone()?;

    let mut cmd = std::process::Command::new(exe);
    cmd.arg("up");
    if headless {
        cmd.arg("--headless");
    }
    // Detached child has no controlling tty; force plain output.
    cmd.arg("--plain");
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log_out))
        .stderr(std::process::Stdio::from(log_err));
    // Safety: setsid() only detaches the child from the parent's controlling
    // terminal/session; it touches no shared state and can't fail in a way
    // that leaves the child (or this process) in an inconsistent state.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd.spawn()?;
    Ok(())
}

pub async fn run_dashboard(settings: &ServerSettings, open: bool) -> Result<()> {
    let dash = dashboard_router(&settings.api_url);
    let listener =
        tokio::net::TcpListener::bind((settings.host.as_str(), settings.dashboard_port)).await?;
    let url = format!("http://{}:{}", settings.host, settings.dashboard_port);
    tracing::info!("Dashboard:     {url}  (API: {})", settings.api_url);
    if open {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let _ = std::process::Command::new(opener).arg(&url).spawn();
    }
    axum::serve(listener, dash).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::SyncSettings, db, store::SqliteStore};

    #[tokio::test]
    async fn unbundled_dashboard_serves_placeholder() {
        // Only meaningful for a source build without dashboard/dist/assets;
        // a local `bun run build` before `cargo build` legitimately bundles
        // the real dashboard and this assertion is skipped.
        if dashboard_is_bundled() {
            return;
        }
        let dash = dashboard_router("http://127.0.0.1:3456");
        let resp = dash
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(std::str::from_utf8(&body).unwrap().contains("not bundled"));
    }
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tower::ServiceExt;

    async fn test_store() -> (Arc<SqliteStore>, TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let sync = SyncSettings::default();
        let database = db::open_database(&sync, path.to_str().unwrap())
            .await
            .unwrap();
        let conn = database.connect().unwrap();
        db::run_migrations(&conn).await.unwrap();
        (Arc::new(SqliteStore::new(conn)), dir)
    }

    /// Writes a stub agent script (mirrors the suggest_session test stub) so
    /// app_router tests don't depend on a real `claude` binary being on PATH.
    fn write_stub_agent(dir: &std::path::Path) -> String {
        let script = dir.join("stub-agent.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\necho '{\"type\":\"result\",\"session_id\":\"stub-1\",\"result\":\"done\",\"is_error\":false}'\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn app_router_serves_rest_api() {
        let (store, dir) = test_store().await;
        let (events_tx, _) = tokio::sync::broadcast::channel::<serde_json::Value>(16);
        let agent = crate::config::AgentSettings {
            command: write_stub_agent(dir.path()),
            args: vec![],
            kind: crate::config::AgentKind::Claude,
        };
        let app = app_router(
            store,
            None,
            crate::config::SyncSettings::default(),
            None,
            None,
            "http://127.0.0.1:3457",
            events_tx,
            agent,
            "http://127.0.0.1:3456/mcp".into(),
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::update::UpdateState::new_idle(),
            )),
            true,
            api::guard::GuardConfig::loopback_only(),
            shutdown_channel().1,
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// Full app router with the request guard, as `mynd up` builds it.
    async fn guarded_app() -> (Router, TempDir) {
        let (store, dir) = test_store().await;
        let (events_tx, _) = tokio::sync::broadcast::channel::<serde_json::Value>(16);
        let agent = crate::config::AgentSettings {
            command: write_stub_agent(dir.path()),
            args: vec![],
            kind: crate::config::AgentKind::Claude,
        };
        let app = app_router(
            store,
            None,
            crate::config::SyncSettings::default(),
            None,
            None,
            "http://127.0.0.1:3457",
            events_tx,
            agent,
            "http://127.0.0.1:3456/mcp".into(),
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::update::UpdateState::new_idle(),
            )),
            true,
            api::guard::GuardConfig::loopback_only(),
            shutdown_channel().1,
        );
        (app, dir)
    }

    async fn send(app: Router, method: &str, uri: &str, headers: &[(&str, &str)]) -> StatusCode {
        let mut b = Request::builder().method(method).uri(uri);
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        app.oneshot(b.body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn sse_stream_ends_when_shutdown_is_signalled() {
        // Without this, graceful shutdown would wait on every open
        // dashboard tab forever (the event stream never ends on its own).
        let (store, dir) = test_store().await;
        let (events_tx, _) = tokio::sync::broadcast::channel::<serde_json::Value>(16);
        let (shutdown_tx, shutdown_rx) = shutdown_channel();
        let agent = crate::config::AgentSettings {
            command: write_stub_agent(dir.path()),
            args: vec![],
            kind: crate::config::AgentKind::Claude,
        };
        let app = app_router(
            store,
            None,
            crate::config::SyncSettings::default(),
            None,
            None,
            "http://127.0.0.1:3457",
            events_tx,
            agent,
            "http://127.0.0.1:3456/mcp".into(),
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::update::UpdateState::new_idle(),
            )),
            true,
            api::guard::GuardConfig::loopback_only(),
            shutdown_rx,
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/events")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body();
        let collect = body.collect();
        tokio::pin!(collect);
        // Still streaming: collecting must not finish yet.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(200), &mut collect)
                .await
                .is_err(),
            "stream ended before shutdown"
        );
        shutdown_tx.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), collect)
            .await
            .expect("stream must end within 2s of shutdown")
            .unwrap();
    }

    #[tokio::test]
    async fn guard_rejects_dns_rebound_host_on_rest_and_mcp() {
        let (app, _dir) = guarded_app().await;
        let s = send(
            app.clone(),
            "GET",
            "/api/v1/status",
            &[("host", "evil.example")],
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let s = send(
            app.clone(),
            "POST",
            "/mcp",
            &[("host", "evil.example:3456")],
        )
        .await;
        assert_eq!(
            s,
            StatusCode::FORBIDDEN,
            "the MCP endpoint must be guarded too"
        );
        let s = send(app, "GET", "/api/v1/status", &[("host", "localhost:3456")]).await;
        assert_eq!(s, StatusCode::OK);
    }

    #[tokio::test]
    async fn guard_rejects_cross_site_post_but_allows_dashboard_and_cli() {
        let (app, _dir) = guarded_app().await;
        // Bodyless POST from a hostile page: the CSRF case.
        let s = send(
            app.clone(),
            "POST",
            "/api/v1/suggest-sessions",
            &[
                ("host", "127.0.0.1:3456"),
                ("origin", "https://evil.example"),
            ],
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        let s = send(
            app.clone(),
            "POST",
            "/api/v1/update/apply",
            &[("host", "127.0.0.1:3456"), ("origin", "null")],
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        // Reads are not CSRF-relevant; CORS + the Host check cover them.
        let s = send(
            app.clone(),
            "GET",
            "/api/v1/status",
            &[
                ("host", "127.0.0.1:3456"),
                ("origin", "https://evil.example"),
            ],
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        // The dashboard's own origin, and a CLI/MCP client with no Origin,
        // both get through to the handler (which here 202s and runs the stub).
        let s = send(
            app.clone(),
            "DELETE",
            "/api/v1/suggest-sessions/current",
            &[
                ("host", "127.0.0.1:3456"),
                ("origin", "http://localhost:3457"),
            ],
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        let s = send(app, "DELETE", "/api/v1/suggest-sessions/current", &[]).await;
        assert_eq!(s, StatusCode::OK);
    }

    #[tokio::test]
    async fn poller_emits_changed_when_other_connection_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hm.db");
        let db1 = libsql::Builder::new_local(&path).build().await.unwrap();
        let conn1 = db1.connect().unwrap();
        crate::db::run_migrations(&conn1).await.unwrap();
        let store = Arc::new(crate::store::SqliteStore::new(conn1));

        let (events, mut rx) = tokio::sync::broadcast::channel::<serde_json::Value>(16);
        let _h = spawn_change_poller(store, events, std::time::Duration::from_millis(50));

        // let the poller take its baseline reading first
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;

        // foreign write through a second connection
        let db2 = libsql::Builder::new_local(&path).build().await.unwrap();
        let conn2 = db2.connect().unwrap();
        crate::db::init_connection(&conn2).await.unwrap();
        conn2
            .execute(
                "INSERT INTO memories (id, title, content, created_at, updated_at, token_count, layer, memory_type)
                 VALUES ('mem_x', 't', 'c', 1, 1, 1, 'workspace', 'project')",
                (),
            )
            .await
            .unwrap();

        let evt = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("poller should emit within 2s")
            .unwrap();
        assert_eq!(evt["type"], "changed");
    }

    #[tokio::test]
    async fn dashboard_router_spa_fallback_returns_html_for_unknown_path() {
        let dash = dashboard_router("http://127.0.0.1:3456");
        let resp = dash
            .oneshot(
                Request::builder()
                    .uri("/some/unknown/route")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // SPA fallback: unknown paths serve index.html (200 with html content-type)
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers()["content-type"].to_str().unwrap();
        assert!(
            ct.contains("text/html"),
            "SPA fallback should serve HTML, got: {ct}"
        );
    }

    #[tokio::test]
    async fn bind_with_retry_binds_an_ephemeral_port() {
        let listener = bind_with_retry("127.0.0.1", 0).await.unwrap();
        assert!(listener.local_addr().unwrap().port() > 0);
    }

    #[test]
    fn write_pidfile_writes_pid_and_guard_removes_it_on_drop() {
        let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: test-only env mutation; serialised by ENV_MUTEX.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", dir.path());
        }
        let path = crate::db::up_pidfile_path();
        let guard = write_pidfile().unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, std::process::id().to_string());
        drop(guard);
        assert!(!path.exists(), "PidGuard's Drop must remove the pidfile");
        // SAFETY: test-only env mutation; serialised by ENV_MUTEX.
        unsafe {
            std::env::remove_var("XDG_DATA_HOME");
        }
    }

    #[tokio::test]
    async fn dashboard_router_serves_html_and_config_js() {
        let dash = dashboard_router("http://127.0.0.1:3456");
        let resp = dash
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(std::str::from_utf8(&body).unwrap().contains("<html"));

        let resp = dash
            .oneshot(
                Request::builder()
                    .uri("/config.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers()["content-type"], "application/javascript");
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            std::str::from_utf8(&body).unwrap().trim(),
            "window.MYND_API = \"http://127.0.0.1:3456\";"
        );
    }
}
