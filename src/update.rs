use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type Events = tokio::sync::broadcast::Sender<serde_json::Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    Idle,
    Checking,
    Updating,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateState {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub available: bool,
    pub release_notes_md: Option<String>,
    pub release_url: Option<String>,
    pub checked_at: Option<i64>,
    pub status: UpdateStatus,
    pub error: Option<String>,
    /// Unix seconds; echoed back over `GET /api/v1/update` so a page reload
    /// mid-update can re-anchor its elapsed-time counter.
    pub update_started_at: Option<i64>,
    pub platform_supported: bool,
}

impl UpdateState {
    pub fn new_idle() -> Self {
        UpdateState {
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            latest_version: None,
            available: false,
            release_notes_md: None,
            release_url: None,
            checked_at: None,
            status: UpdateStatus::Idle,
            error: None,
            update_started_at: None,
            platform_supported: cfg!(unix),
        }
    }
}

pub type SharedUpdateState = Arc<RwLock<UpdateState>>;

pub struct ReleaseInfo {
    pub version: String,
    pub notes_md: String,
    pub html_url: String,
}

/// Fetches release info from GitHub's releases API. The URL is overridable
/// (constructor param, or `MYND_UPDATE_CHECK_URL` env var for the
/// production default) so tests and manual E2E runs can point this at a
/// local mock server instead of the real GitHub API.
pub struct GitHubVersionSource {
    client: reqwest::Client,
    api_url: String,
}

impl Default for GitHubVersionSource {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHubVersionSource {
    pub fn new() -> Self {
        let api_url = std::env::var("MYND_UPDATE_CHECK_URL").unwrap_or_else(|_| {
            "https://api.github.com/repos/oxhive/mynd/releases/latest".to_string()
        });
        GitHubVersionSource::with_url(api_url)
    }

    pub fn with_url(api_url: String) -> Self {
        GitHubVersionSource {
            client: reqwest::Client::new(),
            api_url,
        }
    }

    pub async fn latest(&self) -> Result<ReleaseInfo> {
        #[derive(serde::Deserialize)]
        struct GhRelease {
            tag_name: String,
            body: Option<String>,
            html_url: String,
        }

        let resp = self
            .client
            .get(&self.api_url)
            .header("User-Agent", concat!("mynd/", env!("CARGO_PKG_VERSION")))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .context("requesting latest release info")?;
        if !resp.status().is_success() {
            anyhow::bail!("release check returned HTTP {}", resp.status());
        }
        let gh: GhRelease = resp.json().await.context("parsing release response")?;
        let version = gh
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&gh.tag_name)
            .to_string();
        Ok(ReleaseInfo {
            version,
            notes_md: gh.body.unwrap_or_default(),
            html_url: gh.html_url,
        })
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

async fn check_once(state: &SharedUpdateState, source: &GitHubVersionSource, events: &Events) {
    {
        let s = state.read().await;
        if s.status == UpdateStatus::Updating {
            return;
        }
    }

    let release = match source.latest().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("update check failed: {e:#}");
            let mut s = state.write().await;
            s.error = Some(format!("{e:#}"));
            s.checked_at = Some(now_unix());
            return;
        }
    };

    let current = env!("CARGO_PKG_VERSION");
    let is_newer = match (
        semver::Version::parse(current),
        semver::Version::parse(&release.version),
    ) {
        (Ok(cur), Ok(latest)) => latest > cur,
        _ => {
            tracing::warn!(
                "could not parse versions for comparison (current={current}, latest={})",
                release.version
            );
            false
        }
    };

    let mut s = state.write().await;
    let was_available = s.available;
    s.latest_version = Some(release.version.clone());
    s.release_notes_md = Some(release.notes_md.clone());
    s.release_url = Some(release.html_url.clone());
    s.checked_at = Some(now_unix());
    s.available = is_newer;
    s.error = None;
    if is_newer && !was_available {
        let _ = events.send(json!({
            "type": "update_available",
            "latest_version": release.version,
            "release_url": release.html_url,
        }));
    }
}

pub async fn run_update_check_loop(
    state: SharedUpdateState,
    source: Arc<GitHubVersionSource>,
    interval_secs: u64,
    events: Events,
) {
    check_once(&state, source.as_ref(), &events).await;
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticker.tick().await; // consume the immediate first tick
    loop {
        ticker.tick().await;
        check_once(&state, source.as_ref(), &events).await;
    }
}

pub async fn run_update(state: SharedUpdateState, events: Events) {
    if let Err(e) = do_update().await {
        tracing::error!("update failed: {e:#}");
        let mut s = state.write().await;
        s.status = UpdateStatus::Failed;
        s.error = Some(format!("{e:#}"));
        let _ = events.send(json!({
            "type": "update_failed",
            "error": s.error.clone(),
        }));
    }
}

async fn do_update() -> Result<()> {
    // Resolved before run_binstall(), not after: cargo-binstall replaces this
    // binary's path via an atomic rename, which unlinks the running
    // process's original inode. Post-replace, /proc/self/exe (what
    // std::env::current_exe reads) resolves to "<path> (deleted)" — a path
    // that doesn't exist, so exec() on it fails with ENOENT. Resolving here
    // captures the plain path, which still resolves correctly to the new
    // binary once the rename lands.
    let exe = std::env::current_exe().context("resolving current executable path")?;
    ensure_binstall_available().await?;
    run_binstall().await?;
    restart(&exe)
}

async fn ensure_binstall_available() -> Result<()> {
    let ok = tokio::process::Command::new("cargo")
        .args(["binstall", "-V"])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ok {
        anyhow::bail!(
            "cargo-binstall is not installed — install it from \
             https://github.com/cargo-bins/cargo-binstall, then try again"
        );
    }
    Ok(())
}

async fn run_binstall() -> Result<()> {
    let output = tokio::process::Command::new("cargo")
        .args(["binstall", "oxmynd", "--no-confirm", "--force"])
        .kill_on_drop(true)
        .output()
        .await
        .context("failed to run cargo binstall")?;
    if !output.status.success() {
        anyhow::bail!(
            "cargo binstall failed (exit {}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Re-execs the current binary with its original argv, replacing the running
/// process image in place (same PID). Preserves whatever flags this process
/// was started with (e.g. `up --headless`), regardless of whether it's
/// running under systemd/launchd or a foreground terminal. Never returns on
/// success — only returns (as an `Err`) if `exec()` itself fails.
#[cfg(unix)]
fn restart(exe: &std::path::Path) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let err = std::process::Command::new(exe).args(args).exec();
    Err(anyhow::anyhow!("exec() failed: {err}"))
}

#[cfg(not(unix))]
fn restart(_exe: &std::path::Path) -> Result<()> {
    anyhow::bail!(
        "binary updated, but automatic restart is only supported on Unix — \
         please restart mynd manually to pick up the new version"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, routing::get};

    async fn mock_release_server(tag_name: &str, body: &str) -> String {
        let tag_name = tag_name.to_string();
        let body = body.to_string();
        let app = Router::new().route(
            "/release",
            get(move || {
                let tag_name = tag_name.clone();
                let body = body.clone();
                async move {
                    Json(json!({
                        "tag_name": tag_name,
                        "body": body,
                        "html_url": "https://github.com/oxhive/mynd/releases/tag/test",
                    }))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}/release")
    }

    #[tokio::test]
    async fn version_source_parses_release_response() {
        let url = mock_release_server("v99.0.0", "some notes").await;
        let source = GitHubVersionSource::with_url(url);
        let release = source.latest().await.unwrap();
        assert_eq!(release.version, "99.0.0");
        assert_eq!(release.notes_md, "some notes");
    }

    #[tokio::test]
    async fn check_once_marks_available_and_broadcasts_on_newer_version() {
        let url = mock_release_server("v99.0.0", "notes").await;
        let source = GitHubVersionSource::with_url(url);
        let state: SharedUpdateState = Arc::new(RwLock::new(UpdateState::new_idle()));
        let (tx, mut rx) = tokio::sync::broadcast::channel(4);

        check_once(&state, &source, &tx).await;

        let s = state.read().await;
        assert!(s.available);
        assert_eq!(s.latest_version.as_deref(), Some("99.0.0"));
        let msg = rx.try_recv().expect("expected a broadcast on transition");
        assert_eq!(msg["type"], "update_available");
    }

    #[tokio::test]
    async fn check_once_does_not_flag_available_for_older_or_equal_version() {
        let url = mock_release_server(env!("CARGO_PKG_VERSION"), "notes").await;
        let source = GitHubVersionSource::with_url(url);
        let state: SharedUpdateState = Arc::new(RwLock::new(UpdateState::new_idle()));
        let (tx, mut rx) = tokio::sync::broadcast::channel(4);

        check_once(&state, &source, &tx).await;

        let s = state.read().await;
        assert!(!s.available);
        assert!(
            rx.try_recv().is_err(),
            "should not broadcast when not newer"
        );
    }

    #[tokio::test]
    async fn check_once_only_broadcasts_once_across_repeated_checks() {
        let url = mock_release_server("v99.0.0", "notes").await;
        let source = GitHubVersionSource::with_url(url);
        let state: SharedUpdateState = Arc::new(RwLock::new(UpdateState::new_idle()));
        let (tx, rx) = tokio::sync::broadcast::channel(4);

        check_once(&state, &source, &tx).await;
        check_once(&state, &source, &tx).await;

        assert_eq!(rx.len(), 1, "only the first transition should broadcast");
    }

    #[tokio::test]
    async fn check_once_on_fetch_error_leaves_availability_untouched() {
        // Nothing listening at this URL — request will fail.
        let source = GitHubVersionSource::with_url("http://127.0.0.1:1/release".to_string());
        let state: SharedUpdateState = Arc::new(RwLock::new(UpdateState::new_idle()));
        state.write().await.available = true; // simulate a prior successful check
        let (tx, _rx) = tokio::sync::broadcast::channel(4);

        check_once(&state, &source, &tx).await;

        let s = state.read().await;
        assert!(
            s.available,
            "transient fetch error should not reset availability"
        );
        assert!(s.error.is_some());
    }

    #[tokio::test]
    async fn check_once_skips_while_updating() {
        let url = mock_release_server("v99.0.0", "notes").await;
        let source = GitHubVersionSource::with_url(url);
        let state: SharedUpdateState = Arc::new(RwLock::new(UpdateState::new_idle()));
        state.write().await.status = UpdateStatus::Updating;
        let (tx, mut rx) = tokio::sync::broadcast::channel(4);

        check_once(&state, &source, &tx).await;

        let s = state.read().await;
        assert!(!s.available, "should not have checked while updating");
        assert!(rx.try_recv().is_err());
    }
}
