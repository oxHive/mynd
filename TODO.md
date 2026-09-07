# Rename: `hivemind` -> `mynd`

Catalog of everything still carrying the old name. Grouped by area. Each item lists
what to change and the files involved. Historical docs (`docs/superpowers/**`) are
listed last and are optional.

## Decisions (settled)

- [x] New binary name: `hivemind` -> `mynd`
- [x] New crate name: `oxhivemind` -> `oxmynd` (crates.io publish name + lib name)
- [x] Rust identifiers: `HiveMind` -> `Mynd`, `HiveMindConfig` -> `MyndConfig`
- [x] Session-start context tag: `<hivemind-context>` -> `<mynd-context>`
- [x] Back-compat: **hard cut**, no `hivemind` alias binary
- [ ] New project config file: `.hivemind.toml` / `.hivemind.local.toml` -> `.mynd.toml` / `.mynd.local.toml`?
- [ ] New global config dir: `~/.config/hivemind` / `$XDG_CONFIG_HOME/hivemind` -> `mynd`?
- [ ] New data dir: `~/.local/share/hivemind` / `$XDG_DATA_HOME/hivemind` -> `mynd`?
- [ ] New env var prefix: `HIVEMIND_*` -> `MYND_*`?
- [ ] New MCP tool name: `hivemind_session_start` -> `mynd_session_start`? (breaking for existing CLAUDE.md installs)
- [ ] New MCP server key registered in clients: `hivemind` -> `mynd`?
- [ ] Migration story: detect old config dir, old data dir, old `.hivemind.toml`,
      old MCP registration, old SessionStart hook (`hivemind session-start`), old
      `~/.claude/CLAUDE.md` block; auto-migrate or warn. `mynd migrate` already
      exists for the pre-0.3 `~/.hivemind` path; extend it.

## Progress

- **DONE:** Crate / binary / packaging + CLI command surface (see sections below).
  `cargo build` green.
- **Interim mismatch introduced on purpose:** `mynd session-start` now emits
  `<mynd-context>`, but the deferred `GLOBAL_CLAUDE_BLOCK` in `src/cli/init.rs`
  still tells Claude to look for `<hivemind-context>` and call `hivemind_session_start`.
  Effect: on machines with the old `~/.claude/CLAUDE.md`, Claude may redundantly
  call the MCP tool at session start (wasteful, not broken). Fixed when the
  "MCP tool + session-start" section is done - that pass must sync the block to
  `<mynd-context>` and add the `~/.claude/CLAUDE.md` migration.
- **Still `hivemind` on purpose (deferred, with NOTE comments in the code):**
  - MCP server registration key + client-config detection tokens (`src/cli/mcp_install.rs`, `src/cli/init.rs`)
  - systemd unit basenames, launchd labels, `hivemind.log` (`src/cli/service.rs`)
  - `hivemind_session_start` MCP tool + `# HiveMind Memory System` global block (`src/server.rs`, `src/cli/init.rs`)
  - `.hivemind.toml` / `.hivemind.local.toml` / `.hivemind-tmp`, config + data dirs
  - `HIVEMIND_*` env vars, `HIVEMIND_GIT_SHA` / `HIVEMIND_IS_TAGGED` in `build.rs`
  - pidfiles / sockets, keyring service `hivemind-matrix`
- **Old SessionStart hooks break under the hard cut:** `.claude/settings.json`
  entries running `hivemind session-start` point at a binary that no longer
  exists. `ensure_claude_settings_hook` now writes `mynd session-start` and
  dedupes against both spellings, but existing hooks need `mynd init` re-run or
  the migration above.

## Crate / binary / packaging  — DONE

- [x] `Cargo.toml` - name `oxmynd`, repository `oxhive/mynd`, binstall `pkg-url` `mynd-{target}.tar.gz`, `bin-dir = "mynd"`, `[[bin]] name = "mynd"`, `[lib] name = "oxmynd"`
- [x] `Cargo.lock` - regenerated to `oxmynd`
- [x] `src/main.rs` - `use oxmynd::...`, `use server::Mynd`, tracing targets `mynd=...,oxmynd=...`
- [x] all `oxhivemind::` -> `oxmynd::` (`src/main.rs`, `tests/api_integration.rs`)
- [x] `src/server.rs` - `struct Mynd`, all `impl Mynd`, `ServerHandler for Mynd`, server identity `"mynd"`
- [x] `src/config.rs`, `src/session.rs` - `MyndConfig`
- [x] `src/http.rs` - `server::Mynd`, local var `mynd`
- [x] `src/dashboard_placeholder.html` - `<title>Mynd</title>`, `cargo binstall oxmynd`
- [x] `src/update.rs` - user-agent `mynd/{version}`, release URLs `oxhive/mynd`, "restart mynd" text
- [x] `src/tui/header.rs` - brand string `Mynd`
- [ ] `README.md` - `cargo binstall oxmynd`, `cargo install oxmynd`, crates.io badges, all `git clone .../hivemind` (deferred to Docs pass)
- NOTE: `HIVEMIND_GIT_SHA` / `HIVEMIND_IS_TAGGED` in `build.rs` left as-is (env-var pass).

## CLI command surface  — DONE

- [x] `src/cli/mod.rs` - clap `name = "mynd"`, `about`, `mynd matrix login`. (`~/.hivemind` legacy path kept.)
- [x] `src/cli/init.rs` - all `println!` -> `mynd ...`, SessionStart hook writes `mynd session-start` + dedupes both spellings, "re-run mynd init". Kept: `.hivemind.toml*` scaffolding, `.hivemind-tmp`, MCP detection tokens, `GLOBAL_CLAUDE_*` (reverted to HiveMind + NOTE comment).
- [x] `src/cli/mcp_install.rs` - exe fallback `"mynd"`, `mynd mcp install claude` bail text, `Mynd` prose. Kept + NOTE: MCP registration key `"hivemind"` and detection tokens.
- [x] `src/cli/status.rs` - hints `mynd init` / `mynd mcp install`, `<mynd-context>` tag emission, `mynd: skipped recall`, `(mynd up)` / `(mynd matrix run)`. Kept: `.hivemind.toml*` lookups.
- [x] `src/cli/service.rs` - bail + println command refs `mynd service ...`, test binary path `/usr/local/bin/mynd`. Kept + NOTE: unit basenames, launchd labels, `hivemind.log`.
- [x] `src/cli/matrix_cmds.rs` - `mynd matrix run/status`, example id `@mynd-bot:matrix.org`, device name `Mynd bot`.
- [x] `src/cli/tests.rs` - `["mynd", ...]` argv, `<mynd-context>` + fn name, `mynd session-start` / `mynd init` / `(mynd up)` asserts, `Mynd v` header. Kept: `# HiveMind` block asserts, MCP-key fixtures, `.hivemind.toml*`, `hivemind/config.toml` dir.

## Config files & paths

- [ ] `.hivemind.toml` (repo root) - rename file; `name = "hivemind"` inside; commented example `"project/hivemind"`
- [ ] `src/config.rs:301,341,366` - `.hivemind.toml` / `.hivemind.local.toml` filename constants
- [ ] `src/config.rs:310,315` - global config dir: `$XDG_CONFIG_HOME/hivemind`, `~/.config/hivemind`
- [ ] `src/config.rs:325` - error string "no .hivemind.toml found"
- [ ] `src/cli/init.rs:160-164` - scaffolds `.hivemind.toml`, `.hivemind.local.toml`, adds `.hivemind.local.toml` to `.gitignore`
- [ ] `src/cli/init.rs:299` - `LOCAL_TOML` template comment "additive on top of .hivemind.toml"
- [ ] `src/db.rs:6-35` - data dir `$XDG_DATA_HOME/hivemind` / `~/.local/share/hivemind`; legacy `~/.hivemind`; pidfiles `hivemind.pid`, `hivemind-matrix.pid`
- [ ] `src/http.rs:345` - detached log `hivemind.detached.log`
- [ ] `src/matrix/status.rs:33` - socket `hivemind-matrix.sock`
- [ ] `.gitignore:3,5,9-10` - `/docs/HIVEMIND_*.md`, `.hivemind.local.toml`, `plugins/opencode/hivemind.js`, `plugins/opencode/hivemind.d.ts`

## Environment variables (`HIVEMIND_*`)

- [ ] `build.rs:20-21` - `HIVEMIND_GIT_SHA`, `HIVEMIND_IS_TAGGED` (also every `env!("HIVEMIND_GIT_SHA")` consumer)
- [ ] `src/db.rs:39` - `HIVEMIND_DB_PATH`
- [ ] `src/update.rs:76` - `HIVEMIND_UPDATE_CHECK_URL`
- [x] `src/main.rs` + `src/update.rs` - tracing target now `mynd=...,oxmynd=...`, user-agent `mynd/{version}` (done in CLI/packaging pass)
- [ ] dashboard `window.HIVEMIND_API` (see Dashboard section) - JS global, not an env var, but same rename

## MCP tool + server integration

- [ ] `src/server.rs:751` - `async fn hivemind_session_start` tool name (breaking; existing `~/.claude/CLAUDE.md` blocks call it by name)
- [ ] `src/server.rs:749` - tool description mentions `.hivemind.toml`
- [ ] `src/cli/init.rs:325-354` - `GLOBAL_CLAUDE_MARKER = "# HiveMind Memory System"`, `GLOBAL_CLAUDE_BLOCK` (full text: `hivemind_session_start`, `.hivemind.toml`, `hivemind status`, `hivemind init`, `<hivemind-context>`)
- [ ] `src/cli/init.rs:307` - per-project `CLAUDE.md` template ("# HiveMind - {name}", "per .hivemind.toml")
- [x] `src/cli/init.rs` - SessionStart hook command now writes `mynd session-start`; dedupe checks both spellings (done in CLI pass)
- [ ] `CLAUDE.md` (repo root) - "# HiveMind - hivemind", ".hivemind.toml"
- [ ] `~/.claude/CLAUDE.md` on user machines - migration needed (can't edit remotely; `mynd init` re-run or `migrate` should rewrite the block). **Must also flip `<hivemind-context>` -> `<mynd-context>` in the block** since the emitter already changed (see Progress).

## Session-start context tag  — mostly DONE

- [x] Emitter `src/cli/status.rs` now prints `<mynd-context>`; the `mynd: skipped recall` prefix; `src/cli/tests.rs` asserts updated.
- [ ] `GLOBAL_CLAUDE_BLOCK` reference to `<hivemind-context>` in `src/cli/init.rs:357` still old - flip it in the MCP-tool pass together with the `~/.claude/CLAUDE.md` migration.

## Claude plugin (`.claude-plugin/`)

- [ ] `.claude-plugin/plugin.json` - `name`, `description`, mcpServers key `hivemind`, `command: "hivemind"`
- [ ] `.claude-plugin/marketplace.json` - `name` (x2), `displayName`, `homepage`, `repository` (`github.com/oxHive/hivemind`)
- [ ] `README.md` - `claude plugin marketplace add oxHive/hivemind`, `claude plugin install hivemind@hivemind` (appears ~4x)

## Claude skills (`plugins/claude/skills/`)

- [ ] `memory-connections.md`, `memory-edit.md`, `memory-list.md`, `memory-search.md`, `memory-status.md`, `memory-store.md` - "HiveMind" in prose/descriptions; `hivemind_session_start` tool call in `memory-status.md:12`; `.hivemind.toml` refs in `memory-status.md:18,25,27` and `memory-store.md:22,60`

## OpenCode plugin (`plugins/opencode/`)

- [ ] `plugins/opencode/hivemind.ts` - rename file; `HIVEMIND_INSTRUCTIONS` const + full instruction text; `resolveHivemind()`, `hivemindBin`, `which hivemind`; `cfg.mcp.hivemind` registration key; toast `service: "hivemind"`; `cargo binstall oxhivemind`; `.hivemind.toml` check
- [ ] `plugins/opencode/package.json` - `name: "@oxhive/opencode-hivemind"`, `description`, `homepage`, `repository.url`, `bugs.url`, `main: "hivemind.js"`, `files`, `build` script (`hivemind.ts` -> `hivemind.js`)
- [ ] `plugins/opencode/scripts/resolve-skills.ts` - check for name refs
- [ ] `README.md:140,146-150` - `@oxhive/opencode-hivemind`

## Dashboard (`dashboard/`)

- [ ] `dashboard/package.json` + `dashboard/bun.lock` - `name: "hivemind-dashboard"`
- [ ] `dashboard/index.html:5,7` - `<title>HiveMind</title>`, `window.HIVEMIND_API=undefined` onerror fallback
- [ ] `dashboard/src/App.vue:36,103,110` - `window.HIVEMIND_API`, "Connecting to HiveMind server", "Run `hivemind up`"
- [ ] `dashboard/src/api/client.js:1` - `window.HIVEMIND_API`
- [ ] `src/http.rs:73,569` - server emits `window.HIVEMIND_API = ...` in `config.js`
- [ ] localStorage keys (breaking - users lose UI state, consider migration):
  - `dashboard/src/stores/theme.js:4` - `hivemind.theme`
  - `dashboard/src/stores/fontScale.js:4` - `hivemind.fontScale`
  - `dashboard/src/stores/memories.js:5` - `hivemind.memories.drafts`
  - `dashboard/src/components/graph/GraphCanvas.vue:22,39` - `hivemind.graph.camera`, `hivemind.graph.pinned`
- [ ] `dashboard/src/components/settings/DataSection.vue:16,52` - export filename `hivemind-export-*.json`, "HiveMind export" text
- [ ] `dashboard/src/components/settings/TagsSection.vue:20,124` - "global hivemind config" prose
- [ ] `dashboard/src/stores/tagSettings.js:8,14` - "built into HiveMind" / "global hivemind config" comments
- [ ] `dashboard/src/components/sidebar/AppSidebar.vue:84` - "HiveMind" brand text
- [ ] `dashboard/src/views/AnalyticsView.vue:83` - "HiveMind configured" hint
- [ ] `dashboard/src/style.css:258` - "the HiveMind mark" comment

## Service / daemon (systemd, launchd, keyring)

- [ ] systemd unit basenames `hivemind` / `hivemind-matrix` -> service file `~/.config/systemd/user/hivemind.service` (`src/cli/service.rs`)
- [ ] launchd labels `com.oxhive.hivemind` / `com.oxhive.hivemind-matrix` -> plist `~/Library/LaunchAgents/com.oxhive.hivemind.plist`
- [ ] logs `~/Library/Logs/hivemind.log`
- [ ] `src/matrix/keyring_store.rs:13,19,28` - keyring service name `"hivemind-matrix"` (breaking - existing saved Matrix sessions become unreachable; migration or re-login)
- [ ] `src/cli/service.rs` tests (`:228-248`) - assertion strings

## Matrix bot

- [ ] `src/matrix/agent.rs` - mcp-config key `hivemind` (`:43`), allowed tools `mcp__hivemind__memory_*` (`:62`), opencode agent profile `hivemind-bot` (`:97`), `hivemind_bin` param name
- [ ] `src/suggest_session.rs:245,261,280-294` - mcp key `hivemind`, `mcp__hivemind__*` tools, opencode agent `hivemind-suggest`
- [ ] `src/matrix/daemon.rs` - `hivemind_bin` param, user-facing error strings ("hivemind matrix failed to store", "hivemind matrix hit an error")
- [ ] `src/matrix/store_direct.rs:19-33` - "Spawns `hivemind`", error strings
- [ ] `src/main.rs:173,176-178,186` - "run `hivemind matrix login` first", `hivemind_bin`
- [ ] `README.md` - opencode `hivemind-bot` agent profile setup, `~/.config/hivemind/config.toml` Matrix examples, `@hivemind-bot:matrix.org`, `alias = "hivemind-project"`
- [ ] Default/example tags `project:hivemind` in `src/matrix/rooms.rs`, `src/matrix/daemon.rs:401` - test/example data, low priority

## CI / workflows

- [ ] `.github/workflows/publish.yml:17-18` - `package-name: oxhivemind`, `binary-name: hivemind`
- [ ] `cliff.toml:7` - changelog commit link `github.com/oxHive/hivemind`
- [ ] Check other `.github/workflows/*.yml` for repo slug / badge refs after repo is renamed on GitHub
- [ ] `README.md` badge URLs - CI, codecov, GitHub release all point at `oxhive/hivemind`

## just recipes

- [ ] `.justfile:34,39` - `hivemind mcp install {{client}}`
- [ ] `recipes/testenv.just:2-14` - comments + `hivemind up` / `hivemind status` invocations, `~/.config/hivemind`, `~/.local/share/hivemind`, `.hivemind.toml`

## Docs

- [ ] `README.md` - 160 occurrences. Full pass: install commands, all `hivemind <cmd>` examples, `.hivemind.toml` / `.hivemind.local.toml` / `~/.config/hivemind/config.toml`, `$XDG_CONFIG_HOME/hivemind`, `HIVEMIND_DB_PATH`, `~/.local/share/hivemind`, `~/.hivemind` legacy, plugin install, client-detection table, service paths, FAQ
- [ ] `PRODUCT.md:9,13,21` - "HiveMind" product name in prose
- [ ] `docs/INTEGRATING.md` - "# Integrating with HiveMind", `hivemind up`, `hivemind_session_start`, `.hivemind.toml`, `~/.config/hivemind/config.toml`, `/home/user/.hivemind/memories.db`, `HIVEMIND` python var, `hivemind mcp install`
- [ ] `docs/api/bruno.json:3` - `"name": "HiveMind API"`
- [ ] `docs/api/settings/save-sync-settings.bru:16,26` - `remote-hivemind` example host, "Edit ~/.config/hivemind/config.toml and restart hivemind"

### Historical (optional - leave as-is unless doing a full sweep)

- [ ] `docs/superpowers/plans/2026-06-11-hivemind-phase1.md` - filename + heavy internal use (`HiveMind` struct, `oxhivemind`, `HIVEMIND_DB_PATH`, paths)
- [ ] `docs/superpowers/specs/2026-06-14-hivemind-dashboard-design.md` - filename
- [ ] `docs/superpowers/plans/2026-07-13-boolean-tag-recall.md`, `2026-07-13-tag-namespace-system.md` and matching specs - `.hivemind.toml`, `project:hivemind` examples, `HiveMindConfig`
- [ ] Other `docs/superpowers/**` files - scattered `project:hivemind` tag examples in test snippets
