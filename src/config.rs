use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const DEFAULT_MAX_TOKENS: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecallSource {
    Project,
    Local,
}

#[derive(Debug, Clone)]
pub struct Recall {
    pub query: String,
    pub source: RecallSource,
}

#[derive(Debug, Clone)]
pub struct MyndConfig {
    pub project_name: String,
    pub max_tokens: usize,
    pub recalls: Vec<Recall>,
    pub file_open_rule_count: usize,
    pub mention_trigger_count: usize,
}

#[derive(Debug, Default, Deserialize)]
struct RawProject {
    #[serde(default)]
    project: RawProjectMeta,
    #[serde(default)]
    hooks: RawHooks,
}

#[derive(Debug, Default, Deserialize)]
struct RawProjectMeta {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Default, Deserialize)]
struct RawHooks {
    #[serde(default)]
    on_session_start: RawSessionStart,
    #[serde(default)]
    on_file_open: RawFileOpen,
    #[serde(default)]
    on_mention: RawMention,
}

#[derive(Debug, Default, Deserialize)]
struct RawSessionStart {
    max_tokens: Option<usize>,
    #[serde(default)]
    recalls: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawFileOpen {
    #[serde(default)]
    rules: Vec<toml::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct RawMention {
    #[serde(default)]
    triggers: Vec<toml::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct RawLocal {
    #[serde(default)]
    hooks: RawLocalHooks,
}

#[derive(Debug, Default, Deserialize)]
struct RawLocalHooks {
    #[serde(default)]
    on_session_start: RawLocalSessionStart,
}

#[derive(Debug, Default, Deserialize)]
struct RawLocalSessionStart {
    max_tokens: Option<usize>,
    #[serde(default)]
    recalls: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawServer {
    host: Option<String>,
    port: Option<u16>,
}

#[derive(Debug, Default, Deserialize)]
struct RawDashboard {
    port: Option<u16>,
    api_url: Option<String>,
    /// The origin the browser sends when loading the dashboard.
    /// Set this when the dashboard is accessed via a hostname other than
    /// 127.0.0.1 / localhost (e.g. `http://pi.local:3457`).
    cors_origin: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawSync {
    enabled: Option<bool>,
    remote_url: Option<String>,
    api_key: Option<String>,
    interval_seconds: Option<u64>,
    sync_on_store: Option<bool>,
    sync_on_startup: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct RawUpdate {
    enabled: Option<bool>,
    check_interval_seconds: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct RawAgent {
    command: Option<String>,
    args: Option<Vec<String>>,
    kind: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawTags {
    guard_predefined_namespaces: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct RawMatrix {
    homeserver_url: Option<String>,
    user_id: Option<String>,
    #[serde(default)]
    allowed_users: Vec<String>,
    #[serde(default)]
    rooms: Vec<RawMatrixRoom>,
    session_ttl_seconds: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct RawMatrixRoom {
    room_id: Option<String>,
    alias: Option<String>,
    #[serde(default)]
    base_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncSettings {
    pub enabled: bool,
    pub remote_url: String,
    pub api_key: String,
    pub interval_seconds: u64,
    pub sync_on_store: bool,
    pub sync_on_startup: bool,
}

impl Default for SyncSettings {
    fn default() -> Self {
        SyncSettings {
            enabled: false,
            remote_url: String::new(),
            api_key: String::new(),
            interval_seconds: 300,
            sync_on_store: true,
            sync_on_startup: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateSettings {
    pub enabled: bool,
    pub check_interval_seconds: u64,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        UpdateSettings {
            enabled: true,
            check_interval_seconds: 600,
        }
    }
}

/// Which headless-agent CLI `mynd suggest` / the Matrix bot shell out to.
/// Explicit rather than sniffed from `command`'s file name, so a wrapper
/// script or a renamed binary doesn't silently change which flags get used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    OpenCode,
}

impl AgentKind {
    /// Back-compat default for configs written before `[agent] kind` existed:
    /// guess from the command's file stem.
    fn detect(command: &str) -> Self {
        let name = std::path::Path::new(command)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(command);
        if name.eq_ignore_ascii_case("opencode") {
            AgentKind::OpenCode
        } else {
            AgentKind::Claude
        }
    }

    fn parse(kind: &str) -> anyhow::Result<Self> {
        match kind {
            "claude" => Ok(AgentKind::Claude),
            "opencode" => Ok(AgentKind::OpenCode),
            other => {
                anyhow::bail!(
                    "unknown [agent] kind \"{other}\" (expected \"claude\" or \"opencode\")"
                )
            }
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::OpenCode => "opencode",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSettings {
    pub command: String,
    pub args: Vec<String>,
    pub kind: AgentKind,
}

impl Default for AgentSettings {
    fn default() -> Self {
        AgentSettings {
            command: "claude".into(),
            args: Vec::new(),
            kind: AgentKind::Claude,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixRoomMapping {
    pub room_id: String,
    pub alias: Option<String>,
    pub base_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixSettings {
    pub homeserver_url: String,
    pub user_id: String,
    pub allowed_users: Vec<String>,
    pub rooms: Vec<MatrixRoomMapping>,
    /// How long (in seconds) a room's agent session stays resumable after
    /// its last message before the next message starts a fresh one.
    pub session_ttl_seconds: u64,
}

pub const DEFAULT_SESSION_TTL_SECONDS: u64 = 120;

#[derive(Debug, Default, Deserialize)]
struct RawGlobal {
    #[serde(default)]
    defaults: RawDefaults,
    #[serde(default)]
    server: RawServer,
    #[serde(default)]
    dashboard: RawDashboard,
    #[serde(default)]
    sync: RawSync,
    #[serde(default)]
    org_sync: RawSync,
    #[serde(default)]
    update: RawUpdate,
    #[serde(default)]
    agent: RawAgent,
    #[serde(default)]
    matrix: RawMatrix,
    #[serde(default)]
    tags: RawTags,
}

#[derive(Debug, Default, Deserialize)]
struct RawDefaults {
    max_inject_tokens: Option<usize>,
}

/// Project config filename, current then legacy. `discover_project_root` and
/// `load_config_with_global` accept either; a not-yet-migrated repo keeps
/// working with `.hivemind.toml`.
pub const PROJECT_CONFIG_NAMES: [&str; 2] = [".mynd.toml", ".hivemind.toml"];
pub const PROJECT_LOCAL_CONFIG_NAMES: [&str; 2] = [".mynd.local.toml", ".hivemind.local.toml"];

/// Path to the project config in `root`, preferring the current name and
/// falling back to the legacy one. Returns the current-name path when neither
/// exists (callers surface that as "no config found").
pub fn project_config_file(root: &Path) -> PathBuf {
    PROJECT_CONFIG_NAMES
        .iter()
        .map(|n| root.join(n))
        .find(|p| p.is_file())
        .unwrap_or_else(|| root.join(PROJECT_CONFIG_NAMES[0]))
}

fn project_local_config_file(root: &Path) -> Option<PathBuf> {
    PROJECT_LOCAL_CONFIG_NAMES
        .iter()
        .map(|n| root.join(n))
        .find(|p| p.is_file())
}

pub fn discover_project_root(start: &Path) -> Option<PathBuf> {
    let start = start.canonicalize().ok()?;
    let mut dir: &Path = &start;
    loop {
        if PROJECT_CONFIG_NAMES.iter().any(|n| dir.join(n).is_file()) {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

fn config_base() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg);
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
}

pub fn global_config_dir() -> PathBuf {
    config_base().join("mynd")
}

/// Pre-rename global config dir (`hivemind` instead of `mynd`); source for the
/// one-time directory relocation in [`crate::dir_migrate`].
pub fn legacy_global_config_dir() -> PathBuf {
    config_base().join("hivemind")
}

pub fn global_config_path() -> PathBuf {
    global_config_dir().join("config.toml")
}

pub fn load_config(project_path: &Path) -> Result<MyndConfig> {
    let root = discover_project_root(project_path).ok_or_else(|| {
        anyhow::anyhow!(
            "no {} found at or above {}",
            PROJECT_CONFIG_NAMES[0],
            project_path.display()
        )
    })?;
    load_config_with_global(&root, &global_config_path())
}

pub fn load_config_with_global(project_root: &Path, global_path: &Path) -> Result<MyndConfig> {
    let global_default = if global_path.is_file() {
        let raw: RawGlobal = toml::from_str(&std::fs::read_to_string(global_path)?)
            .with_context(|| format!("parsing {}", global_path.display()))?;
        raw.defaults.max_inject_tokens
    } else {
        None
    };

    let project_file = project_config_file(project_root);
    let raw_project: RawProject = toml::from_str(
        &std::fs::read_to_string(&project_file)
            .with_context(|| format!("reading {}", project_file.display()))?,
    )
    .with_context(|| format!("parsing {}", project_file.display()))?;

    let base_max = raw_project
        .hooks
        .on_session_start
        .max_tokens
        .or(global_default)
        .unwrap_or(DEFAULT_MAX_TOKENS);

    let mut recalls: Vec<Recall> = raw_project
        .hooks
        .on_session_start
        .recalls
        .iter()
        .map(|q| Recall {
            query: q.clone(),
            source: RecallSource::Project,
        })
        .collect();

    let mut max_tokens = base_max;
    if let Some(local_file) = project_local_config_file(project_root) {
        let raw_local: RawLocal = toml::from_str(&std::fs::read_to_string(&local_file)?)
            .with_context(|| format!("parsing {}", local_file.display()))?;
        max_tokens =
            max_tokens.saturating_add(raw_local.hooks.on_session_start.max_tokens.unwrap_or(0));
        for q in &raw_local.hooks.on_session_start.recalls {
            recalls.push(Recall {
                query: q.clone(),
                source: RecallSource::Local,
            });
        }
    }

    let project_name = if raw_project.project.name.is_empty() {
        project_root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".to_string())
    } else {
        raw_project.project.name
    };

    Ok(MyndConfig {
        project_name,
        max_tokens,
        recalls,
        file_open_rule_count: raw_project.hooks.on_file_open.rules.len(),
        mention_trigger_count: raw_project.hooks.on_mention.triggers.len(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
    pub dashboard_port: u16,
    /// API base URL the dashboard frontend should call.
    pub api_url: String,
    /// Origin allowed in CORS — what the browser sends as the `Origin` header
    /// when the dashboard page makes requests to the API. Defaults to
    /// `http://127.0.0.1:<dashboard_port>` so both `127.0.0.1` and `localhost`
    /// variants work out of the box. Override when the dashboard is accessed
    /// via a custom hostname (e.g. `http://pi.local:3457`).
    pub cors_origin: String,
    pub sync: SyncSettings,
    /// The org-layer sync connection, if `[org_sync]` is configured and
    /// enabled with a non-empty `remote_url`. `None` means the org layer is
    /// unavailable — every org-layer code path must degrade cleanly on `None`.
    pub org_sync: Option<SyncSettings>,
    pub update: UpdateSettings,
    pub agent: AgentSettings,
    /// Whether predefined tag namespaces (project, topic, status, lang, kind,
    /// scope, part) can be deleted or modified via the dashboard/API. On by
    /// default; set `[tags] guard_predefined_namespaces = false` in the
    /// global config to disable.
    pub guard_predefined_namespaces: bool,
}

pub fn load_server_settings(global_path: &std::path::Path) -> anyhow::Result<ServerSettings> {
    let raw: RawGlobal = if global_path.is_file() {
        toml::from_str(&std::fs::read_to_string(global_path)?)
            .with_context(|| format!("parsing {}", global_path.display()))?
    } else {
        RawGlobal::default()
    };
    let host = raw.server.host.unwrap_or_else(|| "127.0.0.1".to_string());
    let port = raw.server.port.unwrap_or(3456);
    let dashboard_port = raw.dashboard.port.unwrap_or(3457);
    let api_url = raw
        .dashboard
        .api_url
        .unwrap_or_else(|| format!("http://{host}:{port}"));
    // For the CORS origin default, wildcard bind addresses (0.0.0.0 / ::) are
    // not valid browser origins, so fall back to the loopback address.
    let cors_host = match host.as_str() {
        "0.0.0.0" | "::" => "127.0.0.1",
        h => h,
    };
    let cors_origin = raw
        .dashboard
        .cors_origin
        .unwrap_or_else(|| format!("http://{cors_host}:{dashboard_port}"));
    let sync = SyncSettings {
        enabled: raw.sync.enabled.unwrap_or(false),
        remote_url: raw.sync.remote_url.unwrap_or_default(),
        api_key: raw.sync.api_key.unwrap_or_default(),
        interval_seconds: raw.sync.interval_seconds.unwrap_or(300),
        sync_on_store: raw.sync.sync_on_store.unwrap_or(true),
        sync_on_startup: raw.sync.sync_on_startup.unwrap_or(true),
    };
    let org_sync = {
        let enabled = raw.org_sync.enabled.unwrap_or(false);
        let remote_url = raw.org_sync.remote_url.unwrap_or_default();
        if enabled && !remote_url.is_empty() {
            Some(SyncSettings {
                enabled: true,
                remote_url,
                api_key: raw.org_sync.api_key.unwrap_or_default(),
                interval_seconds: raw.org_sync.interval_seconds.unwrap_or(300),
                sync_on_store: raw.org_sync.sync_on_store.unwrap_or(true),
                sync_on_startup: raw.org_sync.sync_on_startup.unwrap_or(true),
            })
        } else {
            None
        }
    };
    let update = UpdateSettings {
        enabled: raw.update.enabled.unwrap_or(true),
        check_interval_seconds: raw.update.check_interval_seconds.unwrap_or(600),
    };
    let agent_command = raw.agent.command.unwrap_or_else(|| "claude".into());
    let agent_kind = match raw.agent.kind {
        Some(k) => AgentKind::parse(&k)?,
        None => AgentKind::detect(&agent_command),
    };
    let agent = AgentSettings {
        command: agent_command,
        args: raw.agent.args.unwrap_or_default(),
        kind: agent_kind,
    };
    let guard_predefined_namespaces = raw.tags.guard_predefined_namespaces.unwrap_or(true);
    Ok(ServerSettings {
        host,
        port,
        dashboard_port,
        api_url,
        cors_origin,
        sync,
        org_sync,
        update,
        agent,
        guard_predefined_namespaces,
    })
}

pub fn load_matrix_settings(global_path: &Path) -> Result<Option<MatrixSettings>> {
    if !global_path.is_file() {
        return Ok(None);
    }
    let raw: RawGlobal = toml::from_str(&std::fs::read_to_string(global_path)?)
        .with_context(|| format!("parsing {}", global_path.display()))?;
    if raw.matrix.homeserver_url.is_none() && raw.matrix.user_id.is_none() {
        return Ok(None);
    }
    let homeserver_url = raw
        .matrix
        .homeserver_url
        .ok_or_else(|| anyhow::anyhow!("[matrix] is present but homeserver_url is missing"))?;
    let user_id = raw
        .matrix
        .user_id
        .ok_or_else(|| anyhow::anyhow!("[matrix] is present but user_id is missing"))?;
    let rooms = raw
        .matrix
        .rooms
        .into_iter()
        .filter_map(|r| {
            r.room_id.map(|room_id| MatrixRoomMapping {
                room_id,
                alias: r.alias,
                base_tags: r.base_tags,
            })
        })
        .collect();
    Ok(Some(MatrixSettings {
        homeserver_url,
        user_id,
        allowed_users: raw.matrix.allowed_users,
        rooms,
        session_ttl_seconds: raw
            .matrix
            .session_ttl_seconds
            .unwrap_or(DEFAULT_SESSION_TTL_SECONDS),
    }))
}

/// Writes `homeserver_url`/`user_id` into the global config's `[matrix]` table,
/// preserving every other section and any existing `allowed_users`/`[[matrix.rooms]]`.
/// Used by `mynd matrix login` after a successful login.
pub fn write_matrix_login(global_path: &Path, homeserver_url: &str, user_id: &str) -> Result<()> {
    let mut doc: toml::Value = if global_path.is_file() {
        toml::from_str(&std::fs::read_to_string(global_path)?)
            .with_context(|| format!("parsing {}", global_path.display()))?
    } else {
        toml::Value::Table(toml::map::Map::new())
    };
    let table = doc
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("global config root is not a table"))?;
    let matrix = table
        .entry("matrix")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let matrix_table = matrix
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[matrix] is not a table"))?;
    matrix_table.insert(
        "homeserver_url".to_string(),
        toml::Value::String(homeserver_url.to_string()),
    );
    matrix_table.insert(
        "user_id".to_string(),
        toml::Value::String(user_id.to_string()),
    );
    if let Some(dir) = global_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(global_path, toml::to_string_pretty(&doc)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(dir: &std::path::Path, name: &str, body: &str) {
        fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn discover_walks_up_to_find_legacy_hivemind_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, ".hivemind.toml", "[project]\nname=\"x\"\n");
        let nested = root.join("internal").join("svc");
        fs::create_dir_all(&nested).unwrap();
        let found = discover_project_root(&nested).unwrap();
        assert_eq!(found, root.canonicalize().unwrap());
    }

    #[test]
    fn discover_walks_up_to_find_mynd_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, ".mynd.toml", "[project]\nname=\"x\"\n");
        let nested = root.join("internal").join("svc");
        fs::create_dir_all(&nested).unwrap();
        let found = discover_project_root(&nested).unwrap();
        assert_eq!(found, root.canonicalize().unwrap());
    }

    #[test]
    fn discover_returns_none_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(discover_project_root(tmp.path()).is_none());
    }

    #[test]
    fn load_reads_mynd_toml_and_mynd_local_toml() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            ".mynd.toml",
            "[project]\nname=\"p\"\n[hooks.on_session_start]\nmax_tokens=2000\nrecalls=[\"team\"]\n",
        );
        write(
            tmp.path(),
            ".mynd.local.toml",
            "[hooks.on_session_start]\nmax_tokens=500\nrecalls=[\"mine\"]\n",
        );
        let missing_global = tmp.path().join("no-global.toml");
        let cfg = load_config_with_global(tmp.path(), &missing_global).unwrap();
        assert_eq!(cfg.project_name, "p");
        assert_eq!(cfg.max_tokens, 2500);
        assert_eq!(cfg.recalls.len(), 2);
        assert_eq!(cfg.recalls[1].query, "mine");
    }

    #[test]
    fn load_prefers_mynd_toml_over_hivemind_toml_when_both_present() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), ".hivemind.toml", "[project]\nname=\"old\"\n");
        write(tmp.path(), ".mynd.toml", "[project]\nname=\"new\"\n");
        let missing_global = tmp.path().join("no-global.toml");
        let cfg = load_config_with_global(tmp.path(), &missing_global).unwrap();
        assert_eq!(cfg.project_name, "new");
    }

    #[test]
    fn load_uses_project_name_recalls_and_max_tokens() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            ".hivemind.toml",
            "[project]\nname=\"oxhive-api\"\n[hooks.on_session_start]\nmax_tokens=1500\nrecalls=[\"a\",\"b\"]\n",
        );
        let missing_global = tmp.path().join("no-global.toml");
        let cfg = load_config_with_global(tmp.path(), &missing_global).unwrap();
        assert_eq!(cfg.project_name, "oxhive-api");
        assert_eq!(cfg.max_tokens, 1500);
        assert_eq!(cfg.recalls.len(), 2);
        assert_eq!(cfg.recalls[0].query, "a");
        assert!(matches!(cfg.recalls[0].source, RecallSource::Project));
    }

    #[test]
    fn local_config_is_additive() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            ".hivemind.toml",
            "[project]\nname=\"p\"\n[hooks.on_session_start]\nmax_tokens=2000\nrecalls=[\"team\"]\n",
        );
        write(
            tmp.path(),
            ".hivemind.local.toml",
            "[hooks.on_session_start]\nmax_tokens=500\nrecalls=[\"mine\"]\n",
        );
        let missing_global = tmp.path().join("no-global.toml");
        let cfg = load_config_with_global(tmp.path(), &missing_global).unwrap();
        assert_eq!(cfg.max_tokens, 2500, "local max_tokens adds to team budget");
        assert_eq!(cfg.recalls.len(), 2);
        assert_eq!(cfg.recalls[1].query, "mine");
        assert!(matches!(cfg.recalls[1].source, RecallSource::Local));
    }

    #[test]
    fn default_max_tokens_is_2000_when_unset() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), ".hivemind.toml", "[project]\nname=\"p\"\n");
        let missing_global = tmp.path().join("no-global.toml");
        let cfg = load_config_with_global(tmp.path(), &missing_global).unwrap();
        assert_eq!(cfg.max_tokens, 2000);
        assert_eq!(cfg.recalls.len(), 0);
    }

    #[test]
    fn counts_file_open_and_mention_rules() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            ".hivemind.toml",
            "[project]\nname=\"p\"\n\
             [hooks.on_file_open]\nrules=[{pattern=\"*.go\",recall=\"x\"},{pattern=\"*.rs\",recall=\"y\"}]\n\
             [hooks.on_mention]\ntriggers=[{keyword=\"@db\",recall=\"z\"}]\n",
        );
        let missing_global = tmp.path().join("no-global.toml");
        let cfg = load_config_with_global(tmp.path(), &missing_global).unwrap();
        assert_eq!(cfg.file_open_rule_count, 2);
        assert_eq!(cfg.mention_trigger_count, 1);
    }

    #[test]
    fn server_settings_defaults_when_global_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let s = load_server_settings(&tmp.path().join("no-global.toml")).unwrap();
        assert_eq!(s.host, "127.0.0.1");
        assert_eq!(s.port, 3456);
        assert_eq!(s.dashboard_port, 3457);
        assert_eq!(s.api_url, "http://127.0.0.1:3456");
        assert_eq!(s.cors_origin, "http://127.0.0.1:3457");
    }

    #[test]
    fn server_settings_reads_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "config.toml",
            "[server]\nhost=\"0.0.0.0\"\nport=4000\n[dashboard]\nport=4001\napi_url=\"http://pi.local:4000\"\n",
        );
        let s = load_server_settings(&tmp.path().join("config.toml")).unwrap();
        assert_eq!(s.host, "0.0.0.0");
        assert_eq!(s.port, 4000);
        assert_eq!(s.dashboard_port, 4001);
        assert_eq!(s.api_url, "http://pi.local:4000");
        // wildcard bind → CORS default falls back to loopback
        assert_eq!(s.cors_origin, "http://127.0.0.1:4001");
    }

    #[test]
    fn server_settings_cors_origin_explicit_override() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "config.toml",
            "[server]\nhost=\"0.0.0.0\"\nport=4000\n[dashboard]\nport=4001\napi_url=\"http://pi.local:4000\"\ncors_origin=\"http://pi.local:4001\"\n",
        );
        let s = load_server_settings(&tmp.path().join("config.toml")).unwrap();
        assert_eq!(s.cors_origin, "http://pi.local:4001");
    }

    #[test]
    fn sync_settings_defaults_when_global_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let s = load_server_settings(&tmp.path().join("no-global.toml")).unwrap();
        assert!(!s.sync.enabled);
        assert!(s.sync.remote_url.is_empty());
        assert_eq!(s.sync.interval_seconds, 300);
        assert!(s.sync.sync_on_store);
        assert!(s.sync.sync_on_startup);
    }

    #[test]
    fn sync_settings_reads_from_global_config() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "config.toml",
            "[sync]\nenabled=true\nremote_url=\"http://pi.local:3456\"\napi_key=\"secret\"\ninterval_seconds=60\nsync_on_store=false\n",
        );
        let s = load_server_settings(&tmp.path().join("config.toml")).unwrap();
        assert!(s.sync.enabled);
        assert_eq!(s.sync.remote_url, "http://pi.local:3456");
        assert_eq!(s.sync.api_key, "secret");
        assert_eq!(s.sync.interval_seconds, 60);
        assert!(!s.sync.sync_on_store);
    }

    #[test]
    fn org_sync_absent_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        let s = load_server_settings(&tmp.path().join("no-global.toml")).unwrap();
        assert_eq!(s.org_sync, None);
    }

    #[test]
    fn org_sync_none_when_present_but_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        let global = tmp.path().join("config.toml");
        std::fs::write(
            &global,
            "[org_sync]\nenabled = false\nremote_url = \"https://gateway.oxhive.dev\"\napi_key = \"x\"\n",
        )
        .unwrap();
        let s = load_server_settings(&global).unwrap();
        assert_eq!(s.org_sync, None);
    }

    #[test]
    fn org_sync_some_when_enabled_with_remote_url() {
        let tmp = tempfile::tempdir().unwrap();
        let global = tmp.path().join("config.toml");
        std::fs::write(
            &global,
            "[org_sync]\nenabled = true\nremote_url = \"https://gateway.oxhive.dev\"\napi_key = \"hm_org_x\"\ninterval_seconds = 60\n",
        )
        .unwrap();
        let s = load_server_settings(&global).unwrap();
        let org = s.org_sync.expect("org_sync should be Some");
        assert_eq!(org.remote_url, "https://gateway.oxhive.dev");
        assert_eq!(org.api_key, "hm_org_x");
        assert_eq!(org.interval_seconds, 60);
    }

    #[test]
    fn org_sync_none_when_enabled_but_remote_url_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let global = tmp.path().join("config.toml");
        std::fs::write(&global, "[org_sync]\nenabled = true\nremote_url = \"\"\n").unwrap();
        let s = load_server_settings(&global).unwrap();
        assert_eq!(s.org_sync, None);
    }

    #[test]
    fn update_settings_defaults_when_global_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let s = load_server_settings(&tmp.path().join("no-global.toml")).unwrap();
        assert!(s.update.enabled);
        assert_eq!(s.update.check_interval_seconds, 600);
    }

    #[test]
    fn update_settings_reads_from_global_config() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "config.toml",
            "[update]\nenabled=false\ncheck_interval_seconds=120\n",
        );
        let s = load_server_settings(&tmp.path().join("config.toml")).unwrap();
        assert!(!s.update.enabled);
        assert_eq!(s.update.check_interval_seconds, 120);
    }

    #[test]
    fn guard_predefined_namespaces_defaults_to_true_when_global_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let s = load_server_settings(&tmp.path().join("no-global.toml")).unwrap();
        assert!(s.guard_predefined_namespaces);
    }

    #[test]
    fn guard_predefined_namespaces_can_be_disabled_via_global_config() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "config.toml",
            "[tags]\nguard_predefined_namespaces=false\n",
        );
        let s = load_server_settings(&tmp.path().join("config.toml")).unwrap();
        assert!(!s.guard_predefined_namespaces);
    }

    #[test]
    fn load_config_errors_when_no_hivemind_toml_found() {
        let tmp = tempfile::tempdir().unwrap();
        let err = load_config(tmp.path());
        assert!(err.is_err(), "should error when no .hivemind.toml exists");
    }

    #[test]
    fn load_config_succeeds_with_project_toml_present() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            ".hivemind.toml",
            "[project]\nname=\"myproject\"\n",
        );
        let cfg = load_config(tmp.path()).unwrap();
        assert_eq!(cfg.project_name, "myproject");
    }

    #[test]
    fn global_config_dir_uses_xdg_when_set() {
        let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        // SAFETY: test-only env mutation; serialised by ENV_MUTEX.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        }
        let dir = global_config_dir();
        let legacy = legacy_global_config_dir();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        assert_eq!(dir, tmp.path().join("mynd"));
        assert_eq!(legacy, tmp.path().join("hivemind"));
    }

    #[test]
    fn global_config_path_ends_with_config_toml() {
        let path = global_config_path();
        assert_eq!(path.file_name().unwrap(), "config.toml");
    }

    #[test]
    fn project_name_falls_back_to_directory_name_when_empty() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), ".hivemind.toml", "[project]\n");
        let missing_global = tmp.path().join("no-global.toml");
        let cfg = load_config_with_global(tmp.path(), &missing_global).unwrap();
        let dir_name = tmp.path().file_name().unwrap().to_string_lossy();
        assert_eq!(cfg.project_name, dir_name.as_ref());
    }

    #[test]
    fn load_config_with_global_reads_max_inject_tokens_from_global_file() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            ".hivemind.toml",
            "[project]\nname=\"test\"\n[hooks.on_session_start]\nrecalls=[]\n",
        );
        let global_dir = tempfile::tempdir().unwrap();
        let global_path = global_dir.path().join("config.toml");
        fs::write(&global_path, "[defaults]\nmax_inject_tokens=4000\n").unwrap();
        let cfg = load_config_with_global(tmp.path(), &global_path).unwrap();
        assert_eq!(cfg.max_tokens, 4000);
    }

    #[test]
    fn agent_settings_default_to_claude() {
        let tmp = tempfile::tempdir().unwrap();
        let s = load_server_settings(&tmp.path().join("no-global.toml")).unwrap();
        assert_eq!(s.agent.command, "claude");
        assert!(s.agent.args.is_empty());
        assert_eq!(s.agent.kind, AgentKind::Claude);
    }

    #[test]
    fn agent_kind_explicit_overrides_command_sniffing() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "config.toml",
            "[agent]\ncommand=\"/opt/bin/my-claude-wrapper\"\nkind=\"claude\"\n",
        );
        let s = load_server_settings(&tmp.path().join("config.toml")).unwrap();
        assert_eq!(s.agent.kind, AgentKind::Claude);
    }

    #[test]
    fn agent_kind_defaults_to_opencode_when_command_looks_like_it() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "config.toml", "[agent]\ncommand=\"opencode\"\n");
        let s = load_server_settings(&tmp.path().join("config.toml")).unwrap();
        assert_eq!(s.agent.kind, AgentKind::OpenCode);
    }

    #[test]
    fn agent_kind_rejects_unknown_value() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "config.toml", "[agent]\nkind=\"gpt\"\n");
        let err = load_server_settings(&tmp.path().join("config.toml")).unwrap_err();
        assert!(err.to_string().contains("unknown [agent] kind"));
    }

    #[test]
    fn agent_settings_read_from_global_config() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "config.toml",
            "[agent]\ncommand=\"/usr/local/bin/claude\"\nargs=[\"--model\",\"opus\"]\n",
        );
        let s = load_server_settings(&tmp.path().join("config.toml")).unwrap();
        assert_eq!(s.agent.command, "/usr/local/bin/claude");
        assert_eq!(s.agent.args, vec!["--model", "opus"]);
    }

    #[test]
    fn matrix_settings_none_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let s = load_matrix_settings(&tmp.path().join("no-global.toml")).unwrap();
        assert!(s.is_none());
    }

    #[test]
    fn matrix_settings_parses_full_config() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "config.toml",
            "[matrix]\n\
             homeserver_url=\"https://matrix.org\"\n\
             user_id=\"@mynd-bot:matrix.org\"\n\
             allowed_users=[\"@you:matrix.org\"]\n\
             \n\
             [[matrix.rooms]]\n\
             room_id=\"!abc123:matrix.org\"\n\
             alias=\"mynd-project\"\n\
             base_tags=[\"project:hivemind\"]\n",
        );
        let s = load_matrix_settings(&tmp.path().join("config.toml"))
            .unwrap()
            .expect("matrix settings should be present");
        assert_eq!(s.homeserver_url, "https://matrix.org");
        assert_eq!(s.user_id, "@mynd-bot:matrix.org");
        assert_eq!(s.allowed_users, vec!["@you:matrix.org".to_string()]);
        assert_eq!(s.rooms.len(), 1);
        assert_eq!(s.rooms[0].room_id, "!abc123:matrix.org");
        assert_eq!(s.rooms[0].alias.as_deref(), Some("mynd-project"));
        assert_eq!(s.rooms[0].base_tags, vec!["project:hivemind".to_string()]);
        assert_eq!(s.session_ttl_seconds, DEFAULT_SESSION_TTL_SECONDS);
    }

    #[test]
    fn matrix_settings_defaults_allowed_users_and_rooms_to_empty() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "config.toml",
            "[matrix]\nhomeserver_url=\"https://matrix.org\"\nuser_id=\"@bot:matrix.org\"\n",
        );
        let s = load_matrix_settings(&tmp.path().join("config.toml"))
            .unwrap()
            .unwrap();
        assert!(s.allowed_users.is_empty());
        assert!(s.rooms.is_empty());
        assert_eq!(s.session_ttl_seconds, DEFAULT_SESSION_TTL_SECONDS);
    }

    #[test]
    fn matrix_settings_honors_configured_session_ttl() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "config.toml",
            "[matrix]\nhomeserver_url=\"https://matrix.org\"\nuser_id=\"@bot:matrix.org\"\n\
             session_ttl_seconds=30\n",
        );
        let s = load_matrix_settings(&tmp.path().join("config.toml"))
            .unwrap()
            .unwrap();
        assert_eq!(s.session_ttl_seconds, 30);
    }

    #[test]
    fn matrix_settings_errors_when_homeserver_url_missing() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "config.toml",
            "[matrix]\nuser_id=\"@bot:matrix.org\"\n",
        );
        let err = load_matrix_settings(&tmp.path().join("config.toml")).unwrap_err();
        assert!(err.to_string().contains("homeserver_url"));
    }

    #[test]
    fn write_matrix_login_creates_new_matrix_section() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        write_matrix_login(&path, "https://matrix.org", "@bot:matrix.org").unwrap();
        let s = load_matrix_settings(&path).unwrap().unwrap();
        assert_eq!(s.homeserver_url, "https://matrix.org");
        assert_eq!(s.user_id, "@bot:matrix.org");
    }

    #[test]
    fn write_matrix_login_preserves_other_sections_and_room_mappings() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        write(
            tmp.path(),
            "config.toml",
            "[defaults]\nmax_inject_tokens=1500\n\
             [matrix]\nhomeserver_url=\"https://old.example\"\nuser_id=\"@old:example\"\n\
             allowed_users=[\"@you:matrix.org\"]\n\
             [[matrix.rooms]]\nroom_id=\"!abc:matrix.org\"\nbase_tags=[\"project:hivemind\"]\n",
        );
        write_matrix_login(&path, "https://matrix.org", "@bot:matrix.org").unwrap();
        let s = load_matrix_settings(&path).unwrap().unwrap();
        assert_eq!(s.homeserver_url, "https://matrix.org");
        assert_eq!(s.user_id, "@bot:matrix.org");
        assert_eq!(
            s.allowed_users,
            vec!["@you:matrix.org".to_string()],
            "room mappings/allowlist survive a re-login"
        );
        assert_eq!(s.rooms.len(), 1);
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(
            raw.contains("max_inject_tokens"),
            "unrelated [defaults] section survives"
        );
    }
}
