use crate::config::AgentSettings;
use crate::store::SqliteStore;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, broadcast, mpsc};

const TURN_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug)]
pub enum StartError {
    AlreadyActive,
    Failed(String),
}

#[derive(Debug)]
pub enum ReviseError {
    NotActive,
    UnknownEdge,
}

enum Turn {
    Initial { prompt: String },
    Revise { edge_id: String, feedback: String },
}

struct Inner {
    active: bool,
    session_id: Option<String>,
    phase: &'static str, // "idle" | "suggesting" | "reviewing" | "revising"
    revising_edge_id: Option<String>,
    queued_edge_ids: VecDeque<String>,
    tx: Option<mpsc::UnboundedSender<Turn>>,
    worker: Option<tokio::task::JoinHandle<()>>,
}

impl Default for Inner {
    fn default() -> Self {
        Inner {
            active: false,
            session_id: None,
            phase: "idle",
            revising_edge_id: None,
            queued_edge_ids: VecDeque::new(),
            tx: None,
            worker: None,
        }
    }
}

pub struct SuggestSessionManager {
    store: Arc<SqliteStore>,
    events: broadcast::Sender<Value>,
    agent: AgentSettings,
    mcp_url: String,
    inner: Mutex<Inner>,
}

impl SuggestSessionManager {
    pub fn new(
        store: Arc<SqliteStore>,
        events: broadcast::Sender<Value>,
        agent: AgentSettings,
        mcp_url: String,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            events,
            agent,
            mcp_url,
            inner: Mutex::new(Inner::default()),
        })
    }

    fn emit(&self, state: &str, extra: Value) {
        let mut evt = json!({ "type": "suggest_session", "state": state });
        if let (Some(obj), Some(x)) = (evt.as_object_mut(), extra.as_object()) {
            for (k, v) in x {
                obj.insert(k.clone(), v.clone());
            }
        }
        let _ = self.events.send(evt);
    }

    pub async fn start(self: &Arc<Self>) -> Result<(), StartError> {
        let prompt = crate::server::build_suggest_prompt(&self.store)
            .await
            .map_err(|e| StartError::Failed(e.to_string()))?;
        let mut inner = self.inner.lock().await;
        if inner.active {
            return Err(StartError::AlreadyActive);
        }
        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(Turn::Initial { prompt }).ok();
        inner.active = true;
        inner.phase = "suggesting";
        inner.session_id = None;
        inner.queued_edge_ids.clear();
        inner.revising_edge_id = None;
        inner.tx = Some(tx);
        let mgr = Arc::clone(self);
        inner.worker = Some(tokio::spawn(async move { mgr.worker_loop(rx).await }));
        drop(inner);
        self.emit("started", json!({}));
        Ok(())
    }

    pub async fn revise(&self, edge_id: String, feedback: String) -> Result<(), ReviseError> {
        if self.store.get_edge(&edge_id).await.ok().flatten().is_none() {
            return Err(ReviseError::UnknownEdge);
        }
        let mut inner = self.inner.lock().await;
        if !inner.active {
            return Err(ReviseError::NotActive);
        }
        inner.queued_edge_ids.push_back(edge_id.clone());
        let tx = inner.tx.as_ref().ok_or(ReviseError::NotActive)?;
        tx.send(Turn::Revise { edge_id, feedback })
            .map_err(|_| ReviseError::NotActive)?;
        Ok(())
    }

    pub async fn end(&self) {
        let mut inner = self.inner.lock().await;
        inner.active = false;
        inner.phase = "idle";
        inner.tx = None; // closes the channel; worker exits after the in-flight turn
        if let Some(w) = inner.worker.take() {
            w.abort(); // kill_on_drop(true) on the child reaps any in-flight process
        }
        inner.queued_edge_ids.clear();
        inner.revising_edge_id = None;
        inner.session_id = None;
        drop(inner);
        self.emit("ended", json!({}));
    }

    pub async fn status(&self) -> Value {
        let inner = self.inner.lock().await;
        json!({
            "active": inner.active,
            "phase": inner.phase,
            "revising_edge_id": inner.revising_edge_id,
            "queued_edge_ids": inner.queued_edge_ids.iter().collect::<Vec<_>>(),
        })
    }

    async fn worker_loop(self: Arc<Self>, mut rx: mpsc::UnboundedReceiver<Turn>) {
        while let Some(turn) = rx.recv().await {
            match turn {
                Turn::Initial { prompt } => match self.run_turn(&prompt, None).await {
                    Ok(sid) => {
                        let mut inner = self.inner.lock().await;
                        inner.session_id = Some(sid);
                        inner.phase = "reviewing";
                        drop(inner);
                        self.emit("suggestions_ready", json!({}));
                    }
                    Err(msg) => {
                        self.fail_turn(&msg).await;
                    }
                },
                Turn::Revise { edge_id, feedback } => {
                    let resume = {
                        let mut inner = self.inner.lock().await;
                        if !inner.active {
                            break;
                        }
                        inner.queued_edge_ids.retain(|e| e != &edge_id);
                        inner.revising_edge_id = Some(edge_id.clone());
                        inner.phase = "revising";
                        let queued: Vec<String> = inner.queued_edge_ids.iter().cloned().collect();
                        let resume = inner.session_id.clone();
                        drop(inner);
                        self.emit("revising", json!({ "edge_id": edge_id, "queued": queued }));
                        resume
                    };
                    let prompt = match self.build_revision_prompt(&edge_id, &feedback).await {
                        Ok(p) => p,
                        Err(msg) => {
                            self.fail_turn(&msg).await;
                            continue;
                        }
                    };
                    match self.run_turn(&prompt, resume.as_deref()).await {
                        Ok(sid) => {
                            let mut inner = self.inner.lock().await;
                            inner.session_id = Some(sid);
                            inner.revising_edge_id = None;
                            inner.phase = "reviewing";
                            drop(inner);
                            self.emit("revision_ready", json!({ "edge_id": edge_id }));
                        }
                        Err(msg) => {
                            self.fail_turn(&msg).await;
                        }
                    }
                }
            }
        }
    }

    async fn fail_turn(&self, msg: &str) {
        let mut inner = self.inner.lock().await;
        inner.phase = "reviewing";
        inner.revising_edge_id = None;
        drop(inner);
        self.emit("error", json!({ "message": msg }));
    }

    async fn build_revision_prompt(&self, edge_id: &str, feedback: &str) -> Result<String, String> {
        let edge = self
            .store
            .get_edge(edge_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("edge {edge_id} no longer exists"))?;
        Ok(format!(
            "The user reviewed your suggested connection {edge_id} \
             ({} --[{}]--> {}, reason: {}) and asked for a revision:\n\
             \"{feedback}\"\n\
             Call the memory_update_edge tool with id \"{edge_id}\" and the corrected \
             relationship and/or reason. Do not create a new edge and do not change other edges.",
            edge.source_id,
            edge.relationship,
            edge.target_id,
            edge.reason.as_deref().unwrap_or("none given"),
        ))
    }

    /// Runs one headless agent turn; returns the new session id.
    ///
    /// Dispatches on `agent.kind` — Claude Code and OpenCode take different
    /// flags for prompting, JSON output, and MCP wiring.
    async fn run_turn(&self, prompt: &str, resume: Option<&str>) -> Result<String, String> {
        match self.agent.kind {
            crate::config::AgentKind::OpenCode => self.run_opencode_turn(prompt, resume).await,
            crate::config::AgentKind::Claude => self.run_claude_turn(prompt, resume).await,
        }
    }

    async fn run_claude_turn(&self, prompt: &str, resume: Option<&str>) -> Result<String, String> {
        let mcp_config = json!({
            "mcpServers": { "mynd": { "type": "http", "url": self.mcp_url } }
        })
        .to_string();
        let mut cmd = tokio::process::Command::new(&self.agent.command);
        cmd.args(&self.agent.args);
        if let Some(id) = resume {
            cmd.arg("--resume").arg(id);
        }
        cmd.arg("-p")
            .arg(prompt)
            .arg("--output-format")
            .arg("json")
            .arg("--mcp-config")
            .arg(&mcp_config)
            .arg("--strict-mcp-config")
            .arg("--allowedTools")
            .arg("mcp__mynd__memory_store_edge,mcp__mynd__memory_update_edge,mcp__mynd__memory_get_edges")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let out = Self::spawn_and_wait(cmd, &self.agent.command).await?;
        let v: Value =
            serde_json::from_str(&out).map_err(|e| format!("unparseable agent output: {e}"))?;
        v.get("session_id")
            .and_then(|s| s.as_str())
            .map(String::from)
            .ok_or_else(|| "agent output missing session_id".to_string())
    }

    // OpenCode's `run` has no per-invocation `--mcp-config` flag (MCP servers
    // and per-tool permissions are only configurable via `opencode.json` /
    // `OPENCODE_CONFIG`, not CLI args), so — same tradeoff as
    // `matrix::agent::run_opencode_turn` — this requires the user to have
    // pre-configured an OpenCode agent profile named "mynd-suggest" that
    // wires up the mynd MCP server (this `mcp_url`) and allows only
    // memory_store_edge/memory_update_edge/memory_get_edges.
    async fn run_opencode_turn(
        &self,
        prompt: &str,
        resume: Option<&str>,
    ) -> Result<String, String> {
        let mut cmd = tokio::process::Command::new(&self.agent.command);
        cmd.args(&self.agent.args).arg("run").arg(prompt);
        if let Some(id) = resume {
            cmd.arg("-s").arg(id);
        }
        cmd.arg("--agent")
            .arg("mynd-suggest")
            .arg("--format")
            .arg("json")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let out = Self::spawn_and_wait(cmd, &self.agent.command).await?;
        let v: Value =
            serde_json::from_str(&out).map_err(|e| format!("unparseable agent output: {e}"))?;
        v.get("session_id")
            .and_then(|s| s.as_str())
            .map(String::from)
            .ok_or_else(|| "agent output missing session_id".to_string())
    }

    async fn spawn_and_wait(
        mut cmd: tokio::process::Command,
        command_name: &str,
    ) -> Result<String, String> {
        let child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn {command_name}: {e}"))?;
        let out = tokio::time::timeout(TURN_TIMEOUT, child.wait_with_output())
            .await
            .map_err(|_| format!("agent timed out after {}s", TURN_TIMEOUT.as_secs()))?
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!(
                "agent exited with {}: {}",
                out.status,
                stderr.trim()
            ));
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        // Both CLIs print a single JSON object on success; take the last
        // non-empty line to survive any leading noise on stdout.
        stdout
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| "agent produced no output".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SyncSettings;
    use crate::db;
    use crate::store::NewMemoryRow;
    use std::time::Duration;
    use tempfile::TempDir;

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

    fn write_stub_agent(dir: &std::path::Path) -> String {
        let script = dir.join("stub-agent.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$(dirname \"$0\")/args.log\"\necho '{\"type\":\"result\",\"session_id\":\"stub-1\",\"result\":\"done\",\"is_error\":false}'\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script.to_string_lossy().into_owned()
    }

    async fn test_manager(
        dir: &std::path::Path,
    ) -> (
        Arc<SuggestSessionManager>,
        Arc<SqliteStore>,
        broadcast::Receiver<Value>,
        String,
        TempDir,
    ) {
        let (store, db_dir) = test_store().await;
        store
            .store(&NewMemoryRow {
                id: "mem_a",
                title: "A",
                content: "content a",
                tags: &[],
                token_count: None,
                layer: "workspace",
                memory_type: "project",
            })
            .await
            .unwrap();
        store
            .store(&NewMemoryRow {
                id: "mem_b",
                title: "B",
                content: "content b",
                tags: &[],
                token_count: None,
                layer: "workspace",
                memory_type: "project",
            })
            .await
            .unwrap();
        let script = write_stub_agent(dir);
        let (events, rx) = broadcast::channel::<Value>(32);
        let agent = crate::config::AgentSettings {
            command: script.clone(),
            args: vec![],
            kind: crate::config::AgentKind::Claude,
        };
        let mgr = SuggestSessionManager::new(
            Arc::clone(&store),
            events,
            agent,
            "http://127.0.0.1:3456/mcp".into(),
        );
        (mgr, store, rx, script, db_dir)
    }

    async fn next_state(rx: &mut broadcast::Receiver<Value>) -> String {
        let evt = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("event within 5s")
            .unwrap();
        evt["state"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn start_runs_agent_and_emits_lifecycle_events() {
        let dir = tempfile::tempdir().unwrap();
        let (mgr, _store, mut rx, script, _db_dir) = test_manager(dir.path()).await;
        mgr.start().await.unwrap();
        assert_eq!(next_state(&mut rx).await, "started");
        assert_eq!(next_state(&mut rx).await, "suggestions_ready");
        let log = std::fs::read_to_string(dir.path().join("args.log")).unwrap();
        assert!(log.contains("-p"));
        assert!(log.contains("--output-format json"));
        assert!(log.contains("--mcp-config"));
        assert!(log.contains("--strict-mcp-config"));
        assert!(log.contains("--allowedTools"));
        let _ = script;
    }

    #[tokio::test]
    async fn second_start_while_active_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (mgr, _store, mut rx, _, _db_dir) = test_manager(dir.path()).await;
        mgr.start().await.unwrap();
        assert!(matches!(mgr.start().await, Err(StartError::AlreadyActive)));
        let _ = rx.recv().await;
    }

    #[tokio::test]
    async fn revise_resumes_with_latest_session_id_and_emits_revision_ready() {
        let dir = tempfile::tempdir().unwrap();
        let (mgr, store, mut rx, _, _db_dir) = test_manager(dir.path()).await;
        mgr.start().await.unwrap();
        while next_state(&mut rx).await != "suggestions_ready" {}
        // seed a pending edge directly through the manager's store handle
        let crate::model::EdgeCreate::Created(edge_id) = store
            .create_edge_with_status("mem_a", "mem_b", "sibling", "pending", None, None)
            .await
            .unwrap()
        else {
            panic!("expected EdgeCreate::Created");
        };
        mgr.revise(edge_id.clone(), "make it parent".into())
            .await
            .unwrap();
        assert_eq!(next_state(&mut rx).await, "revising");
        assert_eq!(next_state(&mut rx).await, "revision_ready");
        let log = std::fs::read_to_string(dir.path().join("args.log")).unwrap();
        assert!(
            log.contains("--resume stub-1"),
            "second turn must resume the captured session id"
        );
    }

    #[tokio::test]
    async fn revise_without_active_session_errors() {
        let dir = tempfile::tempdir().unwrap();
        let (mgr, _store, _rx, _, _db_dir) = test_manager(dir.path()).await;
        assert!(matches!(
            mgr.revise("edge_x".into(), "hi".into()).await,
            Err(ReviseError::UnknownEdge) | Err(ReviseError::NotActive)
        ));
    }

    #[tokio::test]
    async fn end_emits_ended_and_allows_new_start() {
        let dir = tempfile::tempdir().unwrap();
        let (mgr, _store, mut rx, _, _db_dir) = test_manager(dir.path()).await;
        mgr.start().await.unwrap();
        while next_state(&mut rx).await != "suggestions_ready" {}
        mgr.end().await;
        assert_eq!(next_state(&mut rx).await, "ended");
        mgr.start().await.unwrap();
        assert_eq!(next_state(&mut rx).await, "started");
    }

    #[tokio::test]
    async fn agent_failure_emits_error_event() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _db_dir) = test_store().await;
        store
            .store(&NewMemoryRow {
                id: "mem_a",
                title: "A",
                content: "content a",
                tags: &[],
                token_count: None,
                layer: "workspace",
                memory_type: "project",
            })
            .await
            .unwrap();
        // failing stub instead of the default one
        let script = dir.path().join("bad-agent.sh");
        std::fs::write(&script, "#!/bin/sh\necho 'boom' >&2\nexit 1\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (events, mut rx) = broadcast::channel::<Value>(32);
        let agent = crate::config::AgentSettings {
            command: script.to_string_lossy().into_owned(),
            args: vec![],
            kind: crate::config::AgentKind::Claude,
        };
        let mgr =
            SuggestSessionManager::new(store, events, agent, "http://127.0.0.1:3456/mcp".into());
        mgr.start().await.unwrap();
        assert_eq!(next_state(&mut rx).await, "started");
        let evt = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(evt["state"], "error");
        assert!(evt["message"].as_str().unwrap().contains("boom"));
    }

    #[tokio::test]
    async fn opencode_agent_uses_run_and_agent_profile_flags() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _db_dir) = test_store().await;
        store
            .store(&NewMemoryRow {
                id: "mem_a",
                title: "A",
                content: "content a",
                tags: &[],
                token_count: None,
                layer: "workspace",
                memory_type: "project",
            })
            .await
            .unwrap();
        let script = dir.path().join("stub-opencode.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$(dirname \"$0\")/args.log\"\necho '{\"session_id\":\"stub-oc-1\",\"result\":\"done\"}'\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (events, mut rx) = broadcast::channel::<Value>(32);
        let agent = crate::config::AgentSettings {
            command: script.to_string_lossy().into_owned(),
            args: vec![],
            kind: crate::config::AgentKind::OpenCode,
        };
        let mgr =
            SuggestSessionManager::new(store, events, agent, "http://127.0.0.1:3456/mcp".into());
        mgr.start().await.unwrap();
        assert_eq!(next_state(&mut rx).await, "started");
        assert_eq!(next_state(&mut rx).await, "suggestions_ready");
        let log = std::fs::read_to_string(dir.path().join("args.log")).unwrap();
        assert!(log.contains("run"));
        assert!(log.contains("--agent mynd-suggest"));
        assert!(log.contains("--format json"));
        assert!(
            !log.contains("--mcp-config"),
            "opencode has no --mcp-config flag; MCP wiring is via the pre-configured agent profile"
        );
        assert!(
            !log.contains("-s "),
            "first turn must not pass a resume session id"
        );
    }
}
