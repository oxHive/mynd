use crate::config::{AgentKind, AgentSettings};
use serde_json::{Value, json};
use std::time::Duration;

const TURN_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug)]
pub struct TurnResult {
    pub reply_text: String,
    pub session_id: String,
}

pub async fn run_turn(
    agent: &AgentSettings,
    mynd_bin: &str,
    prompt: &str,
    resume: Option<&str>,
    system_prompt: Option<&str>,
) -> Result<TurnResult, String> {
    match agent.kind {
        AgentKind::OpenCode => {
            // opencode's `run` subcommand has no per-invocation system-prompt
            // flag, so this instruction can't be routed through a trusted
            // channel here; it's silently dropped rather than spliced into the
            // user-visible prompt, where it would be indistinguishable from
            // attacker-controlled message text.
            run_opencode_turn(agent, prompt, resume).await
        }
        AgentKind::Claude => {
            run_claude_turn(agent, mynd_bin, prompt, resume, system_prompt).await
        }
    }
}

async fn run_claude_turn(
    agent: &AgentSettings,
    mynd_bin: &str,
    prompt: &str,
    resume: Option<&str>,
    system_prompt: Option<&str>,
) -> Result<TurnResult, String> {
    let mcp_config = json!({
        "mcpServers": { "mynd": { "command": mynd_bin, "args": [] } }
    })
    .to_string();
    let mut cmd = tokio::process::Command::new(&agent.command);
    cmd.args(&agent.args);
    if let Some(id) = resume {
        cmd.arg("--resume").arg(id);
    }
    if let Some(sp) = system_prompt {
        cmd.arg("--append-system-prompt").arg(sp);
    }
    cmd.arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("json")
        .arg("--mcp-config")
        .arg(&mcp_config)
        .arg("--strict-mcp-config")
        .arg("--allowedTools")
        .arg("mcp__mynd__memory_store,mcp__mynd__memory_recall,mcp__mynd__memory_search,mcp__mynd__memory_update")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let out = spawn_and_wait(cmd).await?;
    let v: Value =
        serde_json::from_str(&out).map_err(|e| format!("unparseable agent output: {e}"))?;
    let session_id = v
        .get("session_id")
        .and_then(|s| s.as_str())
        .ok_or_else(|| "agent output missing session_id".to_string())?
        .to_string();
    let reply_text = v
        .get("result")
        .and_then(|s| s.as_str())
        .unwrap_or_default()
        .to_string();
    Ok(TurnResult {
        reply_text,
        session_id,
    })
}

async fn run_opencode_turn(
    agent: &AgentSettings,
    prompt: &str,
    resume: Option<&str>,
) -> Result<TurnResult, String> {
    let mut cmd = tokio::process::Command::new(&agent.command);
    cmd.args(&agent.args).arg("run").arg(prompt);
    if let Some(id) = resume {
        cmd.arg("-s").arg(id);
    }
    cmd.arg("--agent")
        .arg("mynd-bot")
        .arg("--format")
        .arg("json")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let out = spawn_and_wait(cmd).await?;
    let v: Value =
        serde_json::from_str(&out).map_err(|e| format!("unparseable agent output: {e}"))?;
    let session_id = v
        .get("session_id")
        .and_then(|s| s.as_str())
        .ok_or_else(|| "agent output missing session_id".to_string())?
        .to_string();
    let reply_text = v
        .get("result")
        .and_then(|s| s.as_str())
        .unwrap_or_default()
        .to_string();
    Ok(TurnResult {
        reply_text,
        session_id,
    })
}

async fn spawn_and_wait(mut cmd: tokio::process::Command) -> Result<String, String> {
    let child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn agent: {e}"))?;
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
    stdout
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| "agent produced no output".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn write_stub_claude_agent(dir: &std::path::Path) -> String {
        let script = dir.join("stub-claude.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$(dirname \"$0\")/args.log\"\n\
             echo '{\"type\":\"result\",\"session_id\":\"stub-sess-1\",\"result\":\"stored it\",\"is_error\":false}'\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script.to_string_lossy().into_owned()
    }

    fn write_stub_opencode_agent(dir: &std::path::Path) -> String {
        let script = dir.join("stub-opencode.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$(dirname \"$0\")/args.log\"\n\
             echo '{\"session_id\":\"stub-sess-2\",\"result\":\"stored it\"}'\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn claude_turn_passes_isolation_flags_and_stdio_mcp_config() {
        let dir = tempfile::tempdir().unwrap();
        let script = write_stub_claude_agent(dir.path());
        let agent = AgentSettings {
            command: script,
            args: vec![],
            kind: AgentKind::Claude,
        };
        let result = run_turn(&agent, "/usr/local/bin/mynd", "remember X", None, None)
            .await
            .unwrap();
        assert_eq!(result.session_id, "stub-sess-1");
        assert_eq!(result.reply_text, "stored it");
        let log = std::fs::read_to_string(dir.path().join("args.log")).unwrap();
        assert!(log.contains("-p"));
        assert!(log.contains("remember X"));
        assert!(log.contains("--output-format json"));
        assert!(log.contains("--strict-mcp-config"));
        assert!(log.contains("--allowedTools"));
        assert!(log.contains("mcp__mynd__memory_store"));
        assert!(
            log.contains("\"command\":\"/usr/local/bin/mynd\""),
            "mcp-config must point at mynd in stdio mode, not an HTTP url"
        );
        assert!(
            !log.contains("--resume"),
            "first turn must not pass --resume"
        );
    }

    #[tokio::test]
    async fn claude_turn_resumes_with_the_given_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let script = write_stub_claude_agent(dir.path());
        let agent = AgentSettings {
            command: script,
            args: vec![],
            kind: AgentKind::Claude,
        };
        run_turn(
            &agent,
            "/usr/local/bin/mynd",
            "again",
            Some("prior-session"),
            None,
        )
        .await
        .unwrap();
        let log = std::fs::read_to_string(dir.path().join("args.log")).unwrap();
        assert!(log.contains("--resume prior-session"));
    }

    #[tokio::test]
    async fn claude_turn_passes_system_prompt_via_flag_not_the_user_message() {
        let dir = tempfile::tempdir().unwrap();
        let script = write_stub_claude_agent(dir.path());
        let agent = AgentSettings {
            command: script,
            args: vec![],
            kind: AgentKind::Claude,
        };
        run_turn(
            &agent,
            "/usr/local/bin/mynd",
            "hello",
            None,
            Some("use layer \"personal\" and tag source:matrix"),
        )
        .await
        .unwrap();
        let log = std::fs::read_to_string(dir.path().join("args.log")).unwrap();
        assert!(log.contains("--append-system-prompt"));
        assert!(log.contains("use layer \"personal\" and tag source:matrix"));
    }

    #[tokio::test]
    async fn opencode_turn_uses_run_and_agent_profile_flags() {
        let dir = tempfile::tempdir().unwrap();
        let script = write_stub_opencode_agent(dir.path());
        let agent = AgentSettings {
            command: script,
            args: vec![],
            kind: AgentKind::OpenCode,
        };
        let result = run_turn(
            &agent,
            "/usr/local/bin/mynd",
            "remember X",
            Some("sess-1"),
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.session_id, "stub-sess-2");
        let log = std::fs::read_to_string(dir.path().join("args.log")).unwrap();
        assert!(log.contains("run"));
        assert!(log.contains("--agent mynd-bot"));
        assert!(log.contains("-s sess-1"));
        assert!(log.contains("--format json"));
    }

    #[tokio::test]
    async fn nonzero_exit_is_reported_as_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("bad-agent.sh");
        std::fs::write(&script, "#!/bin/sh\necho 'boom' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let agent = AgentSettings {
            command: script.to_string_lossy().into_owned(),
            args: vec![],
            kind: AgentKind::Claude,
        };
        let err = run_turn(&agent, "/usr/local/bin/mynd", "hi", None, None)
            .await
            .unwrap_err();
        assert!(err.contains("boom"));
    }
}
