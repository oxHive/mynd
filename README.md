# Mynd

[![Crates.io](https://img.shields.io/crates/v/oxmynd.svg)](https://crates.io/crates/oxmynd)
[![Crates.io Downloads](https://img.shields.io/crates/d/oxmynd.svg)](https://crates.io/crates/oxmynd)
[![CI](https://github.com/oxhive/mynd/actions/workflows/pull-request.yml/badge.svg?event=pull_request)](https://github.com/oxhive/mynd/actions/workflows/pull-request.yml)
[![codecov](https://codecov.io/gh/oxhive/mynd/branch/main/graph/badge.svg)](https://codecov.io/gh/oxhive/mynd)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
[![GitHub release](https://img.shields.io/github/v/release/oxhive/mynd)](https://github.com/oxhive/mynd/releases)

> 🚧 **Under active development.** APIs and config formats may change between releases.

Persistent memory for AI coding agents. Mynd is a local MCP server that gives Claude Code (and other AI agents) access to a [libsql](https://github.com/tursodatabase/libsql)-backed memory store, keeping context, preferences, and project knowledge alive across sessions.

## How it works

1. You register Mynd with your AI client once: `mynd mcp install claude`.
2. Your AI client spawns Mynd as a subprocess. No server to start or keep running.
3. At the start of every session, Claude automatically recalls the memories configured for your project.
4. You ask Claude to store anything worth keeping. It never auto-stores.

### The session start flow in detail

When you open a new Claude Code session in a project that has `.mynd.toml`, the primary mechanism is the SessionStart hook that `mynd init` installs in `.claude/settings.json`:

1. Claude Code runs `mynd session-start` before the session begins.
2. Mynd reads `.mynd.toml` (and `.mynd.local.toml` if present), resolves each `recalls` entry against the SQLite database (by exact title, then FTS), and prints the results inside a `<mynd-context>` block, staying within `max_tokens`.
3. Claude Code injects that output into the session context deterministically — no tool call, no model discretion involved.

For clients without hook support (or projects initialised before the hook existed), the fallback is the CLAUDE.md-instructed tool call: Claude reads CLAUDE.md, calls the `mynd_session_start` MCP tool with the project path, and incorporates the returned JSON silently. If the hook already injected a `<mynd-context>` block, Claude skips the tool call.

That's it. One injection, one round-trip to the database, zero per-prompt overhead after that.

### Context budget

The `max_tokens` cap prevents session start from consuming too much of Claude's context window. Memories are loaded in order; if an entry would push past the budget, it is skipped (but later, smaller entries still get a chance). The `mynd status` command shows a preview of exactly what would be injected and how many tokens it costs.

---

## Fetching memories during a session

The `recalls` list in `.mynd.toml` is only for **automatic injection at session start**. You can always fetch any memory on demand during a session:

- **By title or ID**: ask Claude: *"recall the memory titled 'golang preferences'"* → Claude calls `memory_recall`
- **By keyword**: ask Claude: *"search my memories for postgres"* → Claude calls `memory_search` (FTS, returns snippets)
- **By tag**: ask Claude: *"find memories tagged lang:rust and project:mynd"* → Claude calls `memory_search` with a `tags` array (AND-only; combine with a keyword `query` too if you like)
- **Browse all**: use the `/memory-list` prompt

Memories not listed in `recalls` are still available; they're just not auto-loaded. They live in the database and are available any time you ask.

---

## How Mynd differs from Claude Code's built-in hooks

Claude Code has its own hook system in `.claude/settings.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      { "hooks": [{ "type": "command", "command": "my-memory-tool search" }] }
    ]
  }
}
```

This runs a shell command and injects its stdout into the conversation. It works, but the trade-offs differ:

| | Claude Code `UserPromptSubmit` hook | Mynd `[hooks.on_session_start]` |
|---|---|---|
| **When it runs** | On **every message** you send | **Once** per session |
| **Context overhead** | Added to every prompt, every time | Injected once; zero cost after that |
| **Token budget** | None; dumps all output unconditionally | `max_tokens` cap with per-entry skipping |
| **Data source** | Anything a shell command outputs | SQLite FTS store, queryable by title/ID/keyword |
| **Selectivity** | Whatever the command returns | You specify exactly which memories per project |
| **Persistence** | Stateless; reruns the command fresh each call | Stateful; memories survive across machines and reinstalls |
| **On-demand access** | Only what the hook returns | Full MCP tools (`memory_recall`, `memory_search`, etc.) |

**The short version:** the hook approach re-injects context on every single message, which burns tokens proportionally to how often you prompt. Mynd injects once at session start and then stays out of the way. After that, Claude uses what it loaded and can call on-demand tools if it needs more.

---

## Installation

```sh
cargo binstall oxmynd       # download pre-built binary (recommended, includes dashboard)
```

Compile from source instead:

```sh
cargo install oxmynd        # dashboard shows setup instructions instead of the UI
```

To get the dashboard bundled in a source build, compile from a local checkout instead of crates.io:

```sh
git clone https://github.com/oxhive/mynd
cd mynd
(cd dashboard && bun install && bun run build)
cargo install --path .
```

### Claude Code

Install the Mynd plugin (recommended):

```sh
claude plugin marketplace add oxHive/mynd
claude plugin install mynd@mynd
```

This registers the MCP server and installs `/memory-store`, `/memory-search`, `/memory-list`, `/memory-edit`, and `/memory-status` as slash commands in one step.

If you have a local clone, you can add the marketplace from the path instead:

```sh
claude plugin marketplace add /path/to/mynd
claude plugin install mynd@mynd
```

Verify with `/plugin` in a Claude Code session — Mynd should be listed as installed, with its MCP server connected under `/mcp`. The slash commands appear in the `/` menu.

**Manual alternative (MCP only, no slash commands):**

```sh
mynd mcp install claude
```

This registers the MCP server at user scope (once per machine, available in every project) without the plugin skills. Useful if you only want the tools and session start, not the slash commands.

In addition, `mynd init` installs a Claude Code SessionStart hook in the project's `.claude/settings.json` that runs `mynd session-start`. This injects the configured memory context deterministically at the start of every session, without relying on Claude deciding to call the MCP tool. The `mynd_session_start` MCP tool remains available for other clients and for on-demand use.

### OpenCode

Install the Mynd plugin (recommended):

```json
{
  "$schema": "https://opencode.ai/config.json",
  "plugin": ["@oxhive/opencode-mynd"]
}
```

Add that to `opencode.json` (project) or `~/.config/opencode/opencode.json` (global). OpenCode's Bun runtime installs the package automatically on next start — no separate `npm install` step. The plugin then does three things at startup:

- **Auto-registers the MCP server** if it finds `mynd` in `PATH` (skips silently if you've already configured `mcp.mynd` yourself, e.g. via the manual method below).
- **Injects the Mynd system-prompt instructions** into every session — the OpenCode equivalent of the CLAUDE.md block `mynd init` writes for Claude Code, telling the agent when to call `mynd_session_start` and to never auto-store.
- **Installs the memory skills** (`memory-store`, `memory-search`, `memory-list`, `memory-edit`, `memory-status`, `memory-connections`) into `~/.config/opencode/skills/` (or `$XDG_CONFIG_HOME/opencode/skills/`). OpenCode only discovers skills from specific filesystem paths, never from npm package contents, so the plugin copies its bundled skills there itself on every load — this keeps them in sync with the installed plugin version, so don't hand-edit the copies.

The `mynd` binary itself still needs to be installed and on `PATH` (see [Installation](#installation) above) — the plugin only wires it up, it doesn't ship the server.

**Manual alternative (MCP only, no skills):**

```sh
mynd mcp install opencode
```

Writes to `~/.config/opencode/opencode.json` directly (uses the `opencode` CLI if available). Redundant once the plugin is installed, since the plugin registers the MCP server itself — use this if you'd rather not add a plugin dependency, or need MCP-only without the auto-injected instructions or skills. Manual config:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "mynd": {
      "type": "local",
      "command": "mynd",
      "args": []
    }
  }
}
```

Docs: [opencode.ai/docs/mcp-servers](https://opencode.ai/docs/mcp-servers/), [opencode.ai/docs/plugins](https://opencode.ai/docs/plugins/)

### Kimi Code CLI

```sh
mynd mcp install kimi
```

Uses the `kimi` CLI if available, otherwise writes to `~/.kimi/mcp.json` directly. Manual config:

```json
{
  "mcpServers": {
    "mynd": {
      "command": "mynd",
      "args": []
    }
  }
}
```

Docs: [moonshotai.github.io/kimi-cli/en/customization/mcp.html](https://moonshotai.github.io/kimi-cli/en/customization/mcp.html)

### OpenAI Codex CLI

```sh
mynd mcp install codex
```

Appends to `~/.codex/config.toml`. Manual config:

```toml
[mcp_servers.mynd]
command = "mynd"
args = []
```

Docs: [developers.openai.com/codex/mcp](https://developers.openai.com/codex/mcp)

### Cursor

```sh
mynd mcp install cursor
```

Writes to `~/.cursor/mcp.json`. Restart Cursor after running. Manual config:

```json
{
  "mcpServers": {
    "mynd": {
      "command": "mynd",
      "args": []
    }
  }
}
```

Docs: [cursor.com/docs/mcp](https://cursor.com/docs/mcp)

### Windsurf

```sh
mynd mcp install windsurf
```

Writes to `~/.codeium/windsurf/mcp_config.json`. Restart Windsurf after running. Manual config:

```json
{
  "mcpServers": {
    "mynd": {
      "command": "mynd",
      "args": []
    }
  }
}
```

Docs: [docs.windsurf.com/windsurf/cascade/mcp](https://docs.windsurf.com/windsurf/cascade/mcp)

### Other MCP-compatible clients

Any client that supports the MCP stdio transport can run `mynd` as a subprocess. Refer to your client's documentation for how to register a local stdio MCP server.

If your client only supports HTTP transport, start Mynd's HTTP server with `mynd up` and point it at `http://127.0.0.1:3456/mcp`. No authentication is required for local connections.

---

## Quick start

```sh
# 1. Install the plugin (once per machine — registers MCP + slash commands)
claude plugin marketplace add oxHive/mynd
claude plugin install mynd@mynd

# 2. Go to your project and initialise it
cd ~/projects/myapp
mynd init

# 3. Open a new Claude Code session (memory hooks are now active)
```

No server to start. Claude Code spawns Mynd as a subprocess automatically.

### Dashboard and REST API (optional)

The `mynd up` command starts an HTTP server with a web dashboard for browsing and managing memories, plus a REST API for custom integrations. This is not required for the MCP connection to work.

```sh
mynd up          # MCP (HTTP) + REST API + dashboard
mynd up --headless  # MCP (HTTP) + REST API, no dashboard
```

To keep the dashboard available persistently, install Mynd as a user-level service:

```sh
mynd service install
```

This writes a unit file (Linux) or launchd plist (macOS) and enables it immediately, with no `sudo` required.

| Platform | Mechanism | Unit file location |
|----------|-----------|-------------------|
| Linux | systemd user unit | `~/.config/systemd/user/mynd.service` |
| macOS | launchd LaunchAgent | `~/Library/LaunchAgents/dev.oxhive.mynd.plist` |

On macOS, logs are written to `~/Library/Logs/mynd.log`.

```sh
mynd service status    # check if running
mynd service uninstall # stop and remove
```

`mynd init` creates:

| File | Description |
|------|-------------|
| `.mynd.toml` | Project config (commit this) |
| `.mynd.local.toml` | Personal recalls, gitignored |
| `CLAUDE.md` | Instructs Claude to call `mynd_session_start` |
| `.gitignore` | Adds `.mynd.local.toml` entry |

It also appends a Mynd block to `~/.claude/CLAUDE.md` (preserving any existing content) so Claude knows how to use the MCP tools globally.

> **If you already have a project `CLAUDE.md`**, init will not modify it. Add this line manually:
> ```
> At the start of every session, call `mynd_session_start` if .mynd.toml exists in the project root.
> ```

The `CLAUDE.md` created by `mynd init` only covers how to **use** Mynd. It tells Claude when to call `mynd_session_start` and nothing else. It does not document your project's own codebase. If you want Claude Code to understand your codebase architecture, run `/init` in Claude Code after `mynd init`. The `/init` command reads your source code and generates a comprehensive `CLAUDE.md` with build commands, architecture overview, and key design decisions.

---

## Commands

```
mynd up                      Start the server (MCP + REST API + dashboard)
mynd up --headless           Start without the dashboard UI
mynd init                    Scaffold config files for the current project
mynd status                  Show config, memory count, and session-start preview
mynd migrate                 Move the database from the legacy ~/.hivemind path to the XDG data dir
mynd session-start [--json]  Print the session-start context; used by the Claude Code SessionStart hook
mynd mcp install claude      Register with Claude Code
mynd mcp install opencode    Register with OpenCode (manual; the npm plugin does this automatically)
mynd mcp install kimi        Register with Kimi Code CLI
mynd mcp install codex       Register with OpenAI Codex CLI
mynd mcp install cursor      Register with Cursor
mynd mcp install windsurf    Register with Windsurf
mynd service install         Install and enable as a background service
mynd service uninstall       Stop and remove the background service
mynd service status          Show background service status
mynd matrix login            Log into a Matrix account (once); session saved to OS keyring
mynd matrix run               Run the Matrix bot daemon
mynd matrix status            Show Matrix bot login/sync/session state
mynd discord login           Log into a Discord bot account (once); token saved to OS keyring
mynd discord run              Run the Discord bot daemon
mynd discord status           Show Discord bot login/sync/session state
mynd dashboard --open        Open the dashboard (requires server running)
```

---

## Configuration

### Project config: `.mynd.toml`

Committed to the repo. Shared across the team.

```toml
[project]
name = "myapp"
layer = "workspace"
description = "Short project description"

[hooks.on_session_start]
max_tokens = 2000
recalls = [
  "golang preferences",
  "project/myapp",
]
```

`recalls` is a list of memory titles to auto-inject at session start. Each entry is looked up by exact title, then falls back to FTS. The combined size is capped at `max_tokens`.

A recall entry can also be a boolean tag expression instead of a title — use `&` (AND), `|` (OR), `!` (NOT), and parens for grouping, with each tag written as `tag:<namespace:value>`:

```toml
recalls = [
  "tag:project:mynd & tag:lang:rust",
  "tag:project:mynd & !tag:status:done",
  "my exact memory title",
]
```

Unlike a plain title recall (which loads at most one memory), a tag expression loads **every** matching memory, still subject to the overall `max_tokens` budget. An entry is only parsed as a tag expression if it starts with `tag:`, `!tag:`, or `(` — anything else is treated as a plain title/FTS query exactly as before.

### Personal config: `.mynd.local.toml`

Gitignored. Your own additions on top of the team config.

```toml
[hooks.on_session_start]
recalls = ["my personal style notes"]
max_tokens = 500   # added to the team budget
```

### Global config: `~/.config/mynd/config.toml`

Created by `mynd init`. Applies to all projects.

```toml
[defaults]
max_inject_tokens = 2000   # default token budget when project doesn't set one

[server]
host = "127.0.0.1"
port = 3456

[dashboard]
port = 3457
# api_url = "http://127.0.0.1:3456"      # override if the server isn't on the default host/port
# cors_origin = "http://127.0.0.1:3457"  # override if you run the dashboard separately (e.g. `bun run dev` on :5173)

[sync]
enabled = false
remote_url = ""        # sqld server URL or Oxhive hosted endpoint
api_key = ""           # sqld auth token, or Oxhive account key
interval_seconds = 300
sync_on_store = true
sync_on_startup = true

[org_sync]
enabled = false
remote_url = ""        # hivemind-gateway URL (paid), or a self-hosted sqld URL
api_key = ""           # gateway-issued key, or sqld auth token
interval_seconds = 300
sync_on_store = true
sync_on_startup = true
```

`$XDG_CONFIG_HOME/mynd/config.toml` is used instead if `XDG_CONFIG_HOME` is set.

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MYND_DB_PATH` | `~/.local/share/mynd/memories.db` (or `$XDG_DATA_HOME/mynd/memories.db`) | Path to the SQLite database |

Databases from versions before 0.3.x lived at `~/.hivemind/memories.db`; run `mynd migrate` to move them.

---

## Sync (optional)

Mynd can replicate memories to a remote server, which is useful for sharing across machines or keeping a remote backup. Sync uses [libsql](https://github.com/tursodatabase/libsql) embedded replication: the local database stays fully functional offline, and `mynd up` periodically replicates writes to the remote primary.

```toml
[sync]
enabled = true
remote_url = "http://pi.local:8080"   # see options below
api_key = "your-auth-token"           # see options below
interval_seconds = 300                # background sync every 5 minutes
sync_on_store = true                  # also sync immediately after each memory is stored
sync_on_startup = true                # sync once when the server starts
```

Two `remote_url` targets are supported:

| Setup | `remote_url` points to | `api_key` |
|-------|------------------------|-----------|
| **Self-hosted** | Your own [sqld](https://github.com/tursodatabase/libsql/tree/main/libsql-server) server | sqld auth token; leave empty if sqld has no auth configured |
| **Oxhive hosted** *(coming soon)* | `https://sync.oxhive.dev` | Your Oxhive account key |

`api_key` is never sent to Claude or the dashboard. It is only used during replication.

With `sync_on_store = true`, a memory stored through any interface (MCP tool, REST API, or dashboard) triggers an immediate sync in addition to the periodic background sync. If a sync pulls remote changes that overwrite a local edit, Mynd records a conflict holding both versions; pending conflicts appear in the dashboard's Feedback view. Resolving with `keep_local` restores your version of the content, while `keep_remote` accepts the replicated one.

### Org layer (optional)

A third memory layer, alongside `personal` and `workspace`, backed by its own independent database connection — configured separately from `[sync]` via `[org_sync]`:

```toml
[org_sync]
enabled = true
remote_url = "http://pi.local:8080"   # self-hosted sqld, or a hivemind-gateway URL
api_key = "your-auth-token"
interval_seconds = 300
sync_on_store = true
sync_on_startup = true
```

Org-layer memories are visible and editable everywhere personal/workspace memories are — MCP tools, the REST API, and the dashboard (Memories list, Graph view, and the layer picker, which disables "org" until `[org_sync]` is configured). They live in a separate local database (`org.db`, next to `memories.db`) and sync independently of `[sync]`. `memory_store` accepts `layer: "personal" | "workspace" | "org"` (default `workspace`).

Mynd ships with no access control of its own — org CRUD is as open as personal/workspace. Multi-user access control for a shared org store is `hivemind-gateway`'s job, not yet built.

---

## Matrix chat interface (optional)

Capture and recall Mynd memories from a Matrix room or DM — mention the bot in a
room, or DM it directly. Under the hood it's the same headless-agent mechanism as the
dashboard's suggest flow: no bespoke NLU, no local model.

This is a separate process from `mynd up` and doesn't depend on it being started —
each message spawns a short-lived agent turn that talks to Mynd the same way any
other MCP client does.

### Setup

```sh
mynd matrix login
```

Prompts for your homeserver URL, the bot's Matrix user ID, and its password. The
password is used once, to log in, then discarded — only the resulting session is
persisted, in your OS keyring (Secret Service/kwallet on Linux, Keychain on macOS).

> **Headless Linux servers:** `keyring` needs a functioning Secret Service (D-Bus). A
> bare VPS with no login session running may not have one available; `mynd matrix
> login` will fail with an actionable message if so. Install/start a Secret Service
> provider (e.g. `gnome-keyring`) first.

Add room mappings and the DM allowlist to `~/.config/mynd/config.toml`:

```toml
[matrix]
homeserver_url = "https://matrix.org"      # written automatically by `matrix login`
user_id = "@mynd-bot:matrix.org"       # written automatically by `matrix login`
allowed_users = ["@you:matrix.org"]        # required for DMs — anyone else is ignored

[[matrix.rooms]]
room_id = "!abc123:matrix.org"
alias = "mynd-project"                 # optional, for `mynd matrix status`
base_tags = ["project:mynd"]
```

Rooms the bot is in but not listed here still work — memories land in the `workspace`
layer tagged `room:<id-or-alias>` + `source:matrix` instead of your configured
`base_tags`. DMs always use the `personal` layer.

Then run it:

```sh
mynd matrix run
```

Or install it as a background service alongside `mynd up` — `mynd service
install` automatically adds a second unit once `[matrix]` is configured.

### Using it

- Mention the bot in a mapped/unmapped room, or just message it directly in a DM.
- `!hm store <text>` — direct write, skips the agent (fast, no interpretation).
- `!hm reset` — starts a fresh conversation in that room (drops continuity, not memory).
- `mynd matrix status` — shows login state, sync status, and per-room session
  activity.

### Agent compatibility

Claude Code works out of the box (same `[agent]` config as the dashboard's suggest
flow). OpenCode needs one manual step first: OpenCode's non-interactive CLI has no
per-invocation MCP tool allowlist, so you must pre-create a restricted agent profile
named `mynd-bot` in your `opencode.json`, scoped to the mynd MCP tools — the
bot spawns `opencode run --agent mynd-bot`, it doesn't configure that profile for
you.

---

## Discord chat interface (optional)

Capture and recall HiveMind memories from a Discord channel or DM — mention the bot in a
channel, use the `/hm` slash command, or DM it directly. Same headless-agent mechanism as
Matrix and the dashboard's suggest flow: no bespoke NLU, no local model.

This is a separate process from `hivemind up` and doesn't depend on it being started —
each message/command spawns a short-lived agent turn that talks to HiveMind the same way
any other MCP client does.

### Setup

Create a bot application in the [Discord Developer Portal](https://discord.com/developers/applications),
enable the **Message Content Intent** under Bot settings, and invite it to your server with
the `bot` and `applications.commands` OAuth scopes. Then:

```sh
hivemind discord login
```

Prompts for the bot token. The token is validated against Discord once, then persisted to
your OS keyring (Secret Service/kwallet on Linux, Keychain on macOS) — the same storage
Matrix uses.

> **Headless Linux servers:** `keyring` needs a functioning Secret Service (D-Bus). A
> bare VPS with no login session running may not have one available; `hivemind discord
> login` will fail with an actionable message if so. Install/start a Secret Service
> provider (e.g. `gnome-keyring`) first.

Add channel mappings and the DM allowlist to `~/.config/hivemind/config.toml`:

```toml
[discord]
application_id = "123456789012345678"      # written automatically by `discord login`
allowed_users = ["111111111111111111"]     # required for DMs — anyone else is ignored
permission_gate = "manage_guild"           # optional; restricts who can invoke /hm in a guild

[[discord.channels]]
channel_id = "222222222222222222"
alias = "hivemind-project"                 # optional, for `hivemind discord status`
base_tags = ["project:hivemind"]
```

Channels the bot is in but not listed here still work — memories land in the `workspace`
layer tagged `channel:<id-or-alias>` + `source:discord` instead of your configured
`base_tags`. DMs always use the `personal` layer.

> **Trust boundary:** `permission_gate` only restricts the `/hm` slash command. Freeform
> @mention chat in a guild channel is open to any member of that guild who can see the
> channel: it's gated by Discord's own channel permissions, not by `permission_gate` or
> `allowed_users` (`allowed_users` only gates DMs). This mirrors Matrix's trust model,
> where room membership is the boundary, but Discord guilds are typically much larger
> than Matrix rooms, so make sure you're comfortable with everyone in a guild before
> inviting the bot to it.

Then run it:

```sh
hivemind discord run
```

Or install it as a background service alongside `hivemind up` — `hivemind service
install --discord` adds a unit once `[discord]` is configured.

### Using it

- Mention the bot in a channel, or message it directly in a DM, for freeform chat.
- `/hm store text:<text>` — direct write, skips the agent (fast, no interpretation).
- `/hm reset` — starts a fresh conversation in that channel (drops continuity, not memory).
- `/hm help` — lists these commands.
- `hivemind discord status` — shows login state, sync status, and per-channel session
  activity.

### Agent compatibility

Same as Matrix — see [Agent compatibility](#agent-compatibility) above.

---

## Checking your setup

```sh
mynd status
```

Shows the active config, memory count, database path, and a preview of exactly what will be injected at the next session start, including token usage vs budget.

---

## Troubleshooting

### "hint: looks like you haven't run `mynd init` yet"

You ran `mynd up` or `mynd status` before initializing. Run `mynd init` in your project directory first:

```sh
cd ~/projects/myapp
mynd init
```

This creates `.mynd.toml`, scaffolds CLAUDE.md, and writes the global config file that makes the hint go away.

### "hint: no AI client is registered with Mynd yet"

You ran `mynd init` but haven't told your AI client about the MCP server yet. The server will start, but your AI client won't connect to it. Run the install command for your client once:

```sh
mynd mcp install claude      # Claude Code
mynd mcp install cursor      # Cursor
mynd mcp install windsurf    # Windsurf
mynd mcp install opencode    # OpenCode
mynd mcp install kimi        # Kimi Code CLI
mynd mcp install codex       # OpenAI Codex CLI
```

This only needs to be done once per machine, not per project.

**How Mynd detects whether a client is registered:**

| Client | Detection method |
|--------|-----------------|
| Claude Code | Reads `~/.claude/mcp.json`, `~/.claude/settings.json`, and `~/.claude.json` (user-scope registrations), checks for "mynd" |
| Cursor | Reads `~/.cursor/mcp.json`, checks for "mynd" |
| Windsurf | Reads `~/.codeium/windsurf/mcp_config.json`, checks for "mynd" |
| Kimi | Reads `~/.kimi/mcp.json`, checks for "mynd" |
| OpenCode | Reads `~/.config/opencode/opencode.json` (or `$XDG_CONFIG_HOME`), checks for "mynd" |
| Codex CLI | Reads `~/.codex/config.toml`, checks for `[mcp_servers.mynd]` |

Detection failures are silent: a missing config file or unavailable CLI simply means "not registered." If you've registered a client manually and still see the hint, verify that "mynd" appears in the config file at the path listed above.

### Claude connects but session start fails

If `mynd_session_start` errors during a session, the most likely causes are:

- **`mynd` not found in PATH**: verify with `which mynd`. If you installed via `cargo install`, make sure `~/.cargo/bin` is in your PATH.
- **Database error**: check `MYND_DB_PATH` and ensure the directory is writable.
- **Corrupt config**: run `mynd status` in the project directory to validate `.mynd.toml`.
- **Recalls with special characters**: recall titles containing FTS special characters (`/`, `+`, `-`, quotes) no longer fail the whole call; unmatched entries are simply reported as `not_found` in the result.

### Session start succeeds but no memories are injected

`mynd_session_start` loads only the entries listed in `[hooks.on_session_start].recalls` in `.mynd.toml`. If that list is empty or no entries match titles in the database, nothing is injected. Check with:

```sh
mynd status    # previews exactly what would be injected
```

---

## FAQ

**Does Mynd inject memories into every prompt I send?**

No. Memories are injected once, when Claude calls `mynd_session_start` at the start of the session. After that, the loaded memories are part of the conversation context, but nothing extra is added per prompt. Tools like `UserPromptSubmit` hooks in `.claude/settings.json` run on every message; Mynd does not.

**What's the difference between Mynd's session start and a Claude Code `UserPromptSubmit` hook?**

A `UserPromptSubmit` hook runs a shell command and appends its output to every message you send, unconditionally on every prompt, with no token budget. Mynd runs once per session, respects a `max_tokens` cap, and gives you per-project control over exactly which memories to load. See the [comparison table](#how-mynd-differs-from-claude-codes-built-in-hooks) for the full breakdown.

**Can I fetch memories that aren't listed in `recalls`?**

Yes. `recalls` is only the auto-inject list for session start. Every memory in the database is available on demand at any time. Ask Claude to recall it by title or ID (`memory_recall`), or search by keyword (`memory_search`). Nothing is hidden or inaccessible.

**Does Claude store memories automatically as we chat?**

No. Mynd never auto-stores. Claude only writes a memory when you explicitly ask it to, such as *"remember this"* or *"store that preference"*. This keeps the store intentional and free of noise.

**What happens if a memory doesn't fit within `max_tokens`?**

It gets skipped. Mynd loads recalls in order; if an entry would push past the budget, it skips that entry and continues with the next one; a later, smaller entry can still fit. Skipped entries are reported in the result. Use `mynd status` to preview what would be loaded and how many tokens it costs before opening a session.

**Can I have different recalls per project?**

Yes. Each project has its own `.mynd.toml` with its own `recalls` list and `max_tokens`. Your personal additions go in `.mynd.local.toml` (gitignored), which stacks on top of the project config.

**Do my teammates see my personal memories?**

No. Memories stored with `layer = "personal"` follow you, not the repo. Only `layer = "workspace"` memories are project-scoped. The `memory_store` MCP tool accepts `layer: "personal" | "workspace" | "org"` (default `workspace`), and the dashboard filters by layer. The `.mynd.local.toml` file is gitignored, and your personal layer is local to your machine unless you configure sync. See [Org layer](#org-layer-optional) for the third, separately-configured layer.

**Is the MCP connection authenticated?**

The MCP endpoint (`/mcp`) and the REST API (`/api/v1/*`) are unauthenticated and bind to `127.0.0.1` by default, so only processes on your local machine can reach them. The `api_key` under `[sync]` is your auth token for the remote sync target (sqld token for self-hosted, account key for Oxhive hosted); it is used only during replication and has nothing to do with Claude's connection to Mynd.

**Can I use Mynd with agents other than Claude Code?**

Yes, as long as the agent supports MCP over stdio. Register it the same way you would any local stdio MCP server, pointing it at the `mynd` binary. If your client only supports HTTP transport, run `mynd up` to start the HTTP server and connect to `http://127.0.0.1:3456/mcp`. The REST API is also fully accessible for custom integrations.

**Where is the database stored?**

`~/.local/share/mynd/memories.db` by default (or `$XDG_DATA_HOME/mynd/memories.db` if `XDG_DATA_HOME` is set). Override with the `MYND_DB_PATH` environment variable. It's a plain SQLite file; you can back it up, copy it between machines, or inspect it directly. Databases from versions before 0.3.x lived at `~/.hivemind/memories.db`; run `mynd migrate` to move them.

---

## Integrating with Mynd

Detailed docs for connecting your own app, script, or AI agent to Mynd's MCP tools and REST API: [docs/INTEGRATING.md](docs/INTEGRATING.md)

---

## License

AGPL-3.0-only
