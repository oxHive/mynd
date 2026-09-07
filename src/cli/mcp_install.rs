use anyhow::Result;

use super::init::home_dir;

// ── mcp install ───────────────────────────────────────────────────────────────
// NOTE: the MCP server registration key ("hivemind") and the client-config
// detection tokens are still "hivemind" on purpose — renaming that key is
// deferred to the "MCP tool + server integration" rename pass (it orphans
// existing client registrations and pairs with the hivemind_session_start
// tool rename).

pub(crate) fn exe_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| "mynd".to_string())
}

pub fn cmd_mcp_install(client: &str) -> Result<()> {
    match client {
        "claude" => install_claude(),
        "opencode" => install_opencode(),
        "kimi" => install_kimi(),
        "codex" => install_codex(),
        "cursor" => install_cursor(),
        "windsurf" => install_windsurf(),
        other => anyhow::bail!(
            "unknown client \"{other}\" — supported: claude, opencode, kimi, codex, cursor, windsurf"
        ),
    }
}

fn install_claude() -> Result<()> {
    // Verify the claude CLI is available.
    let claude_check = std::process::Command::new("claude")
        .arg("--version")
        .output();
    if claude_check.is_err() {
        anyhow::bail!(
            "claude CLI not found in PATH\n\
             Install Claude Code first: https://claude.ai/download\n\
             Then re-run: mynd mcp install claude"
        );
    }

    // Check if already registered so we can skip gracefully.
    let list_out = std::process::Command::new("claude")
        .args(["mcp", "list"])
        .output()?;
    let list_str = String::from_utf8_lossy(&list_out.stdout);
    if list_str.contains("hivemind") {
        println!("Mynd is already registered with Claude Code.");
        println!("Open a new Claude Code session to use it.");
        return Ok(());
    }

    // Register as stdio using the full binary path so Claude Code can find it
    // regardless of whether ~/.cargo/bin is in its subprocess PATH. User scope:
    // the default (local) scope is per project directory, but registration is
    // meant to happen once per machine.
    let exe = exe_path();
    let status = std::process::Command::new("claude")
        .args(["mcp", "add", "--scope", "user", "hivemind", "--", &exe])
        .status()?;

    if !status.success() {
        anyhow::bail!("claude mcp add failed. Run `claude mcp list` to inspect existing servers");
    }

    println!("Mynd registered with Claude Code.");
    println!();
    println!("Next steps:");
    println!("  1. Open a new Claude Code session");
    println!("  2. Type /memory-status to verify");
    Ok(())
}

fn install_opencode() -> Result<()> {
    // Try the CLI first; fall back to writing the config file directly.
    let cli_available = std::process::Command::new("opencode")
        .arg("--version")
        .output()
        .is_ok();

    let exe = exe_path();
    if cli_available {
        let list_out = std::process::Command::new("opencode")
            .args(["mcp", "list"])
            .output()?;
        if String::from_utf8_lossy(&list_out.stdout).contains("hivemind") {
            println!("Mynd is already registered with OpenCode.");
            println!("Open a new OpenCode session to use it.");
            return Ok(());
        }
        let status = std::process::Command::new("opencode")
            .args(["mcp", "add", "hivemind", &exe])
            .status()?;
        if !status.success() {
            anyhow::bail!("opencode mcp add failed. Check `opencode mcp list`");
        }
    } else {
        // Write to the global opencode config.
        let xdg_config = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".config"));
        let config_path = xdg_config.join("opencode").join("opencode.json");
        upsert_json_mcp(
            &config_path,
            "hivemind",
            serde_json::json!({
                "type": "local",
                "command": exe,
                "args": []
            }),
        )?;
        println!("Written to {}", config_path.display());
    }

    println!("Mynd registered with OpenCode.");
    println!();
    println!("Next steps:");
    println!("  1. Open a new OpenCode session");
    Ok(())
}

fn install_kimi() -> Result<()> {
    let cli_available = std::process::Command::new("kimi")
        .arg("--version")
        .output()
        .is_ok();

    let exe = exe_path();
    if cli_available {
        let list_out = std::process::Command::new("kimi")
            .args(["mcp", "list"])
            .output()?;
        if String::from_utf8_lossy(&list_out.stdout).contains("hivemind") {
            println!("Mynd is already registered with Kimi.");
            println!("Open a new Kimi session to use it.");
            return Ok(());
        }
        let status = std::process::Command::new("kimi")
            .args(["mcp", "add", "hivemind", &exe])
            .status()?;
        if !status.success() {
            anyhow::bail!("kimi mcp add failed. Check `kimi mcp list`");
        }
    } else {
        let config_path = home_dir().join(".kimi").join("mcp.json");
        upsert_json_mcp(
            &config_path,
            "hivemind",
            serde_json::json!({ "command": exe, "args": [] }),
        )?;
        println!("Written to {}", config_path.display());
    }

    println!("Mynd registered with Kimi Code CLI.");
    println!();
    println!("Next steps:");
    println!("  1. Open a new Kimi session");
    Ok(())
}

/// Escape a string for use inside a basic (double-quoted) TOML string.
pub(crate) fn toml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn install_codex() -> Result<()> {
    let config_path = home_dir().join(".codex").join("config.toml");
    std::fs::create_dir_all(config_path.parent().unwrap())?;

    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    if existing.contains("[mcp_servers.hivemind]") {
        println!("Mynd is already registered with Codex CLI.");
        println!("Open a new Codex session to use it.");
        return Ok(());
    }

    let block = format!(
        "\n[mcp_servers.hivemind]\ncommand = \"{}\"\nargs = []\n",
        toml_escape(&exe_path())
    );
    let block = block.as_str();
    let new_content = format!("{}{}", existing.trim_end(), block);
    std::fs::write(&config_path, new_content)?;
    println!("Written to {}", config_path.display());

    println!("Mynd registered with OpenAI Codex CLI.");
    println!();
    println!("Next steps:");
    println!("  1. Open a new Codex session");
    Ok(())
}

fn install_cursor() -> Result<()> {
    let config_path = home_dir().join(".cursor").join("mcp.json");
    upsert_json_mcp(
        &config_path,
        "hivemind",
        serde_json::json!({ "command": exe_path(), "args": [] }),
    )?;
    println!("Written to {}", config_path.display());
    println!("Mynd registered with Cursor.");
    println!();
    println!("Next steps:");
    println!("  1. Restart Cursor completely for the change to take effect");
    Ok(())
}

fn install_windsurf() -> Result<()> {
    let config_path = home_dir()
        .join(".codeium")
        .join("windsurf")
        .join("mcp_config.json");
    upsert_json_mcp(
        &config_path,
        "hivemind",
        serde_json::json!({ "command": exe_path(), "args": [] }),
    )?;
    println!("Written to {}", config_path.display());
    println!("Mynd registered with Windsurf.");
    println!();
    println!("Next steps:");
    println!("  1. Restart Windsurf for the change to take effect");
    Ok(())
}

/// Read an mcpServers-style JSON config, insert or update the named server entry, write back.
/// Creates the file and parent directories if absent.
pub(crate) fn upsert_json_mcp(
    path: &std::path::Path,
    name: &str,
    entry: serde_json::Value,
) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;

    let mut root: serde_json::Value = if path.exists() {
        let raw = std::fs::read_to_string(path)?;
        serde_json::from_str(&raw).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // OpenCode uses {"mcp": {...}}; all other clients use {"mcpServers": {...}}.
    // Detect from existing file structure; fall back based on whether entry has "type" field.
    let servers_key = if root.get("mcp").is_some() || entry.get("type").is_some() {
        "mcp"
    } else {
        "mcpServers"
    };

    root.as_object_mut()
        .unwrap()
        .entry(servers_key)
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .unwrap()
        .insert(name.to_string(), entry);

    std::fs::write(path, serde_json::to_string_pretty(&root)?)?;
    Ok(())
}
