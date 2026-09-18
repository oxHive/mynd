use crate::{
    api,
    config::{AgentSettings, ServerSettings, SyncSettings},
    hive::identity::DeviceIdentity,
    server::HiveMind,
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
    sync: SyncSettings,
    notify_on_store: Option<Arc<tokio::sync::Notify>>,
    dashboard_origin: &str,
    events_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
    agent: AgentSettings,
    mcp_url: String,
    update_state: SharedUpdateState,
    guard_predefined_namespaces: bool,
    hive_enabled: bool,
    hive_identity: Option<DeviceIdentity>,
    pairing_codes: Arc<crate::hive::pairing::PairingCodeStore>,
    pairing_window: Option<Arc<crate::hive::pairing_window::PairingWindow>>,
    hive_sync_port: u16,
    restart_notify: Arc<tokio::sync::Notify>,
) -> Router {
    // Fires whenever a memory or edge is created/updated/deleted, either via
    // an MCP tool call (below) or the REST API (api::router) — the dashboard
    // subscribes to it over SSE to silently refresh in the background.
    let mcp = StreamableHttpService::new(
        {
            let store = store.clone();
            let trigger = notify_on_store.clone();
            let events_tx = events_tx.clone();
            let hive_identity = hive_identity.clone();
            move || {
                let hivemind = match &trigger {
                    Some(t) => HiveMind::with_sync(store.clone(), t.clone()),
                    None => HiveMind::with_store(store.clone()),
                };
                let hivemind = hivemind.with_events(events_tx.clone());
                let hivemind = match (hive_enabled, &hive_identity) {
                    (true, Some(identity)) => hivemind.with_hive(identity.clone()),
                    _ => hivemind,
                };
                Ok(hivemind)
            }
        },
        Arc::new(LocalSessionManager::default()),
        Default::default(),
    );
    let agent_for_status = agent.clone();
    let suggest = SuggestSessionManager::new(store.clone(), events_tx.clone(), agent, mcp_url);
    api::router(
        store,
        sync,
        dashboard_origin,
        events_tx,
        suggest,
        update_state,
        agent_for_status,
        guard_predefined_namespaces,
        hive_enabled,
        hive_identity,
        pairing_codes,
        pairing_window,
        crate::api::HiveSyncPort(hive_sync_port),
    )
    .nest_service("/mcp", mcp)
    // Layered on the fully-composed outer app (not inside `api::router`): the
    // `POST /api/v1/hive/enabled` handler lives on `router()`'s merged
    // loopback-gated sub-router, and an outer Extension layer here reaches it
    // (unlike extensions layered on the *main* router before the merge). It is
    // provided at this boundary rather than inside `router()` because the
    // restart lifecycle it drives is owned by `run_up`, and because an inner
    // layer would shadow any Notify a `router()` caller supplies from outside
    // (see `api::tests::set_hive_enabled_persists_override_and_signals_restart`).
    .layer(axum::Extension(restart_notify))
}

pub fn dashboard_router(api_url: &str) -> Router {
    let config_js = format!("window.HIVEMIND_API = {};\n", serde_json::json!(api_url));
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

/// Records this process's PID so `hivemind status`'s `k` shortcut (a
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
    let pairing_codes = Arc::new(crate::hive::pairing::PairingCodeStore::new());
    // Fired by `POST /api/v1/hive/enabled` after it persists the DB override;
    // `run_up`'s tail races it against the normal serve-await and re-execs
    // this process so the child re-reads the override on a fresh boot.
    let restart_notify = Arc::new(tokio::sync::Notify::new());
    // The DB override (set via the dashboard's enable/disable toggle, a
    // later plan) wins when present; config.toml's [hive] enabled remains
    // the default otherwise -- same relationship hive_settings_override
    // already has to config.toml's [hive] interval fields.
    let hive_enabled = store
        .hive_enabled_override()
        .await?
        .unwrap_or(settings.hive.enabled);
    // config.toml refuses [sync] + [hive] together at load time, but the DB
    // override is applied after that check. Don't let it become a bypass:
    // keep hive off (loudly) rather than running both sync modes at once --
    // and don't bail, or a stray override would turn into a restart loop.
    let hive_enabled = if hive_enabled && settings.sync.enabled {
        tracing::error!(
            "Hive Mode is enabled (dashboard override) but [sync] cloud sync is also enabled; \
             they are mutually exclusive, so Hive Mode stays OFF until [sync] is disabled"
        );
        false
    } else {
        hive_enabled
    };
    let (hive_sync_port, hive_pairing_port) = crate::hive::hive_ports(settings.port)?;
    // Bootstrapped up-front (rather than only inside the `hive_enabled`
    // block further down) so the REST/MCP write handlers wired into `app_router`
    // below can spawn push-on-change attempts (Plan 2 Task 11) using this same
    // identity, instead of each handler needing its own bootstrap call.
    let hive_identity: Option<DeviceIdentity> = if hive_enabled {
        let key_store = crate::hive::keyring_store::KeyringHiveKeyStore;
        Some(crate::hive::bootstrap_self_identity(&store, &key_store).await?)
    } else {
        None
    };
    // The pairing-window coordinator owns the server-TLS-only pairing listener,
    // opened lazily (and time-bounded) each time a pairing code is issued via
    // the plaintext `POST /api/v1/hive/pairing-code` handler. Built up-front so
    // that handler (wired into `app_router` below) can reach it as an
    // Extension; only present when hive is enabled.
    let pairing_window: Option<Arc<crate::hive::pairing_window::PairingWindow>> =
        if let Some(identity) = hive_identity.clone() {
            let pairing_router = api::hive_pairing_router(store.clone(), pairing_codes.clone());
            Some(Arc::new(crate::hive::pairing_window::PairingWindow::new(
                settings.host.clone(),
                hive_pairing_port,
                identity,
                pairing_router,
            )))
        } else {
            None
        };
    let app = app_router(
        store.clone(),
        settings.sync.clone(),
        notify_on_store,
        &settings.cors_origin,
        events_tx.clone(),
        settings.agent.clone(),
        mcp_url.clone(),
        update_state,
        settings.guard_predefined_namespaces,
        hive_enabled,
        hive_identity.clone(),
        pairing_codes.clone(),
        pairing_window.clone(),
        if hive_enabled { hive_sync_port } else { 0 },
        restart_notify.clone(),
    );

    if !matches!(settings.host.as_str(), "127.0.0.1" | "localhost" | "::1") {
        tracing::warn!(
            "binding to {}: the REST API and MCP endpoint are UNAUTHENTICATED; \
             anyone who can reach this address can read and modify all memories, \
             and can call POST /api/v1/suggest-sessions to spawn the configured agent command",
            settings.host
        );
        if hive_enabled {
            tracing::warn!(
                "binding to {}: Hive Mode exposes a mandatory-mTLS sync port ({}) and, only \
                 while a pairing code is outstanding, a server-TLS-only pairing port ({}); the \
                 pairing code itself is issued via the local plaintext POST \
                 /api/v1/hive/pairing-code — anyone who can reach that endpoint can open a \
                 pairing window and join this device's roster, so only run Hive Mode on a \
                 network you trust",
                settings.host,
                hive_sync_port,
                hive_pairing_port
            );
        }
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

    if hive_enabled {
        let identity = hive_identity
            .clone()
            .expect("hive_identity is Some whenever hive_enabled, bootstrapped above");

        let trusted = store.hive_trusted_networks().await?;
        let current = crate::hive::network::current_network_key_async().await;
        let start_now = trusted.is_empty()
            || current
                .as_deref()
                .map(|c| trusted.iter().any(|t| t.id == c))
                .unwrap_or(false);

        let stack: Arc<tokio::sync::Mutex<Option<crate::hive::network_guard::HiveStackHandle>>> =
            Arc::new(tokio::sync::Mutex::new(None));
        let guard_params = crate::hive::network_guard::HiveStackParams {
            host: settings.host.clone(),
            port: settings.port,
            sync_interval_seconds: settings.hive.sync_interval_seconds,
            ping_interval_seconds: settings.hive.ping_interval_seconds,
        };
        if start_now {
            let handle = crate::hive::network_guard::spawn_hive_stack(
                store.clone(),
                identity.clone(),
                guard_params.clone(),
                events_tx.clone(),
            )
            .await?;
            *stack.lock().await = Some(handle);
        } else {
            tracing::warn!(
                "current network is not in the trusted-networks list; Hive Mode starting paused"
            );
        }

        tokio::spawn(crate::hive::network_guard::run_guard_loop(
            store.clone(),
            identity,
            guard_params,
            events_tx.clone(),
            stack,
        ));
    }

    let mut dashboard_url = None;
    // `with_connect_info` makes the real TCP peer address available to
    // handlers/middleware via the `ConnectInfo` extractor (used by
    // `api::require_loopback` to gate the trusted-networks endpoints,
    // issue #27) -- unlike a header, it can't be spoofed by the client.
    let mut api_handle = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
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
        Some(tokio::spawn(async move {
            axum::serve(dash_listener, dash).await
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
        crate::tui::up_view::run(
            data,
            dashboard_url,
            mcp_url,
            events_tx,
            store.clone(),
            restart_notify.clone(),
        )
        .await?;
        // `d` returns here: terminal is already restored by up_view's TerminalGuard.
        // Actually detach: stop this process's listeners so a re-exec'd child
        // can rebind the same port, hand off the pidfile, and exit — the
        // shell gets its prompt back immediately, and the child survives
        // this terminal closing (new session, stdio off the tty).
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

    tokio::select! {
        result = &mut api_handle => { result??; }
        _ = restart_notify.notified() => {
            // Same sequence the TUI's detach key already runs: stop this
            // process's listeners so the re-exec'd child can rebind the now-
            // free port, hand off the pidfile, and exit. The child re-reads
            // hive_enabled_override on its own fresh boot (Task 3), which is
            // the whole point -- no other state travels through the restart.
            api_handle.abort();
            if let Some(h) = &dash_handle {
                h.abort();
            }
            for _ in 0..20 {
                if !crate::cli::probe_server_up(settings) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            drop(_pid_guard);
            spawn_detached_child(headless)?;
            std::process::exit(0);
        }
    }
    if let Some(h) = dash_handle {
        h.await??;
    }
    Ok(())
}

/// Re-execs this binary as `hivemind up [--headless] --plain`, detached from
/// the controlling terminal (new session via `setsid`, stdio redirected to a
/// log file), and does not wait for it. Used by the `up` TUI's `d` (detach)
/// key: the caller aborts its own listeners and exits right after this
/// returns, so the child can bind the now-free port.
fn spawn_detached_child(headless: bool) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let exe = std::env::current_exe()?;
    let log_path = crate::db::xdg_data_dir().join("hivemind.detached.log");
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

    #[test]
    fn hive_tls_uses_explicit_crypto_provider_not_ambiguous_default() {
        // Both `ring` and `aws-lc-rs` end up compiled in (our own rustls
        // feature selects `ring`; axum-server's own rustls dependency pulls
        // in `aws-lc-rs`), so rustls's implicit per-process default
        // resolution is genuinely ambiguous. `rustls::ServerConfig::builder()`
        // panics in that situation -- this test proves the explicit
        // `builder_with_provider(ring::default_provider())` path used in
        // `run_up` avoids that panic, which a plain `cargo build`/compile
        // check can't catch since the panic only fires at runtime.
        let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
        let result = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(rustls::DEFAULT_VERSIONS);
        assert!(result.is_ok(), "explicit provider selection must not fail");

        // Prove the ambiguity is real, not hypothetical: the bare builder()
        // this fix replaced genuinely panics in this exact build (both
        // crypto backends compiled in, no process-wide default installed).
        // Without this assertion, the test above would pass identically
        // even if only one backend were ever compiled in, and wouldn't
        // prove anything about the bug it's guarding against.
        // Note: this prints a panic backtrace to stderr even though the test
        // passes -- that's expected and benign (it's the panic this test is
        // proving happens). Not suppressing it via a custom panic hook here,
        // since std::panic::set_hook/take_hook are process-global and this
        // test binary runs many tests in parallel threads; swapping the hook
        // could race with a genuine panic in another concurrently-running
        // test and swallow its real backtrace.
        let bare_builder_panics = std::panic::catch_unwind(rustls::ServerConfig::builder).is_err();
        assert!(
            bare_builder_panics,
            "expected the bare ServerConfig::builder() to panic on an ambiguous \
             default crypto provider -- if this now passes, either the dependency \
             graph changed to only compile in one backend (fine, but re-check \
             whether the explicit-provider workaround above is still needed) or \
             something upstream started installing a process-wide default before \
             this test runs (also worth understanding before treating this as safe)"
        );
    }

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
            crate::config::SyncSettings::default(),
            None,
            "http://127.0.0.1:3457",
            events_tx,
            agent,
            "http://127.0.0.1:3456/mcp".into(),
            std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::update::UpdateState::new_idle(),
            )),
            true,
            false,
            None,
            std::sync::Arc::new(crate::hive::pairing::PairingCodeStore::new()),
            None,
            0,
            std::sync::Arc::new(tokio::sync::Notify::new()),
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
            "window.HIVEMIND_API = \"http://127.0.0.1:3456\";"
        );
    }
}
