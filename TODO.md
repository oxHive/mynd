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

- **All sections DONE.** Crate / binary / packaging; CLI; config files & paths;
  env vars; service / daemon units; MCP tool + server; Claude plugin; Claude
  skills; OpenCode plugin; dashboard; Matrix bot; CI; just recipes; docs.
  `cargo build` / `cargo test` / `cargo clippy` green; `dashboard bun run build`
  and `plugins/opencode bun run build` green. Only `docs/superpowers/**`
  historical records left untouched by design.
- The earlier `<mynd-context>` / `<hivemind-context>` interim mismatch is **resolved**:
  the global CLAUDE block now matches the emitter, and `migrate_global_claude_block`
  rewrites the pre-rename block in `~/.claude/CLAUDE.md` in place (via `mynd init`
  and unconditionally at startup).
- **Still `hivemind` on purpose (deferred, with NOTE comments in the code):**
  - MCP registration key detection still *also* matches `hivemind` (transition), and `install` drops it
  - `HIVEMIND_DB_PATH` env fallback (deliberate), `~/.hivemind` pre-0.3 constant (migration source), `test_hivemind()` helper
  - `hivemind` systemd unit / launchd / keyring names kept only for legacy teardown + forward-migration
  - `hivemind-suggest` / `hivemind-bot` opencode agent-profile names + `hivemind_bin` var + daemon/store_direct strings (Matrix pass)
  - `project:hivemind` tag fixtures in tests (tag-system fixtures, not the product name)
- **Upgrade steps still required under the hard cut** (migrations cover config dir,
  data dir, `~/.claude/CLAUDE.md` block, keyring, service units - not these):
  - re-run `mynd init` so `.claude/settings.json` runs `mynd session-start` (old hook points at a gone binary)
  - re-run `mynd mcp install <client>` so the MCP server is registered under the `mynd` key

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

## Config files & paths  — DONE

Migration is **hybrid**: `src/dir_migrate.rs` relocates the global config dir and
the data dir (`~/.config/hivemind` -> `mynd`, `~/.local/share/hivemind` -> `mynd`,
`$XDG_*` variants, and the pre-0.3 `~/.hivemind/memories.db`) on startup - wired
at the top of `main()` via `run_startup_migration()`, prints one stderr line per
move, skipped entirely when `HIVEMIND_DB_PATH` is set. Project config is
**fallback-read**: `discover_project_root` / `load_config_with_global` accept
`.mynd.toml` or `.hivemind.toml` (and `.mynd.local.toml` / `.hivemind.local.toml`),
preferring the new name. `PROJECT_CONFIG_NAMES` / `PROJECT_LOCAL_CONFIG_NAMES`
constants in `config.rs`.

- [x] `.hivemind.toml` (repo root) - `git mv` -> `.mynd.toml`, `name = "mynd"`, commented example `project/mynd`
- [x] `CLAUDE.md` (repo root) - `# Mynd — mynd`, `per .mynd.toml`
- [x] `src/config.rs` - `global_config_dir()` -> `mynd` + `legacy_global_config_dir()`; project-config fallback-read; error string
- [x] `src/db.rs` - `xdg_data_dir()` -> `mynd` + `legacy_xdg_data_dir()`; pidfiles `mynd.pid` / `mynd-matrix.pid`; `resolve_db_path` default
- [x] `src/dir_migrate.rs` - **new module** (`relocate_dir`, `relocate_legacy_db_file`, `run_startup_migration`), TDD, 5 tests
- [x] `src/main.rs` - calls `run_startup_migration()` before dispatch
- [x] `src/cli/init.rs` - scaffolds `.mynd.toml` / `.mynd.local.toml`, `.gitignore` line, `.mynd-tmp` suffix, `LOCAL_TOML` + `project_claude_md` templates
- [x] `src/cli/status.rs`, `src/tui/status_view.rs` - "No .mynd.toml found", "Config: .mynd.toml", local-config detection via fallback constant
- [x] `src/http.rs` - detached log `mynd.detached.log`
- [x] `src/matrix/status.rs` - socket `mynd-matrix.sock`
- [x] `src/api/settings.rs` - "restart mynd" / "global mynd config" messages
- [x] `src/tui/header.rs` - test fixture `db_path` / project label
- [x] `.gitignore` - `/docs/MYND_*.md`, `.mynd.local.toml`
- [x] `recipes/testenv.just`, `.justfile`, `docs/api/settings/save-sync-settings.bru` - paths + command names
- NOTE kept as `hivemind` on purpose: `~/.hivemind` legacy path constant + its test (pre-0.3 migration source), `HIVEMIND_DB_PATH` env (env-var pass), `test_hivemind()` test helper name, `plugins/opencode/hivemind.{js,d.ts}` gitignore lines (plugin pass), MCP tool description's `.hivemind.toml` mention (fallback still reads it; synced in MCP pass).
- NOTE `src/cli/status.rs` "Config: .mynd.toml" is hardcoded - a not-yet-migrated repo with `.hivemind.toml` shows the new name in `mynd status`. Cosmetic, transitional.

## Environment variables  — DONE

- [x] `build.rs` - `MYND_GIT_SHA`, `MYND_IS_TAGGED` (compile-time `rustc-env`; no consumers today)
- [x] `src/db.rs` - `db_path_override()`: `MYND_DB_PATH`, falls back to `HIVEMIND_DB_PATH`. Auto-migration checks the same. TDD, 3 new tests.
- [x] `src/update.rs` - `MYND_UPDATE_CHECK_URL` (internal test/E2E knob, no fallback)
- [x] `src/main.rs` + `src/update.rs` - tracing target `mynd=...,oxmynd=...`, user-agent `mynd/{version}` (done in pass 1)
- [x] `src/test_env_lock.rs` - doc comment
- [ ] dashboard `window.HIVEMIND_API` - JS global, not an env var; Dashboard pass
- NOTE `HIVEMIND_DB_PATH` still honoured as a fallback on purpose (shell profiles / service units); drop it a release cycle later.

## Service / daemon units  — DONE

- [x] `src/cli/service.rs` - systemd units `mynd` / `mynd-matrix`, launchd labels `dev.oxhive.mynd` / `dev.oxhive.mynd-matrix` (reverse-DNS of oxhive.dev; the old code's `com.oxhive.*` was wrong), log `mynd.log`. `CURRENT_UNIT` / `LEGACY_UNITS` / `LEGACY_LAUNCH_AGENT_LABELS` constants.
- [x] `remove_legacy_units_{linux,macos}()` - `mynd service install` / `uninstall` tear down any `hivemind` unit/agent left by an older build (best-effort), so an upgraded machine never runs two competing services.
- [x] `src/matrix/keyring_store.rs` - keyring service `mynd-matrix`; `load` falls back to `hivemind-matrix` and migrates it forward (re-save + delete old), so an existing Matrix login survives the upgrade. `delete` clears both.
- [x] `src/matrix/daemon.rs` - doc comments (`mynd up` / `mynd matrix send`)
- NOTE real systemd/launchd/keyring paths are shell-outs; covered by the existing `systemd_unit_content` tests + a new constant-pinning test, not by end-to-end unit tests.

## MCP tool + server integration  — DONE

- [x] `src/server.rs` - tool `hivemind_session_start` -> `mynd_session_start` (fn name drives the MCP tool name); description now says `.mynd.toml`
- [x] `src/cli/init.rs` - `GLOBAL_CLAUDE_MARKER` -> `# Mynd Memory System`; `GLOBAL_CLAUDE_BLOCK` fully rewritten (`mynd_session_start`, `.mynd.toml`, `mynd status`, `mynd init`, `<mynd-context>`)
- [x] `migrate_global_claude_block()` - rewrites the pre-rename block in `~/.claude/CLAUDE.md` in place, preserving surrounding user content. Runs via `scaffold` (`mynd init`) and unconditionally at startup (`run_startup_migration`). No-op when the current block is present or none is. TDD, 4 tests.
- [x] MCP registration key `hivemind` -> `mynd` (`SERVER_KEY` / `LEGACY_KEY` in `mcp_install.rs`); `install` drops a stale `hivemind` registration first (CLI `mcp remove`, JSON key removal, `strip_toml_table` for codex); `already_registered()` + `detect_registered_clients` accept either key so an in-transition machine is not nagged. TDD, 3 tests.
- [x] `src/matrix/agent.rs`, `src/suggest_session.rs` - `mcpServers` key + `mcp__mynd__*` allowed-tools (must match the registration key)
- [x] `CLAUDE.md` (repo root) - done in pass 2
- [x] session-start context tag - `GLOBAL_CLAUDE_BLOCK` now says `<mynd-context>`, matching the emitter; interim mismatch resolved
- NOTE still `hivemind`: `hivemind-bot` / `hivemind-suggest` opencode agent-profile names + `hivemind_bin` var + daemon/store_direct strings (Matrix pass); MCP-key detection keeps matching `hivemind` for the transition.

## Claude plugin (`.claude-plugin/`)  — DONE

- [x] `.claude-plugin/plugin.json` - `name` `mynd`, description, mcpServers key `mynd`, `command: "mynd"`
- [x] `.claude-plugin/marketplace.json` - `name` x2, `displayName` `Mynd`, `homepage` / `repository` -> `github.com/oxHive/mynd`
- [ ] `README.md` - `claude plugin marketplace add oxHive/mynd`, `claude plugin install mynd@mynd` (Docs pass)

## Claude skills (`plugins/claude/skills/`)  — DONE

- [x] all six `memory-*.md` - `HiveMind` -> `Mynd`, `hivemind_session_start` -> `mynd_session_start`, `.hivemind.toml` -> `.mynd.toml`

## OpenCode plugin (`plugins/opencode/`)  — DONE

- [x] `plugins/opencode/hivemind.ts` -> `mynd.ts` (git mv); `MYND_INSTRUCTIONS` + full text, `resolveMynd()`, `myndBin`, `which mynd`, `cfg.mcp.mynd`, toast `service: "mynd"`, `cargo binstall oxmynd`, `.mynd.toml`
- [x] `plugins/opencode/package.json` - `name: "@oxhive/opencode-mynd"`, description, homepage / repo / bugs URLs, `main` / `files` / `build` -> `mynd.js` / `mynd.ts`
- [x] `.gitignore` - `plugins/opencode/mynd.{js,d.ts}`; stale `hivemind.{js,d.ts}` build artifacts deleted
- [ ] `README.md` - `@oxhive/opencode-mynd` (Docs pass)
- [ ] `plugins/opencode/scripts/resolve-skills.ts` - check for name refs
- [ ] `README.md:140,146-150` - `@oxhive/opencode-hivemind`

## Dashboard (`dashboard/`)  — DONE

- [x] `dashboard/package.json` + `dashboard/bun.lock` - `mynd-dashboard`
- [x] `dashboard/index.html` - `<title>Mynd</title>`, `window.MYND_API=undefined` fallback
- [x] `dashboard/src/App.vue` - `window.MYND_API`, "Connecting to Mynd server", "Run `mynd up`"
- [x] `dashboard/src/api/client.js` - `window.MYND_API`
- [x] `src/http.rs` - server emits `window.MYND_API = ...` in `config.js` + its test
- [x] localStorage keys -> `mynd.*`, with `dashboard/src/lib/localStore.js` `readStored()`
  helper that adopts (and clears) a `hivemind.*` value on first read so theme /
  fontScale / drafts / graph camera & pins survive the rename. Applied in
  `stores/theme.js`, `stores/fontScale.js`, `stores/memories.js`,
  `components/graph/GraphCanvas.vue`.
- [x] `DataSection.vue` export filename `mynd-export-*.json` + text
- [x] `TagsSection.vue`, `tagSettings.js`, `AppSidebar.vue`, `AnalyticsView.vue`, `style.css` - prose/brand
- [x] `dashboard bun run build` passes

## Matrix bot  — DONE

- [x] `src/matrix/agent.rs` - `mcpServers` key `mynd` + `mcp__mynd__*` (pass 4); `mynd_bin` param; opencode agent profile `mynd-bot`; test path literals `/usr/local/bin/mynd`
- [x] `src/suggest_session.rs` - `mynd` key + `mcp__mynd__*` (pass 4); opencode agent `mynd-suggest`; comment
- [x] `src/matrix/daemon.rs` - `mynd_bin` param, user-facing strings ("mynd matrix failed to store", "mynd matrix hit an error", "run `mynd matrix login` first")
- [x] `src/matrix/store_direct.rs` - "Spawns `mynd`", spawn/connect error strings
- [x] `src/main.rs` - `mynd_bin`, "run `mynd matrix login` first" (pass 1)
- [x] `README.md` - opencode `mynd-bot` profile, `~/.config/mynd/config.toml`, `@mynd-bot:matrix.org`, `alias = "mynd-project"`
- [x] example tags `project:hivemind` -> `project:mynd` in `matrix/rooms.rs`, `matrix/daemon.rs`, `matrix/status.rs`
- NOTE keyring `LEGACY_SERVICE = "hivemind-matrix"` kept for the login-survives-upgrade fallback (Service pass)

## CI / workflows  — DONE

- [x] `.github/workflows/publish.yml` - `package-name: oxmynd`, `binary-name: mynd`
- [x] `cliff.toml` - changelog commit link `github.com/oxHive/mynd`
- [x] `README.md` badge URLs - CI / codecov / crates / release -> `oxhive/mynd` / `oxmynd`
- NOTE other workflows (`pull-request.yml` etc) reference no repo slug; GitHub repo already renamed to `oxHive/mynd`

## just recipes  — DONE (pass 2)

- [x] `.justfile` - `mynd mcp install {{client}}`
- [x] `recipes/testenv.just` - paths + `mynd up` / `mynd status`

## Docs  — DONE

- [x] `README.md` - full pass: install (`cargo binstall oxmynd`), badges, every `mynd <cmd>`, `.mynd.toml*`, `~/.config/mynd`, `$XDG_*`, `MYND_DB_PATH`, `~/.local/share/mynd`, plugin install (`mynd@mynd`, `@oxhive/opencode-mynd`), detection table (`"mynd"`), service paths (`mynd.service`, `dev.oxhive.mynd.plist`, `mynd.log`), FAQ, `mynd_session_start`, `<mynd-context>`, `[mcp_servers.mynd]`. Kept: `~/.hivemind/memories.db` (pre-0.3 legacy path, 3 refs).
- [x] `PRODUCT.md` - "Mynd" product name
- [x] `docs/INTEGRATING.md` - full pass
- [x] `docs/api/bruno.json` - "Mynd API"
- [x] `docs/api/settings/save-sync-settings.bru` - done in pass 2

### Historical (left as-is)

- [ ] `docs/superpowers/**` plans + specs - historical records; filenames + internal `HiveMind`/`oxhivemind`/`.hivemind.toml` left untouched by design.

## Service / daemon (systemd, launchd, keyring)  — DONE (see "Service / daemon units" above)

- [x] systemd units -> `mynd` / `mynd-matrix` (`~/.config/systemd/user/mynd.service`)
- [x] launchd labels -> `dev.oxhive.mynd` / `dev.oxhive.mynd-matrix` (reverse-DNS of oxhive.dev)
- [x] logs -> `~/Library/Logs/mynd.log`
- [x] keyring service -> `mynd-matrix` with fallback-read + forward-migration of `hivemind-matrix`
- [x] legacy-unit teardown on install/uninstall; `service.rs` tests updated + constant-pin test added
- [ ] `README.md` service table (`~/.config/systemd/user/`, `~/Library/LaunchAgents/`, log path) - Docs pass

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
