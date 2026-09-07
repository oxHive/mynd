use anyhow::Result;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(crate) fn with_spinner<T>(msg: &str, f: impl FnOnce() -> T) -> T {
    let done = Arc::new(AtomicBool::new(false));
    let done2 = done.clone();
    let label = msg.to_string();
    let width = msg.len() + 4;
    let handle = std::thread::spawn(move || {
        let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let mut i = 0usize;
        while !done2.load(Ordering::Relaxed) {
            print!("\r  {} {label}", frames[i % frames.len()]);
            let _ = std::io::stdout().flush();
            std::thread::sleep(std::time::Duration::from_millis(80));
            i += 1;
        }
        print!("\r{}\r", " ".repeat(width));
        let _ = std::io::stdout().flush();
    });
    let result = f();
    done.store(true, Ordering::Relaxed);
    let _ = handle.join();
    result
}

pub fn cmd_init() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let home = home_dir();
    let report = scaffold(&cwd, &home, &crate::config::global_config_dir())?;
    for (path, status) in &report {
        println!("  {status:7}  {}", path.display());
    }
    println!();

    let registered_clients = with_spinner("checking registered MCP clients...", || {
        detect_registered_clients(&home)
    });
    match registered_clients {
        registered if registered.is_empty() => {
            println!("Mynd initialized.");
            println!();
            println!("Next steps:");
            println!("  1. Register with your AI coding client (run once, not per project):");
            println!("       mynd mcp install claude      # Claude Code");
            println!("       mynd mcp install cursor      # Cursor");
            println!("       mynd mcp install windsurf    # Windsurf");
            println!("       mynd mcp install opencode    # OpenCode");
            println!("       mynd mcp install kimi        # Kimi Code CLI");
            println!("       mynd mcp install codex       # OpenAI Codex CLI");
            println!("  2. Start the server:  mynd service install  (or: mynd up)");
            println!("  3. Open a new session in your AI client. Memory hooks are now active.");
        }
        registered => {
            let list = registered.join(", ");
            println!("Mynd initialized.");
            println!("MCP client already registered: {list}");
            println!();
            println!("Next steps:");
            println!("  1. Start the server:  mynd service install  (or: mynd up)");
            println!("  2. Open a new session. Memory hooks are now active.");
        }
    }

    Ok(())
}

/// Returns names of AI clients that already have Mynd registered.
pub(crate) fn detect_registered_clients(home: &Path) -> Vec<&'static str> {
    let mut found = Vec::new();

    // Claude Code: check config files directly.
    // `claude mcp list` subprocess only shows file-registered servers, not OAuth
    // sessions, so it produces false negatives when the server isn't running yet.
    let claude_dot = home.join(".claude");
    let claude_ok = [
        // stdio/HTTP servers added via `claude mcp add`
        claude_dot.join("mcp.json"),
        // user-scoped settings (mcpServers key)
        claude_dot.join("settings.json"),
        // user-scope registrations from `claude mcp add --scope user`
        home.join(".claude.json"),
    ]
    .iter()
    .any(|p| {
        p.exists()
            && std::fs::read_to_string(p)
                .map(|s| s.contains("hivemind"))
                .unwrap_or(false)
    });
    if claude_ok {
        found.push("claude");
    }

    // File-based clients: just grep for "hivemind" in their config
    let file_clients: &[(&str, &[&str])] = &[
        ("cursor", &[".cursor", "mcp.json"]),
        ("windsurf", &[".codeium", "windsurf", "mcp_config.json"]),
        ("kimi", &[".kimi", "mcp.json"]),
    ];
    for (name, parts) in file_clients {
        let path = parts.iter().fold(home.to_path_buf(), |p, s| p.join(s));
        if path.exists()
            && let Ok(contents) = std::fs::read_to_string(&path)
            && contents.contains("hivemind")
        {
            found.push(name);
        }
    }

    // OpenCode global config
    let xdg_config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let opencode_cfg = xdg_config.join("opencode").join("opencode.json");
    if opencode_cfg.exists()
        && let Ok(contents) = std::fs::read_to_string(&opencode_cfg)
        && contents.contains("hivemind")
    {
        found.push("opencode");
    }

    // Codex: TOML config
    let codex_cfg = home.join(".codex").join("config.toml");
    if codex_cfg.exists()
        && let Ok(contents) = std::fs::read_to_string(&codex_cfg)
        && contents.contains("[mcp_servers.hivemind]")
    {
        found.push("codex");
    }

    found
}

pub(crate) fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Create project + global config files and CLAUDE.md integration.
/// Returns (path, "created"|"exists") for each target. Idempotent.
pub fn scaffold(
    project_root: &Path,
    home: &Path,
    config_dir: &Path,
) -> Result<Vec<(PathBuf, &'static str)>> {
    let project_name = project_root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".to_string());

    let report = vec![
        write_if_absent(
            &project_root.join(".mynd.toml"),
            &project_toml(&project_name),
        )?,
        write_if_absent(&project_root.join(".mynd.local.toml"), LOCAL_TOML)?,
        ensure_line(&project_root.join(".gitignore"), ".mynd.local.toml")?,
        write_if_absent(
            &project_root.join("CLAUDE.md"),
            &project_claude_md(&project_name),
        )?,
        append_block_if_absent(
            &home.join(".claude").join("CLAUDE.md"),
            GLOBAL_CLAUDE_MARKER,
            GLOBAL_CLAUDE_BLOCK,
        )?,
        write_if_absent(&config_dir.join("config.toml"), GLOBAL_CONFIG)?,
        ensure_claude_settings_hook(project_root)?,
    ];

    Ok(report)
}

/// Write `contents` to `path` atomically: write to a sibling temp file, then
/// rename over the destination. This protects an existing user file (e.g. a
/// customized ~/.claude/CLAUDE.md) from truncation if the process is interrupted
/// mid-write. (tempfile is dev-only, so the temp file is created manually.)
pub(crate) fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let tmp = parent.join(format!(".{file_name}.mynd-tmp"));
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub(crate) fn write_if_absent(path: &Path, contents: &str) -> Result<(PathBuf, &'static str)> {
    if path.exists() {
        return Ok((path.to_path_buf(), "exists"));
    }
    write_atomic(path, contents)?;
    Ok((path.to_path_buf(), "created"))
}

/// Ensure `line` is present in the file (create the file if needed).
pub(crate) fn ensure_line(path: &Path, line: &str) -> Result<(PathBuf, &'static str)> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == line) {
        return Ok((path.to_path_buf(), "exists"));
    }
    let mut body = existing;
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(line);
    body.push('\n');
    write_atomic(path, &body)?;
    Ok((path.to_path_buf(), "created"))
}

/// Append `block` to the file unless `marker` is already present.
pub(crate) fn append_block_if_absent(
    path: &Path,
    marker: &str,
    block: &str,
) -> Result<(PathBuf, &'static str)> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    if existing.contains(marker) {
        return Ok((path.to_path_buf(), "exists"));
    }
    let mut body = existing;
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    if !body.is_empty() {
        body.push('\n');
    }
    body.push_str(block);
    write_atomic(path, &body)?;
    Ok((path.to_path_buf(), "created"))
}

/// Merge a SessionStart hook running `mynd session-start` into the
/// project's .claude/settings.json, preserving all existing content.
/// If the file exists but is not valid JSON, returns an error instead of
/// overwriting it, so a malformed user file is never destroyed.
pub(crate) fn ensure_claude_settings_hook(project_root: &Path) -> Result<(PathBuf, &'static str)> {
    use anyhow::Context as _;

    let path = project_root.join(".claude").join("settings.json");
    let existing = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
    if existing.contains("mynd session-start") || existing.contains("hivemind session-start") {
        return Ok((path, "exists"));
    }
    let mut root: serde_json::Value = serde_json::from_str(&existing).with_context(|| {
        format!(
            "{} is not valid JSON; fix or remove it, then re-run mynd init",
            path.display()
        )
    })?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("{} root is not a JSON object", path.display()))?;
    let hooks = obj.entry("hooks").or_insert(serde_json::json!({}));
    let session_start = hooks
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("\"hooks\" is not a JSON object"))?
        .entry("SessionStart")
        .or_insert(serde_json::json!([]));
    session_start
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("\"SessionStart\" is not a JSON array"))?
        .push(serde_json::json!({
            "hooks": [{ "type": "command", "command": "mynd session-start" }]
        }));
    std::fs::create_dir_all(path.parent().unwrap())?;
    write_atomic(&path, &serde_json::to_string_pretty(&root)?)?;
    Ok((path, "created"))
}

pub(crate) fn project_toml(name: &str) -> String {
    format!(
        "[project]\n\
         name = \"{name}\"\n\
         layer = \"workspace\"\n\
         description = \"\"  # fill in: one-line description of this project\n\
         \n\
         [hooks.on_session_start]\n\
         max_tokens = 2000\n\
         recalls = [\n\
         \x20 # Exact memory titles to auto-load at session start, e.g.:\n\
         \x20 # \"golang preferences\",\n\
         \x20 # \"project/{name}\",\n\
         ]\n"
    )
}

pub(crate) const LOCAL_TOML: &str = "# Personal, gitignored recalls — additive on top of .mynd.toml.\n\
# Teammates do not see these. max_tokens here is ADDED to the team budget.\n\
[hooks.on_session_start]\n\
recalls = []\n\
max_tokens = 0\n";

pub(crate) fn project_claude_md(name: &str) -> String {
    format!(
        "# Mynd — {name}\n\n\
         Load project context on session start per .mynd.toml.\n\
         Suggest storing any new architectural decisions made during this session.\n"
    )
}

pub(crate) const GLOBAL_CONFIG: &str = "[defaults]\n\
max_inject_tokens = 2000\n\
suggest_store = true\n\
\n\
[sync]\n\
enabled = false\n\
remote_url = \"\" # Oxhive hosted: https://sync.oxhive.dev  or  self-hosted sqld: http://your-server:8080\n\
api_key = \"\"    # Oxhive account key, or sqld auth token (leave empty if sqld has no auth)\n\
interval_seconds = 300\n\
sync_on_store = true\n\
sync_on_startup = true\n";

// NOTE: rename of this marker + block (and the already-installed ~/.claude/CLAUDE.md
// on user machines) is deferred to the "MCP tool + session-start" rename pass, since
// it depends on renaming the `hivemind_session_start` MCP tool and needs a migration
// that rewrites the existing block rather than appending a second one. That pass must
// also flip the block's `.hivemind.toml` and `<hivemind-context>` references, which
// are stale as of the config-file rename.
pub(crate) const GLOBAL_CLAUDE_MARKER: &str = "# HiveMind Memory System";

pub(crate) const GLOBAL_CLAUDE_BLOCK: &str = "# HiveMind Memory System

You have access to HiveMind via MCP tools: memory_store, memory_recall,
memory_search, memory_update, memory_delete, memory_store_edge, hivemind_session_start.

At the start of every session, before doing anything else:

1. Check if .hivemind.toml exists in the project root.
2. If it exists, call `hivemind_session_start` with the project root path immediately.
3. Incorporate the returned context silently — do not narrate it.

After calling hivemind_session_start:

- If budget.truncated is true, mention once: \"Some memory entries were skipped
  due to token budget. Run `hivemind status` to review.\"
- If any skipped entry has reason not_found, mention once which recalls were not
  found so the user can check their .hivemind.toml.
- Then proceed normally.

If .hivemind.toml does not exist:

- Do not call hivemind_session_start.
- Tools remain available on demand.
- If the user seems to be starting a new project, suggest: \"Run `hivemind init`
  to set up memory hooks for this project.\"

If a <hivemind-context> block is already present in the session context (injected
by the SessionStart hook), do NOT call hivemind_session_start again; use the
injected context directly.

## Suggest storing — never auto-store

When the user shares something worth persisting (preferences, project context,
design decisions), suggest: \"That seems worth remembering — should I store it?\"
Wait for explicit confirmation before calling memory_store.
";
