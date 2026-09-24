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
    /// How this binary was installed. Only `script` installs can be
    /// upgraded in place; the others are owned by their package manager.
    pub install_method: InstallMethod,
    /// The command to run by hand when `install_method` can't self-upgrade
    /// (`None` for script installs). Serialised so the dashboard can show it
    /// in place of the Update button.
    pub upgrade_command: Option<String>,
    /// Whether `POST /api/v1/update/apply` is allowed at all
    /// (`[update] allow_apply_from_api`). Serialised so the dashboard can
    /// hide the button instead of showing one that 403s.
    pub apply_enabled: bool,
}

impl UpdateState {
    pub fn new_idle() -> Self {
        let install_method = InstallMethod::detect();
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
            upgrade_command: install_method.manual_upgrade_command().map(str::to_string),
            install_method,
            apply_enabled: true,
        }
    }
}

pub type SharedUpdateState = Arc<RwLock<UpdateState>>;

pub struct ReleaseInfo {
    pub version: String,
    pub notes_md: String,
    pub html_url: String,
}

const DEFAULT_CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

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
        GitHubVersionSource::with_timeout(DEFAULT_CHECK_TIMEOUT)
    }

    pub fn with_timeout(timeout: std::time::Duration) -> Self {
        let api_url = std::env::var("MYND_UPDATE_CHECK_URL").unwrap_or_else(|_| {
            "https://api.github.com/repos/oxhive/mynd/releases/latest".to_string()
        });
        GitHubVersionSource::build(api_url, timeout)
    }

    pub fn with_url(api_url: String) -> Self {
        GitHubVersionSource::build(api_url, DEFAULT_CHECK_TIMEOUT)
    }

    fn build(api_url: String, timeout: std::time::Duration) -> Self {
        // A stalled response would otherwise park the check loop forever
        // (and hang `mynd update`): the loop awaits `check_once`.
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        GitHubVersionSource { client, api_url }
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

/// Whether `latest` is a strictly newer semver than this binary. Unparseable
/// versions count as "not newer" so a malformed tag never nags anyone.
pub fn is_newer_than_current(latest: &str) -> bool {
    let current = env!("CARGO_PKG_VERSION");
    match (
        semver::Version::parse(current),
        semver::Version::parse(latest),
    ) {
        (Ok(cur), Ok(latest)) => latest > cur,
        _ => {
            tracing::warn!(
                "could not parse versions for comparison (current={current}, latest={latest})"
            );
            false
        }
    }
}

/// Best-effort check for `mynd status`: returns the latest version only when
/// it's newer than this binary. Any failure (offline, rate-limited, slow
/// network past `timeout`) is swallowed — status must never fail or stall
/// because GitHub is unreachable.
pub async fn newer_version_available(timeout: std::time::Duration) -> Option<String> {
    let release = GitHubVersionSource::with_timeout(timeout)
        .latest()
        .await
        .ok()?;
    is_newer_than_current(&release.version).then_some(release.version)
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

    let is_newer = is_newer_than_current(&release.version);

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

pub(crate) async fn do_update() -> Result<()> {
    // Resolved before the install script runs, not after: the script `mv`s
    // the new binary over this one, which unlinks the running process's
    // original inode. Post-replace, /proc/self/exe (what
    // std::env::current_exe reads) resolves to "<path> (deleted)" — a path
    // that doesn't exist, so exec() on it fails with ENOENT. Resolving here
    // captures the plain path, which still resolves correctly to the new
    // binary once the rename lands.
    let exe = std::env::current_exe().context("resolving current executable path")?;
    let method = InstallMethod::detect_from(&exe);
    let InstallMethod::Script { install_dir } = &method else {
        anyhow::bail!("{}", method.refusal_message());
    };
    run_install_script(install_dir, ScriptOutput::Capture).await?;
    restart(&exe)
}

/// How the running `mynd` binary got onto this machine, which decides
/// whether `mynd upgrade` may replace it or must defer to a package manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallMethod {
    /// `curl -fsSL https://get.oxhive.dev/mynd | sh` — a plain binary in
    /// `$INSTALL_DIR` (default `~/.local/bin`). Also the fallback for any
    /// location we don't recognise, since that's where a custom
    /// `INSTALL_DIR` puts it.
    Script {
        #[serde(skip)]
        install_dir: std::path::PathBuf,
    },
    /// `brew install oxhive/tap/mynd` — lives under a Homebrew `Cellar`.
    Homebrew,
    /// `cargo install` — lives in `$CARGO_HOME/bin`.
    Cargo,
}

pub const INSTALL_SCRIPT_URL: &str = "https://get.oxhive.dev/mynd";

impl InstallMethod {
    pub fn detect() -> Self {
        match std::env::current_exe() {
            Ok(exe) => InstallMethod::detect_from(&exe),
            // Without a path there's nothing to replace; report it as a
            // cargo-style install so nothing tries to self-upgrade.
            Err(_) => InstallMethod::Cargo,
        }
    }

    pub fn detect_from(exe: &std::path::Path) -> Self {
        // Homebrew symlinks `<prefix>/bin/mynd` into the Cellar, so resolve
        // links first; fall back to the raw path if that fails.
        let resolved = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
        let cargo_bin = std::env::var_os("CARGO_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".cargo")))
            .map(|c| c.join("bin"));
        InstallMethod::classify(&resolved, cargo_bin.as_deref())
    }

    /// Pure classification, split out so tests don't depend on the real
    /// filesystem or environment.
    fn classify(exe: &std::path::Path, cargo_bin: Option<&std::path::Path>) -> Self {
        let in_cellar = exe
            .components()
            .any(|c| c.as_os_str() == "Cellar" || c.as_os_str() == ".linuxbrew");
        if in_cellar {
            return InstallMethod::Homebrew;
        }
        if cargo_bin.is_some_and(|bin| exe.parent() == Some(bin)) {
            return InstallMethod::Cargo;
        }
        InstallMethod::Script {
            install_dir: exe
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default(),
        }
    }

    /// The command the user must run themselves, or `None` when `mynd
    /// upgrade` can do it.
    pub fn manual_upgrade_command(&self) -> Option<&'static str> {
        match self {
            InstallMethod::Script { .. } => None,
            InstallMethod::Homebrew => Some("brew update && brew upgrade oxhive/tap/mynd"),
            InstallMethod::Cargo => {
                Some("cargo install --git https://github.com/oxhive/mynd --locked oxmynd")
            }
        }
    }

    /// The next step to tell someone who has an update waiting.
    pub fn upgrade_hint(&self) -> &'static str {
        self.manual_upgrade_command().unwrap_or("mynd upgrade")
    }

    pub fn refusal_message(&self) -> String {
        let (how, cmd) = match self {
            InstallMethod::Script { .. } => return String::new(),
            InstallMethod::Homebrew => ("Homebrew", self.upgrade_hint()),
            InstallMethod::Cargo => ("cargo", self.upgrade_hint()),
        };
        format!("mynd was installed with {how}, so it can't upgrade itself. Upgrade with:\n\n  {cmd}")
    }
}

#[derive(Clone, Copy)]
pub enum ScriptOutput {
    /// Stream the installer's output to this terminal (`mynd upgrade`).
    Inherit,
    /// Collect it and surface stderr in the error (dashboard-triggered
    /// upgrades, where there's no terminal to print to).
    Capture,
}

/// Downloads the install script and runs it with `sh`, pointed at
/// `install_dir` so it replaces this binary in place. The script is fetched
/// with reqwest (not `curl | sh`) so there's no shell quoting of the path and
/// no dependency on curl; `MYND_INSTALL_SCRIPT_URL` overrides the URL for
/// tests and manual E2E runs.
pub async fn run_install_script(install_dir: &std::path::Path, output: ScriptOutput) -> Result<()> {
    use tokio::io::AsyncWriteExt as _;

    let url =
        std::env::var("MYND_INSTALL_SCRIPT_URL").unwrap_or_else(|_| INSTALL_SCRIPT_URL.to_string());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let resp = client
        .get(&url)
        .header("User-Agent", concat!("mynd/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .with_context(|| format!("downloading install script from {url}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("downloading install script returned HTTP {}", resp.status());
    }
    let script = resp.bytes().await.context("reading install script")?;

    let (stdout, stderr) = match output {
        ScriptOutput::Inherit => (std::process::Stdio::inherit(), std::process::Stdio::inherit()),
        ScriptOutput::Capture => (std::process::Stdio::piped(), std::process::Stdio::piped()),
    };
    let mut child = tokio::process::Command::new("sh")
        .env("INSTALL_DIR", install_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(stdout)
        .stderr(stderr)
        .kill_on_drop(true)
        .spawn()
        .context("failed to run sh for the install script")?;
    {
        let mut stdin = child.stdin.take().context("install script stdin unavailable")?;
        stdin
            .write_all(&script)
            .await
            .context("piping install script to sh")?;
    } // drop closes stdin so sh sees EOF and runs to completion
    let out = child
        .wait_with_output()
        .await
        .context("waiting for install script")?;
    if !out.status.success() {
        anyhow::bail!(
            "install script failed (exit {}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
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

    #[test]
    fn classify_detects_homebrew_cellar_paths() {
        for exe in [
            "/opt/homebrew/Cellar/mynd/0.16.0/bin/mynd",
            "/usr/local/Cellar/mynd/0.16.0/bin/mynd",
            "/home/linuxbrew/.linuxbrew/Cellar/mynd/0.16.0/bin/mynd",
        ] {
            assert_eq!(
                InstallMethod::classify(std::path::Path::new(exe), None),
                InstallMethod::Homebrew,
                "{exe}"
            );
        }
    }

    #[test]
    fn classify_detects_cargo_bin() {
        let cargo_bin = std::path::Path::new("/home/u/.cargo/bin");
        assert_eq!(
            InstallMethod::classify(&cargo_bin.join("mynd"), Some(cargo_bin)),
            InstallMethod::Cargo
        );
    }

    #[test]
    fn classify_treats_everything_else_as_script_install() {
        let cargo_bin = std::path::Path::new("/home/u/.cargo/bin");
        for dir in ["/home/u/.local/bin", "/usr/local/bin", "/opt/tools"] {
            let dir = std::path::Path::new(dir);
            assert_eq!(
                InstallMethod::classify(&dir.join("mynd"), Some(cargo_bin)),
                InstallMethod::Script {
                    install_dir: dir.to_path_buf()
                }
            );
        }
    }

    #[test]
    fn only_script_installs_can_self_upgrade() {
        let script = InstallMethod::Script {
            install_dir: "/home/u/.local/bin".into(),
        };
        assert_eq!(script.manual_upgrade_command(), None);
        assert_eq!(script.upgrade_hint(), "mynd upgrade");
        assert!(
            InstallMethod::Homebrew
                .upgrade_hint()
                .contains("brew upgrade oxhive/tap/mynd")
        );
        assert!(InstallMethod::Homebrew.refusal_message().contains("Homebrew"));
        assert!(InstallMethod::Cargo.upgrade_hint().starts_with("cargo install"));
    }

    #[test]
    fn install_script_runs_with_install_dir_and_reports_failure() {
        let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(install_script_runs_with_install_dir_and_reports_failure_inner());
    }

    async fn install_script_runs_with_install_dir_and_reports_failure_inner() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let script = format!(
            "printf '%s' \"$INSTALL_DIR\" > '{}'\nexit 0\n",
            marker.display()
        );
        let app = Router::new().route("/ok", get(move || async move { script }))
            .route("/fail", get(|| async { "echo boom >&2\nexit 3\n" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        // SAFETY: test-only env mutation; serialised by ENV_MUTEX.
        unsafe { std::env::set_var("MYND_INSTALL_SCRIPT_URL", format!("http://{addr}/ok")) };
        let ok = run_install_script(dir.path(), ScriptOutput::Capture).await;
        // SAFETY: as above.
        unsafe { std::env::set_var("MYND_INSTALL_SCRIPT_URL", format!("http://{addr}/fail")) };
        let fail = run_install_script(dir.path(), ScriptOutput::Capture).await;
        // SAFETY: as above.
        unsafe { std::env::remove_var("MYND_INSTALL_SCRIPT_URL") };

        ok.unwrap();
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap(),
            dir.path().display().to_string()
        );
        let err = format!("{:#}", fail.unwrap_err());
        assert!(err.contains("boom"), "{err}");
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
