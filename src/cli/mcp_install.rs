use anyhow::Result;

use super::init::home_dir;

// ── mcp install ───────────────────────────────────────────────────────────────

/// Key the MCP server is registered under in every client's config, plus the
/// pre-rename key. `install` writes `SERVER_KEY` and tears down `LEGACY_KEY`;
/// detection accepts either so an in-transition machine is not nagged.
pub(crate) const SERVER_KEY: &str = "mynd";
pub(crate) const LEGACY_KEY: &str = "hivemind";

/// True if a client's JSON config registers either key, as an MCP server
/// (`"mynd"`) or a Claude Code plugin (`"mynd@<marketplace>"`). Quoted so a
/// project path like `"/home/me/mynd"` in ~/.claude.json doesn't count.
pub(crate) fn already_registered(haystack: &str) -> bool {
    [SERVER_KEY, LEGACY_KEY]
        .iter()
        .any(|k| haystack.contains(&format!("\"{k}\"")) || haystack.contains(&format!("\"{k}@")))
}

pub(crate) fn exe_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| "mynd".to_string())
}

/// Shown wherever a user still has to register Mynd with a client.
pub const REGISTER_HINT: &str = "Register Mynd with your AI client (once per machine):
  Claude Code:  claude plugin marketplace add oxHive/mynd
                claude plugin install mynd@mynd
  Others:       mynd mcp install <cursor|windsurf|opencode|kimi|codex>";

pub fn cmd_mcp_install(client: &str) -> Result<()> {
    match client {
        "claude" => anyhow::bail!(
            "Claude Code uses the Mynd plugin (MCP server + slash commands):\n  \
             claude plugin marketplace add oxHive/mynd\n  \
             claude plugin install mynd@mynd"
        ),
        "opencode" => install_opencode(),
        "kimi" => install_kimi(),
        "codex" => install_codex(),
        "cursor" => install_cursor(),
        "windsurf" => install_windsurf(),
        other => anyhow::bail!(
            "unknown client \"{other}\"; supported: opencode, kimi, codex, cursor, windsurf \
             (Claude Code: use the Mynd plugin)"
        ),
    }
}

/// Install the Mynd Claude Code plugin via the `claude` CLI: adds the
/// marketplace, then the plugin. Re-running either step is harmless.
pub(crate) fn install_claude_plugin() -> Result<()> {
    for args in [
        &["plugin", "marketplace", "add", "oxHive/mynd"][..],
        &["plugin", "install", "mynd@mynd"][..],
    ] {
        let status = std::process::Command::new("claude")
            .args(args)
            .status()
            .map_err(|e| anyhow::anyhow!("could not run the claude CLI: {e}"))?;
        if !status.success() {
            anyhow::bail!("`claude {}` failed", args.join(" "));
        }
    }
    println!("Mynd plugin installed for Claude Code.");
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
        let listed = String::from_utf8_lossy(&list_out.stdout).into_owned();
        if listed.contains(SERVER_KEY) {
            println!("Mynd is already registered with OpenCode.");
            println!("Open a new OpenCode session to use it.");
            return Ok(());
        }
        if listed.contains(LEGACY_KEY) {
            let _ = std::process::Command::new("opencode")
                .args(["mcp", "remove", LEGACY_KEY])
                .status();
        }
        let status = std::process::Command::new("opencode")
            .args(["mcp", "add", SERVER_KEY, &exe])
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
            SERVER_KEY,
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
        let listed = String::from_utf8_lossy(&list_out.stdout).into_owned();
        if listed.contains(SERVER_KEY) {
            println!("Mynd is already registered with Kimi.");
            println!("Open a new Kimi session to use it.");
            return Ok(());
        }
        if listed.contains(LEGACY_KEY) {
            let _ = std::process::Command::new("kimi")
                .args(["mcp", "remove", LEGACY_KEY])
                .status();
        }
        let status = std::process::Command::new("kimi")
            .args(["mcp", "add", SERVER_KEY, &exe])
            .status()?;
        if !status.success() {
            anyhow::bail!("kimi mcp add failed. Check `kimi mcp list`");
        }
    } else {
        let config_path = home_dir().join(".kimi").join("mcp.json");
        upsert_json_mcp(
            &config_path,
            SERVER_KEY,
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

/// Remove a `[header]` table and its body (up to the next table header or EOF)
/// from a TOML document. Whitespace-tolerant on the header line; leaves the
/// rest untouched. Used to drop a stale pre-rename `[mcp_servers.hivemind]`.
pub(crate) fn strip_toml_table(doc: &str, header: &str) -> String {
    let mut out = String::with_capacity(doc.len());
    let mut skipping = false;
    for line in doc.lines() {
        let trimmed = line.trim();
        if trimmed == header {
            skipping = true;
            continue;
        }
        if skipping {
            if trimmed.starts_with('[') {
                skipping = false;
            } else {
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn install_codex() -> Result<()> {
    let config_path = home_dir().join(".codex").join("config.toml");
    std::fs::create_dir_all(config_path.parent().unwrap())?;

    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    if existing.contains("[mcp_servers.mynd]") {
        println!("Mynd is already registered with Codex CLI.");
        println!("Open a new Codex session to use it.");
        return Ok(());
    }

    // Strip a stale pre-rename table, then append the current one.
    let existing = strip_toml_table(&existing, "[mcp_servers.hivemind]");
    let block = format!(
        "\n[mcp_servers.mynd]\ncommand = \"{}\"\nargs = []\n",
        toml_escape(&exe_path())
    );
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
        SERVER_KEY,
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
        SERVER_KEY,
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

    let servers = root
        .as_object_mut()
        .unwrap()
        .entry(servers_key)
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .unwrap();
    // Drop a stale pre-rename entry, then write the current one.
    if name != LEGACY_KEY {
        servers.remove(LEGACY_KEY);
    }
    servers.insert(name.to_string(), entry);

    std::fs::write(path, serde_json::to_string_pretty(&root)?)?;
    Ok(())
}
