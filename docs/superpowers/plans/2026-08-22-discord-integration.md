# Discord Chat Interface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Discord bot chat interface to hivemind with full functional parity to the existing Matrix integration (capture/recall memories via DM or a mentioned/slash command in a channel, backed by the same headless-agent mechanism).

**Architecture:** A new `src/discord/` module mirrors `src/matrix/`'s shape (daemon, token storage, login, status socket, channel-mapping, session TTL, direct-store fast path), adapted to Discord's bot-token auth and slash-command interaction model instead of Matrix's session-login and `!`-prefix text parsing. The one truly platform-agnostic piece of `src/matrix/` — the agent-CLI turn runner — is extracted into a new shared `src/chat_bot/` module used by both platforms. CLI, service-install, and status/TUI wiring are extended in place, following the exact patterns already used for `matrix`.

**Tech Stack:** Rust, `serenity` (Discord gateway/HTTP client), `keyring` (OS credential storage, already a dependency), `tokio`, existing `rmcp` MCP client machinery.

**Spec:** `docs/superpowers/specs/2026-08-22-discord-integration-design.md`

## Global Constraints

- Slash commands (`/hm store|reset|help`) are registered **globally**, not per-guild, at daemon startup.
- `permission_gate` is optional; unset means any guild member can invoke `/hm` (matches Matrix's existing "room membership is the trust boundary" default).
- DMs are always gated by `allowed_users` regardless of `permission_gate`.
- The bot token is the only secret; it lives solely in the OS keyring (service name `"hivemind-discord"`), never in the TOML config. `application_id` (not secret) lives in config.
- No on-disk gateway/cache store — Discord state is in-memory only, rebuilt each run.
- `commands.rs`, `rooms.rs`/`channels.rs`, `keyring_store.rs`/`token_store.rs`, `status.rs`, `daemon.rs`, `session.rs` stay duplicated-but-adapted per platform, not shared. Only the agent-CLI turn runner (`agent.rs`) is extracted into shared code.

---

## Task 1: Extract the shared agent-turn-runner into `src/chat_bot/agent.rs`

**Files:**
- Create: `src/chat_bot/mod.rs`
- Move: `src/matrix/agent.rs` → `src/chat_bot/agent.rs` (content unchanged — it has zero Matrix-type coupling today)
- Modify: `src/matrix/mod.rs:1` (remove `pub mod agent;`)
- Modify: `src/matrix/daemon.rs:355` (call `crate::chat_bot::agent::run_turn` instead of `crate::matrix::agent::run_turn`)
- Modify: `src/lib.rs` (add `pub mod chat_bot;`)

**Interfaces:**
- Produces: `chat_bot::agent::run_turn(agent: &AgentSettings, hivemind_bin: &str, prompt: &str, resume: Option<&str>, system_prompt: Option<&str>) -> Result<TurnResult, String>` and `chat_bot::agent::TurnResult { reply_text: String, session_id: String }` — used by both `matrix::daemon` and (from Task 12 onward) `discord::daemon`.

- [ ] **Step 1: Move the file with its history intact**

```bash
mkdir -p src/chat_bot
git mv src/matrix/agent.rs src/chat_bot/agent.rs
```

- [ ] **Step 2: Create the `chat_bot` module root**

`src/chat_bot/mod.rs`:
```rust
pub mod agent;
```

- [ ] **Step 3: Wire `chat_bot` into the crate root**

In `src/lib.rs`, alphabetically among the existing `pub mod` lines (after `budget`, before `cli`):
```rust
pub mod chat_bot;
```

- [ ] **Step 4: Remove `agent` from the matrix module**

In `src/matrix/mod.rs`, delete the line:
```rust
pub mod agent;
```

- [ ] **Step 5: Update the one call site**

In `src/matrix/daemon.rs:355`, change:
```rust
                    match crate::matrix::agent::run_turn(&agent, &hivemind_bin, &message, resume.as_deref(), Some(&system_prompt)).await {
```
to:
```rust
                    match crate::chat_bot::agent::run_turn(&agent, &hivemind_bin, &message, resume.as_deref(), Some(&system_prompt)).await {
```

- [ ] **Step 6: Verify the whole crate still compiles and the moved tests still pass**

Run: `cargo test chat_bot::agent:: matrix:: --lib`
Expected: PASS — all of `chat_bot::agent`'s existing tests (unchanged, just relocated) and `matrix::daemon`'s tests pass with no other regressions.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor: extract shared agent-turn-runner into src/chat_bot"
```

---

## Task 2: Add the `serenity` dependency

**Files:**
- Modify: `Cargo.toml:48` (after the `matrix-sdk` line)

**Interfaces:**
- Produces: the `serenity` crate available to `src/discord/*` from Task 4 onward.

- [ ] **Step 1: Add the dependency**

In `Cargo.toml`, in the `[dependencies]` block, after the `matrix-sdk` line:
```toml
serenity = { version = "0.12", default-features = false, features = ["client", "gateway", "cache", "model", "rustls_backend"] }
```

- [ ] **Step 2: Resolve and verify it builds**

Run: `cargo check`
Expected: succeeds, `Cargo.lock` gains `serenity` and its transitive dependencies.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "build: add serenity dependency for the Discord integration"
```

---

## Task 3: Add Discord config schema to `src/config.rs`

**Files:**
- Modify: `src/config.rs` (near the existing `RawMatrix`/`MatrixSettings`/`load_matrix_settings`/`write_matrix_login` definitions, and the `RawGlobal` struct)
- Test: `src/config.rs` (`#[cfg(test)] mod tests`, alongside the existing matrix config tests)

**Interfaces:**
- Produces:
  - `config::DiscordChannelMapping { channel_id: String, alias: Option<String>, base_tags: Vec<String> }`
  - `config::DiscordSettings { application_id: String, allowed_users: Vec<String>, permission_gate: Option<String>, channels: Vec<DiscordChannelMapping>, session_ttl_seconds: u64 }`
  - `config::load_discord_settings(global_path: &Path) -> Result<Option<DiscordSettings>>`
  - `config::write_discord_login(global_path: &Path, application_id: &str) -> Result<()>`
  - Reuses the existing `config::DEFAULT_SESSION_TTL_SECONDS` constant.

- [ ] **Step 1: Write the failing tests**

Add to `src/config.rs`'s `#[cfg(test)] mod tests`, right after the existing `matrix_settings_*`/`write_matrix_login_*` tests:

```rust
    #[test]
    fn discord_settings_none_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let s = load_discord_settings(&tmp.path().join("no-global.toml")).unwrap();
        assert!(s.is_none());
    }

    #[test]
    fn discord_settings_parses_full_config() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "config.toml",
            "[discord]\n\
             application_id=\"123456789012345678\"\n\
             allowed_users=[\"111111111111111111\"]\n\
             permission_gate=\"manage_guild\"\n\
             \n\
             [[discord.channels]]\n\
             channel_id=\"222222222222222222\"\n\
             alias=\"hivemind-project\"\n\
             base_tags=[\"project:hivemind\"]\n",
        );
        let s = load_discord_settings(&tmp.path().join("config.toml"))
            .unwrap()
            .expect("discord settings should be present");
        assert_eq!(s.application_id, "123456789012345678");
        assert_eq!(s.allowed_users, vec!["111111111111111111".to_string()]);
        assert_eq!(s.permission_gate, Some("manage_guild".to_string()));
        assert_eq!(s.channels.len(), 1);
        assert_eq!(s.channels[0].channel_id, "222222222222222222");
        assert_eq!(s.channels[0].alias, Some("hivemind-project".to_string()));
    }

    #[test]
    fn discord_settings_defaults_allowed_users_permission_gate_and_channels() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "config.toml",
            "[discord]\napplication_id=\"123456789012345678\"\n",
        );
        let s = load_discord_settings(&tmp.path().join("config.toml"))
            .unwrap()
            .unwrap();
        assert!(s.allowed_users.is_empty());
        assert_eq!(s.permission_gate, None);
        assert!(s.channels.is_empty());
        assert_eq!(s.session_ttl_seconds, DEFAULT_SESSION_TTL_SECONDS);
    }

    #[test]
    fn discord_settings_honors_configured_session_ttl() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "config.toml",
            "[discord]\napplication_id=\"123456789012345678\"\nsession_ttl_seconds=60\n",
        );
        let s = load_discord_settings(&tmp.path().join("config.toml"))
            .unwrap()
            .unwrap();
        assert_eq!(s.session_ttl_seconds, 60);
    }

    #[test]
    fn write_discord_login_creates_new_discord_section() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        write_discord_login(&path, "123456789012345678").unwrap();
        let s = load_discord_settings(&path).unwrap().unwrap();
        assert_eq!(s.application_id, "123456789012345678");
    }

    #[test]
    fn write_discord_login_preserves_other_sections_and_channel_mappings() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        write(
            tmp.path(),
            "config.toml",
            "[server]\nport=3456\n\
             [discord]\napplication_id=\"000000000000000000\"\n\
             allowed_users=[\"111111111111111111\"]\n\
             [[discord.channels]]\nchannel_id=\"222222222222222222\"\nbase_tags=[\"project:hivemind\"]\n",
        );
        write_discord_login(&path, "999999999999999999").unwrap();
        let s = load_discord_settings(&path).unwrap().unwrap();
        assert_eq!(s.application_id, "999999999999999999");
        assert_eq!(
            s.allowed_users,
            vec!["111111111111111111".to_string()]
        );
        assert_eq!(s.channels.len(), 1);
        assert_eq!(s.channels[0].channel_id, "222222222222222222");
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("[server]"));
        assert!(raw.contains("port = 3456"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail to compile**

Run: `cargo test config:: --lib`
Expected: FAIL — `load_discord_settings`/`write_discord_login`/`DiscordSettings` not found.

- [ ] **Step 3: Add the raw parsing structs and public types**

In `src/config.rs`, right after the existing `RawMatrixRoom` struct (around line 151):

```rust
#[derive(Debug, Default, Deserialize)]
struct RawDiscord {
    application_id: Option<String>,
    #[serde(default)]
    allowed_users: Vec<String>,
    permission_gate: Option<String>,
    #[serde(default)]
    channels: Vec<RawDiscordChannel>,
    session_ttl_seconds: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct RawDiscordChannel {
    channel_id: Option<String>,
    alias: Option<String>,
    #[serde(default)]
    base_tags: Vec<String>,
}
```

Add `discord: RawDiscord` to `RawGlobal` (around line 289), alongside the existing `matrix: RawMatrix`:

```rust
    #[serde(default)]
    discord: RawDiscord,
```

Right after the existing `MatrixSettings` struct (around line 268), before `DEFAULT_SESSION_TTL_SECONDS`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordChannelMapping {
    pub channel_id: String,
    pub alias: Option<String>,
    pub base_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordSettings {
    pub application_id: String,
    pub allowed_users: Vec<String>,
    /// One of `"manage_guild"`, `"administrator"`, `"manage_channels"`,
    /// `"manage_messages"`, `"kick_members"`, `"ban_members"` — or `None` to
    /// leave `/hm` open to every guild member (the default).
    pub permission_gate: Option<String>,
    pub channels: Vec<DiscordChannelMapping>,
    pub session_ttl_seconds: u64,
}
```

- [ ] **Step 4: Implement `load_discord_settings` and `write_discord_login`**

Right after `write_matrix_login`'s closing brace (around line 578):

```rust
pub fn load_discord_settings(global_path: &Path) -> Result<Option<DiscordSettings>> {
    if !global_path.is_file() {
        return Ok(None);
    }
    let raw: RawGlobal = toml::from_str(&std::fs::read_to_string(global_path)?)
        .with_context(|| format!("parsing {}", global_path.display()))?;
    let Some(application_id) = raw.discord.application_id else {
        return Ok(None);
    };
    let channels = raw
        .discord
        .channels
        .into_iter()
        .filter_map(|c| {
            c.channel_id.map(|channel_id| DiscordChannelMapping {
                channel_id,
                alias: c.alias,
                base_tags: c.base_tags,
            })
        })
        .collect();
    Ok(Some(DiscordSettings {
        application_id,
        allowed_users: raw.discord.allowed_users,
        permission_gate: raw.discord.permission_gate,
        channels,
        session_ttl_seconds: raw
            .discord
            .session_ttl_seconds
            .unwrap_or(DEFAULT_SESSION_TTL_SECONDS),
    }))
}

/// Writes `application_id` into the global config's `[discord]` table,
/// preserving every other section and any existing `allowed_users`/
/// `permission_gate`/`[[discord.channels]]`. Used by `hivemind discord login`
/// after a successful token validation.
pub fn write_discord_login(global_path: &Path, application_id: &str) -> Result<()> {
    let mut doc: toml::Value = if global_path.is_file() {
        toml::from_str(&std::fs::read_to_string(global_path)?)
            .with_context(|| format!("parsing {}", global_path.display()))?
    } else {
        toml::Value::Table(toml::map::Map::new())
    };
    let table = doc
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("global config root is not a table"))?;
    let discord = table
        .entry("discord")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let discord_table = discord
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[discord] is not a table"))?;
    discord_table.insert(
        "application_id".to_string(),
        toml::Value::String(application_id.to_string()),
    );
    if let Some(dir) = global_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(global_path, toml::to_string_pretty(&doc)?)?;
    Ok(())
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test config:: --lib`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs
git commit -m "feat: add [discord] config schema (application_id, channels, permission_gate)"
```

---

## Task 4: `src/discord/token_store.rs` — keyring-backed bot token storage

**Files:**
- Create: `src/discord/token_store.rs`

**Interfaces:**
- Consumes: nothing new (uses the existing `keyring` crate, already a dependency).
- Produces:
  - `discord::token_store::TokenStore` trait: `save(&self, application_id: &str, token: &str) -> Result<()>`, `load(&self, application_id: &str) -> Result<Option<String>>`, `delete(&self, application_id: &str) -> Result<()>`.
  - `discord::token_store::KeyringTokenStore` (production impl, keyring service `"hivemind-discord"`).
  - `discord::token_store::FakeTokenStore` (test double, `#[cfg(test)]`).

- [ ] **Step 1: Write the failing tests**

`src/discord/token_store.rs`:
```rust
use anyhow::Result;

pub trait TokenStore: Send + Sync {
    fn save(&self, application_id: &str, token: &str) -> Result<()>;
    fn load(&self, application_id: &str) -> Result<Option<String>>;
    fn delete(&self, application_id: &str) -> Result<()>;
}

pub struct KeyringTokenStore;

impl TokenStore for KeyringTokenStore {
    fn save(&self, application_id: &str, token: &str) -> Result<()> {
        todo!()
    }

    fn load(&self, application_id: &str) -> Result<Option<String>> {
        todo!()
    }

    fn delete(&self, application_id: &str) -> Result<()> {
        todo!()
    }
}

#[cfg(test)]
pub struct FakeTokenStore(std::sync::Mutex<std::collections::HashMap<String, String>>);

#[cfg(test)]
impl Default for FakeTokenStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl FakeTokenStore {
    pub fn new() -> Self {
        FakeTokenStore(std::sync::Mutex::new(std::collections::HashMap::new()))
    }
}

#[cfg(test)]
impl TokenStore for FakeTokenStore {
    fn save(&self, application_id: &str, token: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .insert(application_id.to_string(), token.to_string());
        Ok(())
    }

    fn load(&self, application_id: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(application_id).cloned())
    }

    fn delete(&self, application_id: &str) -> Result<()> {
        self.0.lock().unwrap().remove(application_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_on_empty_store_returns_none() {
        let store = FakeTokenStore::new();
        assert_eq!(store.load("123456789012345678").unwrap(), None);
    }

    #[test]
    fn save_then_load_round_trips() {
        let store = FakeTokenStore::new();
        store.save("123456789012345678", "bot-token-abc").unwrap();
        assert_eq!(
            store.load("123456789012345678").unwrap(),
            Some("bot-token-abc".to_string())
        );
    }

    #[test]
    fn delete_removes_the_entry() {
        let store = FakeTokenStore::new();
        store.save("123456789012345678", "tok").unwrap();
        store.delete("123456789012345678").unwrap();
        assert_eq!(store.load("123456789012345678").unwrap(), None);
    }

    #[test]
    fn delete_on_missing_entry_does_not_error() {
        let store = FakeTokenStore::new();
        store.delete("000000000000000000").unwrap();
    }

    #[test]
    fn entries_are_keyed_per_application_id() {
        let store = FakeTokenStore::new();
        store.save("111111111111111111", "tok-a").unwrap();
        store.save("222222222222222222", "tok-b").unwrap();
        assert_eq!(
            store.load("111111111111111111").unwrap(),
            Some("tok-a".to_string())
        );
        assert_eq!(
            store.load("222222222222222222").unwrap(),
            Some("tok-b".to_string())
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail (via `todo!()` panics)**

Run: `cargo test discord::token_store:: --lib`
Expected: FAIL — panics with "not yet implemented" from the `todo!()` bodies.

- [ ] **Step 3: Implement `KeyringTokenStore`**

Replace the three `todo!()` bodies:
```rust
impl TokenStore for KeyringTokenStore {
    fn save(&self, application_id: &str, token: &str) -> Result<()> {
        let entry = keyring::Entry::new("hivemind-discord", application_id)?;
        entry.set_password(token)?;
        Ok(())
    }

    fn load(&self, application_id: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new("hivemind-discord", application_id)?;
        match entry.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn delete(&self, application_id: &str) -> Result<()> {
        let entry = keyring::Entry::new("hivemind-discord", application_id)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test discord::token_store:: --lib`
Expected: PASS (the `FakeTokenStore` tests exercise the trait; `KeyringTokenStore` itself isn't unit-tested, matching `keyring_store.rs`'s existing coverage boundary).

- [ ] **Step 5: Wire the module (temporary root, replaced fully in Task 13)**

Create `src/discord/mod.rs`:
```rust
pub mod token_store;
```

Add to `src/lib.rs` (after `pub mod db;`, alphabetically before `pub mod http;`):
```rust
pub mod discord;
```

- [ ] **Step 6: Commit**

```bash
git add src/discord/token_store.rs src/discord/mod.rs src/lib.rs
git commit -m "feat: add Discord bot-token keyring storage"
```

---

## Task 5: `src/discord/login.rs` — persist a validated login

**Files:**
- Create: `src/discord/login.rs`
- Modify: `src/discord/mod.rs` (add `pub mod login;`)

**Interfaces:**
- Consumes: `config::write_discord_login`, `discord::token_store::TokenStore`.
- Produces: `discord::login::persist_login(application_id: &str, bot_token: &str, store: &dyn TokenStore, global_config_path: &Path) -> Result<()>`.

- [ ] **Step 1: Write the failing test**

`src/discord/login.rs`:
```rust
use crate::config::write_discord_login;
use crate::discord::token_store::TokenStore;
use anyhow::Result;
use std::path::Path;

pub fn persist_login(
    application_id: &str,
    bot_token: &str,
    store: &dyn TokenStore,
    global_config_path: &Path,
) -> Result<()> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::token_store::FakeTokenStore;

    #[test]
    fn persists_token_and_writes_config() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.toml");
        let store = FakeTokenStore::new();
        persist_login(
            "123456789012345678",
            "bot-token-abc",
            &store,
            &config_path,
        )
        .unwrap();
        assert_eq!(
            store.load("123456789012345678").unwrap(),
            Some("bot-token-abc".to_string())
        );
        let settings = crate::config::load_discord_settings(&config_path)
            .unwrap()
            .unwrap();
        assert_eq!(settings.application_id, "123456789012345678");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test discord::login:: --lib`
Expected: FAIL — panics on `todo!()`.

- [ ] **Step 3: Implement `persist_login`**

```rust
pub fn persist_login(
    application_id: &str,
    bot_token: &str,
    store: &dyn TokenStore,
    global_config_path: &Path,
) -> Result<()> {
    store.save(application_id, bot_token)?;
    write_discord_login(global_config_path, application_id)?;
    Ok(())
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test discord::login:: --lib`
Expected: PASS.

- [ ] **Step 5: Wire the module**

Add to `src/discord/mod.rs`:
```rust
pub mod login;
```

- [ ] **Step 6: Commit**

```bash
git add src/discord/login.rs src/discord/mod.rs
git commit -m "feat: add Discord login persistence"
```

---

## Task 6: `src/discord/channels.rs` — channel/DM → memory layer + tags

**Files:**
- Create: `src/discord/channels.rs`
- Modify: `src/discord/mod.rs` (add `pub mod channels;`)

**Interfaces:**
- Consumes: `config::DiscordSettings`, `config::DiscordChannelMapping`.
- Produces:
  - `discord::channels::MemoryTarget { layer: &'static str, tags: Vec<String> }`
  - `discord::channels::resolve_target(settings: &DiscordSettings, channel_id: &str, is_dm: bool) -> MemoryTarget`
  - `discord::channels::context_system_prompt(target: &MemoryTarget) -> String`
  - Used by Task 8 (`store_direct.rs`) and Task 12 (`daemon.rs`).

- [ ] **Step 1: Write the failing tests**

`src/discord/channels.rs`:
```rust
use crate::config::DiscordSettings;

pub struct MemoryTarget {
    pub layer: &'static str,
    pub tags: Vec<String>,
}

pub fn resolve_target(settings: &DiscordSettings, channel_id: &str, is_dm: bool) -> MemoryTarget {
    todo!()
}

pub fn context_system_prompt(target: &MemoryTarget) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DiscordChannelMapping;

    fn settings_with_channel(mapping: DiscordChannelMapping) -> DiscordSettings {
        DiscordSettings {
            application_id: "123456789012345678".into(),
            allowed_users: vec![],
            permission_gate: None,
            channels: vec![mapping],
            session_ttl_seconds: crate::config::DEFAULT_SESSION_TTL_SECONDS,
        }
    }

    #[test]
    fn dm_maps_to_personal_layer_with_source_tag() {
        let settings = settings_with_channel(DiscordChannelMapping {
            channel_id: "999999999999999999".into(),
            alias: None,
            base_tags: vec!["project:hivemind".into()],
        });
        let target = resolve_target(&settings, "888888888888888888", true);
        assert_eq!(target.layer, "personal");
        assert_eq!(target.tags, vec!["source:discord".to_string()]);
    }

    #[test]
    fn mapped_channel_uses_configured_base_tags() {
        let settings = settings_with_channel(DiscordChannelMapping {
            channel_id: "222222222222222222".into(),
            alias: Some("hivemind-project".into()),
            base_tags: vec!["project:hivemind".into(), "topic:discord".into()],
        });
        let target = resolve_target(&settings, "222222222222222222", false);
        assert_eq!(target.layer, "workspace");
        assert_eq!(
            target.tags,
            vec!["project:hivemind".to_string(), "topic:discord".to_string()]
        );
    }

    #[test]
    fn unmapped_channel_falls_back_to_channel_id() {
        let settings = DiscordSettings {
            application_id: "123456789012345678".into(),
            allowed_users: vec![],
            permission_gate: None,
            channels: vec![],
            session_ttl_seconds: crate::config::DEFAULT_SESSION_TTL_SECONDS,
        };
        let target = resolve_target(&settings, "333333333333333333", false);
        assert_eq!(target.layer, "workspace");
        assert_eq!(
            target.tags,
            vec![
                "channel:333333333333333333".to_string(),
                "source:discord".to_string()
            ]
        );
    }

    #[test]
    fn context_system_prompt_includes_layer_and_tags() {
        let target = MemoryTarget {
            layer: "workspace",
            tags: vec!["project:hivemind".to_string(), "topic:discord".to_string()],
        };
        let prompt = context_system_prompt(&target);
        assert!(prompt.contains("workspace"));
        assert!(prompt.contains("project:hivemind"));
        assert!(prompt.contains("topic:discord"));
    }

    #[test]
    fn context_system_prompt_handles_no_tags() {
        let target = MemoryTarget {
            layer: "personal",
            tags: vec![],
        };
        let prompt = context_system_prompt(&target);
        assert!(prompt.contains("personal"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test discord::channels:: --lib`
Expected: FAIL — panics on `todo!()`.

- [ ] **Step 3: Implement `resolve_target` and `context_system_prompt`**

```rust
pub fn resolve_target(settings: &DiscordSettings, channel_id: &str, is_dm: bool) -> MemoryTarget {
    if is_dm {
        return MemoryTarget {
            layer: "personal",
            tags: vec!["source:discord".to_string()],
        };
    }
    if let Some(mapping) = settings.channels.iter().find(|c| c.channel_id == channel_id) {
        return MemoryTarget {
            layer: "workspace",
            tags: mapping.base_tags.clone(),
        };
    }
    MemoryTarget {
        layer: "workspace",
        tags: vec![format!("channel:{channel_id}"), "source:discord".to_string()],
    }
}

/// Instruction for the agent's system prompt (not spliced into the user
/// message) so it can't be confused with attacker-controlled text arriving
/// in the DM/channel message itself.
pub fn context_system_prompt(target: &MemoryTarget) -> String {
    let tags = if target.tags.is_empty() {
        "(none)".to_string()
    } else {
        target.tags.join(", ")
    };
    format!(
        "If you store or update a memory as part of this conversation, use layer \"{}\" \
         and include these tags: {tags}.",
        target.layer
    )
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test discord::channels:: --lib`
Expected: PASS.

- [ ] **Step 5: Wire the module**

Add to `src/discord/mod.rs`:
```rust
pub mod channels;
```

- [ ] **Step 6: Commit**

```bash
git add src/discord/channels.rs src/discord/mod.rs
git commit -m "feat: add Discord channel/DM memory-target resolution"
```

---

## Task 7: `src/discord/session.rs` — per-channel agent-session TTL map

**Files:**
- Create: `src/discord/session.rs`
- Modify: `src/discord/mod.rs` (add `pub mod session;`)

**Interfaces:**
- Produces: `discord::session::SessionMap` with `new(ttl: Duration) -> Self`, `async fn get(&self, channel_id: &str) -> Option<String>`, `async fn set(&self, channel_id: &str, session_id: String)`, `async fn reset(&self, channel_id: &str)`, `Clone`, `Default`. Identical semantics to `matrix::session::SessionMap`, keyed by Discord channel ID instead of Matrix room ID. Used by Task 12 (`daemon.rs`).

- [ ] **Step 1: Write the failing tests**

`src/discord/session.rs`:
```rust
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

struct Entry {
    session_id: String,
    last_active: Instant,
}

/// How long a channel's session stays resumable after its last activity, in
/// the absence of a configured `[discord] session_ttl_seconds`. Past this,
/// `get` treats it as detached and the next message starts a fresh agent
/// session instead of resuming a stale one.
const DEFAULT_SESSION_TTL: Duration =
    Duration::from_secs(crate::config::DEFAULT_SESSION_TTL_SECONDS);

#[derive(Clone)]
pub struct SessionMap {
    entries: Arc<Mutex<HashMap<String, Entry>>>,
    ttl: Duration,
}

impl Default for SessionMap {
    fn default() -> Self {
        Self::new(DEFAULT_SESSION_TTL)
    }
}

impl SessionMap {
    pub fn new(ttl: Duration) -> Self {
        SessionMap {
            entries: Arc::new(Mutex::new(HashMap::new())),
            ttl,
        }
    }

    pub async fn get(&self, channel_id: &str) -> Option<String> {
        todo!()
    }

    pub async fn set(&self, channel_id: &str, session_id: String) {
        todo!()
    }

    pub async fn reset(&self, channel_id: &str) {
        todo!()
    }

    #[cfg(test)]
    async fn set_aged(&self, channel_id: &str, session_id: String, age: Duration) {
        self.entries.lock().await.insert(
            channel_id.to_string(),
            Entry {
                session_id,
                last_active: Instant::now() - age,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_on_empty_map_returns_none() {
        let map = SessionMap::default();
        assert_eq!(map.get("111111111111111111").await, None);
    }

    #[tokio::test]
    async fn set_then_get_returns_the_stored_session_id() {
        let map = SessionMap::default();
        map.set("111111111111111111", "sess-1".to_string()).await;
        assert_eq!(
            map.get("111111111111111111").await,
            Some("sess-1".to_string())
        );
    }

    #[tokio::test]
    async fn set_overwrites_previous_session_id_for_the_same_channel() {
        let map = SessionMap::default();
        map.set("111111111111111111", "sess-1".to_string()).await;
        map.set("111111111111111111", "sess-2".to_string()).await;
        assert_eq!(
            map.get("111111111111111111").await,
            Some("sess-2".to_string())
        );
    }

    #[tokio::test]
    async fn session_within_ttl_is_resumable() {
        let map = SessionMap::default();
        map.set_aged("111111111111111111", "sess-1".to_string(), Duration::from_secs(60))
            .await;
        assert_eq!(
            map.get("111111111111111111").await,
            Some("sess-1".to_string())
        );
    }

    #[tokio::test]
    async fn session_past_ttl_is_detached_and_removed() {
        let map = SessionMap::default();
        map.set_aged("111111111111111111", "sess-1".to_string(), Duration::from_secs(121))
            .await;
        assert_eq!(map.get("111111111111111111").await, None);
        map.set_aged("111111111111111111", "sess-1".to_string(), Duration::from_secs(121))
            .await;
        assert_eq!(map.get("111111111111111111").await, None);
    }

    #[tokio::test]
    async fn reset_clears_only_that_channel() {
        let map = SessionMap::default();
        map.set("111111111111111111", "sess-a".to_string()).await;
        map.set("222222222222222222", "sess-b".to_string()).await;
        map.reset("111111111111111111").await;
        assert_eq!(map.get("111111111111111111").await, None);
        assert_eq!(
            map.get("222222222222222222").await,
            Some("sess-b".to_string())
        );
    }

    #[tokio::test]
    async fn cloned_map_shares_state() {
        let map = SessionMap::default();
        let cloned = map.clone();
        cloned.set("111111111111111111", "sess-1".to_string()).await;
        assert_eq!(
            map.get("111111111111111111").await,
            Some("sess-1".to_string())
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test discord::session:: --lib`
Expected: FAIL — panics on `todo!()`.

- [ ] **Step 3: Implement `get`/`set`/`reset`**

```rust
    pub async fn get(&self, channel_id: &str) -> Option<String> {
        let mut map = self.entries.lock().await;
        let entry = map.get(channel_id)?;
        if entry.last_active.elapsed() > self.ttl {
            map.remove(channel_id);
            return None;
        }
        Some(entry.session_id.clone())
    }

    pub async fn set(&self, channel_id: &str, session_id: String) {
        self.entries.lock().await.insert(
            channel_id.to_string(),
            Entry {
                session_id,
                last_active: Instant::now(),
            },
        );
    }

    pub async fn reset(&self, channel_id: &str) {
        self.entries.lock().await.remove(channel_id);
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test discord::session:: --lib`
Expected: PASS.

- [ ] **Step 5: Wire the module**

Add to `src/discord/mod.rs`:
```rust
pub mod session;
```

- [ ] **Step 6: Commit**

```bash
git add src/discord/session.rs src/discord/mod.rs
git commit -m "feat: add Discord per-channel session TTL map"
```

---

## Task 8: `src/discord/store_direct.rs` — fast-path `/hm store`

**Files:**
- Create: `src/discord/store_direct.rs`
- Modify: `src/discord/mod.rs` (add `pub mod store_direct;`)

**Interfaces:**
- Consumes: `discord::channels::MemoryTarget` (Task 6), the `rmcp` crate (already a dependency).
- Produces: `discord::store_direct::derive_title(content: &str) -> String`, `discord::store_direct::store_memory(hivemind_bin: &str, content: &str, target: &MemoryTarget) -> Result<(), String>`. Used by Task 12 (`daemon.rs`'s `/hm store` handler).

- [ ] **Step 1: Write the failing tests**

`src/discord/store_direct.rs`:
```rust
const MAX_TITLE_CHARS: usize = 60;

/// Derives a short title from free-form content for direct-store memories
/// (the `/hm store` path has no separate title field — just the text option).
pub fn derive_title(content: &str) -> String {
    todo!()
}

use crate::discord::channels::MemoryTarget;
use rmcp::ServiceExt;
use rmcp::transport::TokioChildProcess;

/// Spawns `hivemind` in stdio MCP mode and calls `memory_store` directly —
/// no agent CLI, no LLM interpretation. This is the `/hm store` fast path:
/// verbatim, no cost, no tagging/dedup judgment beyond what resolve_target
/// already decided.
pub async fn store_memory(
    hivemind_bin: &str,
    content: &str,
    target: &MemoryTarget,
) -> Result<(), String> {
    let transport = TokioChildProcess::new(tokio::process::Command::new(hivemind_bin))
        .map_err(|e| format!("failed to spawn hivemind: {e}"))?;
    let client = ()
        .serve(transport)
        .await
        .map_err(|e| format!("failed to connect to hivemind MCP: {e}"))?;

    let mut arguments = serde_json::Map::new();
    arguments.insert(
        "title".to_string(),
        serde_json::Value::String(derive_title(content)),
    );
    arguments.insert(
        "content".to_string(),
        serde_json::Value::String(content.to_string()),
    );
    arguments.insert(
        "tags".to_string(),
        serde_json::Value::Array(
            target
                .tags
                .iter()
                .cloned()
                .map(serde_json::Value::String)
                .collect(),
        ),
    );
    arguments.insert(
        "layer".to_string(),
        serde_json::Value::String(target.layer.to_string()),
    );

    let result = client
        .call_tool(
            rmcp::model::CallToolRequestParams::new("memory_store").with_arguments(arguments),
        )
        .await;

    let _ = client.cancel().await;

    match result {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("memory_store call failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_content_becomes_the_whole_title() {
        assert_eq!(derive_title("use tabs not spaces"), "use tabs not spaces");
    }

    #[test]
    fn long_content_is_truncated_with_ellipsis() {
        let content = "a".repeat(100);
        let title = derive_title(&content);
        assert!(
            title.len() <= 63,
            "title should be truncated, got {} chars",
            title.len()
        );
        assert!(title.ends_with('…'));
    }

    #[test]
    fn truncation_happens_on_a_char_boundary_for_multibyte_content() {
        let content = "café ".repeat(20);
        let title = derive_title(&content); // must not panic
        assert!(title.ends_with('…'));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test discord::store_direct:: --lib`
Expected: FAIL — panics on `todo!()` in `derive_title`.

- [ ] **Step 3: Implement `derive_title`**

```rust
pub fn derive_title(content: &str) -> String {
    let trimmed = content.trim();
    let char_count = trimmed.chars().count();
    if char_count <= MAX_TITLE_CHARS {
        return trimmed.to_string();
    }
    let truncated: String = trimmed.chars().take(MAX_TITLE_CHARS).collect();
    format!("{truncated}…")
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test discord::store_direct:: --lib`
Expected: PASS.

- [ ] **Step 5: Wire the module**

Add to `src/discord/mod.rs`:
```rust
pub mod store_direct;
```

- [ ] **Step 6: Commit**

```bash
git add src/discord/store_direct.rs src/discord/mod.rs
git commit -m "feat: add Discord fast-path direct memory store"
```

---

## Task 9: `src/discord/status.rs` — Unix-socket status broadcast

**Files:**
- Create: `src/discord/status.rs`
- Modify: `src/discord/mod.rs` (add `pub mod status;`)

**Interfaces:**
- Produces:
  - `discord::status::ChannelStatus { channel_id: String, alias: Option<String>, active_session: bool, last_active_at: Option<String> }`
  - `discord::status::StatusReply { logged_in: bool, application_id: String, sync_state: String, last_sync_at: Option<String>, channels: Vec<ChannelStatus> }`
  - `discord::status::QueryError { NotRunning, Protocol(String) }`
  - `discord::status::socket_path() -> PathBuf` (`hivemind-discord.sock` under `db::xdg_data_dir()`)
  - `discord::status::serve_status(socket_path: &Path, reply_source: Arc<Mutex<StatusReply>>) -> Result<()>`
  - `discord::status::query_status(socket_path: &Path) -> Result<StatusReply, QueryError>`
  - Used by Task 12 (`daemon.rs`, serving side) and Task 18 (`cli/status.rs`, querying side).

- [ ] **Step 1: Write the failing tests**

`src/discord/status.rs`:
```rust
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub enum QueryError {
    NotRunning,
    Protocol(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelStatus {
    pub channel_id: String,
    pub alias: Option<String>,
    pub active_session: bool,
    pub last_active_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatusReply {
    pub logged_in: bool,
    pub application_id: String,
    pub sync_state: String,
    pub last_sync_at: Option<String>,
    pub channels: Vec<ChannelStatus>,
}

pub fn socket_path() -> PathBuf {
    crate::db::xdg_data_dir().join("hivemind-discord.sock")
}

pub async fn serve_status(socket_path: &Path, reply_source: Arc<Mutex<StatusReply>>) -> Result<()> {
    todo!()
}

pub async fn query_status(socket_path: &Path) -> Result<StatusReply, QueryError> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_reply() -> StatusReply {
        StatusReply {
            logged_in: true,
            application_id: "123456789012345678".to_string(),
            sync_state: "connected".to_string(),
            last_sync_at: Some("2026-08-22T10:03:00Z".to_string()),
            channels: vec![ChannelStatus {
                channel_id: "222222222222222222".to_string(),
                alias: Some("hivemind-project".to_string()),
                active_session: true,
                last_active_at: Some("2026-08-22T10:02:40Z".to_string()),
            }],
        }
    }

    #[tokio::test]
    async fn client_receives_the_servers_current_reply() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let reply = Arc::new(Mutex::new(sample_reply()));
        let server_socket = socket_path.clone();
        let server_reply = reply.clone();
        tokio::spawn(async move {
            serve_status(&server_socket, server_reply).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let got = query_status(&socket_path).await.unwrap();
        assert_eq!(got, sample_reply());
    }

    #[tokio::test]
    async fn client_sees_updates_to_the_shared_reply() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let reply = Arc::new(Mutex::new(sample_reply()));
        let server_socket = socket_path.clone();
        let server_reply = reply.clone();
        tokio::spawn(async move {
            serve_status(&server_socket, server_reply).await.unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        {
            let mut r = reply.lock().await;
            r.sync_state = "reconnecting".to_string();
        }
        let got = query_status(&socket_path).await.unwrap();
        assert_eq!(got.sync_state, "reconnecting");
    }

    #[tokio::test]
    async fn query_against_nonexistent_socket_errors() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("does-not-exist.sock");
        assert!(matches!(
            query_status(&socket_path).await,
            Err(QueryError::NotRunning)
        ));
    }

    #[tokio::test]
    async fn query_against_a_socket_serving_garbage_reports_a_protocol_error_not_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("garbage.sock");
        let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = stream.write_all(b"not valid json\n").await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let result = query_status(&socket_path).await;
        assert!(
            matches!(result, Err(QueryError::Protocol(_))),
            "expected a Protocol error (daemon reachable but sent bad data), got: {result:?}"
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test discord::status:: --lib`
Expected: FAIL — panics on `todo!()`.

- [ ] **Step 3: Implement `serve_status` and `query_status`**

```rust
pub async fn serve_status(socket_path: &Path, reply_source: Arc<Mutex<StatusReply>>) -> Result<()> {
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    if let Some(dir) = socket_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    loop {
        let (mut stream, _) = listener.accept().await?;
        let reply = reply_source.lock().await.clone();
        let line = serde_json::to_string(&reply)? + "\n";
        let _ = stream.write_all(line.as_bytes()).await;
    }
}

pub async fn query_status(socket_path: &Path) -> Result<StatusReply, QueryError> {
    let mut stream = UnixStream::connect(socket_path)
        .await
        .map_err(|_| QueryError::NotRunning)?;
    let mut buf = String::new();
    stream
        .read_to_string(&mut buf)
        .await
        .map_err(|e| QueryError::Protocol(format!("failed reading from daemon: {e}")))?;
    serde_json::from_str(buf.trim())
        .map_err(|e| QueryError::Protocol(format!("daemon sent malformed status: {e}")))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test discord::status:: --lib`
Expected: PASS.

- [ ] **Step 5: Wire the module**

Add to `src/discord/mod.rs`:
```rust
pub mod status;
```

- [ ] **Step 6: Commit**

```bash
git add src/discord/status.rs src/discord/mod.rs
git commit -m "feat: add Discord daemon status Unix-socket broadcast"
```

---

## Task 10: `discord_pidfile_path()` in `src/db.rs`

**Files:**
- Modify: `src/db.rs` (right after `matrix_pidfile_path`, around line 36)
- Test: `src/db.rs` (`#[cfg(test)] mod tests`, if one exists — otherwise add inline as a doc-verified assertion in Task 12's daemon test, since this is a one-line pure function)

**Interfaces:**
- Produces: `db::discord_pidfile_path() -> PathBuf` — `$XDG_DATA_HOME/hivemind/hivemind-discord.pid`. Used by Task 12 (`daemon.rs`'s pidfile guard).

- [ ] **Step 1: Add the function**

In `src/db.rs`, right after `matrix_pidfile_path` (around line 36):
```rust
/// PID file written by `hivemind discord run` while its daemon is running:
/// $XDG_DATA_HOME/hivemind/hivemind-discord.pid
pub fn discord_pidfile_path() -> std::path::PathBuf {
    xdg_data_dir().join("hivemind-discord.pid")
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check --lib`
Expected: succeeds.

- [ ] **Step 3: Commit**

```bash
git add src/db.rs
git commit -m "feat: add discord daemon pidfile path"
```

---

## Task 11: `src/discord/daemon.rs` part 1 — pure authorization logic

**Files:**
- Create: `src/discord/daemon.rs`
- Modify: `src/discord/mod.rs` (add `pub mod daemon;`)

**Interfaces:**
- Consumes: `config::DiscordSettings`.
- Produces:
  - `discord::daemon::EventDecision { should_handle: bool, is_dm: bool }`
  - `discord::daemon::decide(settings: &DiscordSettings, is_dm: bool, author_is_bot: bool, author_id: &str, mentions_bot: bool) -> EventDecision`
  - `discord::daemon::parse_permission_gate(value: &str) -> Result<serenity::model::Permissions, String>`
  - Both consumed by Task 12's `EventHandler` implementation, added to this same file.

- [ ] **Step 1: Write the failing tests**

`src/discord/daemon.rs`:
```rust
use crate::config::DiscordSettings;

pub struct EventDecision {
    pub should_handle: bool,
    pub is_dm: bool,
}

pub fn decide(
    settings: &DiscordSettings,
    is_dm: bool,
    author_is_bot: bool,
    author_id: &str,
    mentions_bot: bool,
) -> EventDecision {
    todo!()
}

/// Maps the `[discord] permission_gate` config string to a Discord permission
/// bit, used as `/hm`'s `default_member_permissions` at registration time.
pub fn parse_permission_gate(value: &str) -> Result<serenity::model::Permissions, String> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DiscordChannelMapping;

    fn settings() -> DiscordSettings {
        DiscordSettings {
            application_id: "999999999999999999".into(),
            allowed_users: vec!["111111111111111111".into()],
            permission_gate: None,
            channels: vec![DiscordChannelMapping {
                channel_id: "222222222222222222".into(),
                alias: None,
                base_tags: vec!["project:hivemind".into()],
            }],
            session_ttl_seconds: crate::config::DEFAULT_SESSION_TTL_SECONDS,
        }
    }

    #[test]
    fn own_bot_messages_are_never_handled() {
        let d = decide(&settings(), false, true, "999999999999999999", true);
        assert!(!d.should_handle);
    }

    #[test]
    fn dm_from_allowed_user_is_handled() {
        let d = decide(&settings(), true, false, "111111111111111111", false);
        assert!(d.should_handle);
        assert!(d.is_dm);
    }

    #[test]
    fn dm_from_non_allowed_user_is_silently_ignored() {
        let d = decide(&settings(), true, false, "333333333333333333", false);
        assert!(!d.should_handle);
    }

    #[test]
    fn channel_message_without_mention_is_ignored() {
        let d = decide(&settings(), false, false, "111111111111111111", false);
        assert!(!d.should_handle);
    }

    #[test]
    fn channel_message_with_mention_is_handled_regardless_of_sender() {
        let d = decide(&settings(), false, false, "444444444444444444", true);
        assert!(d.should_handle);
        assert!(!d.is_dm);
    }

    #[test]
    fn parse_permission_gate_accepts_known_values() {
        assert_eq!(
            parse_permission_gate("manage_guild").unwrap(),
            serenity::model::Permissions::MANAGE_GUILD
        );
        assert_eq!(
            parse_permission_gate("administrator").unwrap(),
            serenity::model::Permissions::ADMINISTRATOR
        );
    }

    #[test]
    fn parse_permission_gate_rejects_unknown_values() {
        let err = parse_permission_gate("super_admin").unwrap_err();
        assert!(err.contains("super_admin"));
        assert!(err.contains("manage_guild"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test discord::daemon:: --lib`
Expected: FAIL — panics on `todo!()`.

- [ ] **Step 3: Implement `decide` and `parse_permission_gate`**

```rust
pub fn decide(
    settings: &DiscordSettings,
    is_dm: bool,
    author_is_bot: bool,
    author_id: &str,
    mentions_bot: bool,
) -> EventDecision {
    if author_is_bot {
        return EventDecision {
            should_handle: false,
            is_dm,
        };
    }
    let should_handle = if is_dm {
        settings.allowed_users.iter().any(|u| u == author_id)
    } else {
        mentions_bot
    };
    EventDecision { should_handle, is_dm }
}

pub fn parse_permission_gate(value: &str) -> Result<serenity::model::Permissions, String> {
    use serenity::model::Permissions;
    match value {
        "manage_guild" => Ok(Permissions::MANAGE_GUILD),
        "administrator" => Ok(Permissions::ADMINISTRATOR),
        "manage_channels" => Ok(Permissions::MANAGE_CHANNELS),
        "manage_messages" => Ok(Permissions::MANAGE_MESSAGES),
        "kick_members" => Ok(Permissions::KICK_MEMBERS),
        "ban_members" => Ok(Permissions::BAN_MEMBERS),
        other => Err(format!(
            "unknown [discord] permission_gate \"{other}\" (expected one of: manage_guild, \
             administrator, manage_channels, manage_messages, kick_members, ban_members)"
        )),
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test discord::daemon:: --lib`
Expected: PASS.

- [ ] **Step 5: Wire the module**

Add to `src/discord/mod.rs`:
```rust
pub mod daemon;
```

- [ ] **Step 6: Commit**

```bash
git add src/discord/daemon.rs src/discord/mod.rs
git commit -m "feat: add Discord message-authorization and permission-gate logic"
```

---

## Task 12: `src/discord/daemon.rs` part 2 — gateway client, event handling, `run()`

**Files:**
- Modify: `src/discord/daemon.rs` (append below the Task 11 code, inside the same file)

**Interfaces:**
- Consumes: `chat_bot::agent::run_turn` (Task 1), `discord::channels::{resolve_target, context_system_prompt}` (Task 6), `discord::session::SessionMap` (Task 7), `discord::store_direct::store_memory` (Task 8), `discord::status::{StatusReply, ChannelStatus, socket_path, serve_status}` (Task 9), `discord::token_store::{TokenStore, KeyringTokenStore}` (Task 4), `db::discord_pidfile_path` (Task 10), `config::DiscordSettings`, `config::AgentSettings`.
- Produces: `discord::daemon::run(settings: DiscordSettings, agent: AgentSettings, hivemind_bin: String) -> Result<()>` and `discord::daemon::send_direct_message(settings: &DiscordSettings, to_user_id: &str, message: &str) -> Result<()>`. Consumed by Task 16 (`main.rs`).

This task's gateway-connection code is not unit-tested, matching `matrix::daemon::run`/`restore_client`'s existing coverage boundary — its only verification is a successful compile and (later, manually) a real Discord bot.

- [ ] **Step 1: Add imports and the pidfile guard**

At the top of `src/discord/daemon.rs`, above the existing `use crate::config::DiscordSettings;`:
```rust
use crate::config::AgentSettings;
use crate::discord::status::{ChannelStatus, StatusReply};
use crate::discord::token_store::{KeyringTokenStore, TokenStore};
use anyhow::Result;
use serenity::all::{
    ChannelId, Command, Context, CreateCommand, CreateCommandOption, CommandOptionType,
    CreateInteractionResponse, CreateInteractionResponseMessage, EventHandler, GatewayIntents,
    Interaction, Message, Ready, ResolvedOption, ResolvedValue,
};
use serenity::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

struct PidGuard(std::path::PathBuf);

impl Drop for PidGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn write_pidfile() -> Result<PidGuard> {
    let path = crate::db::discord_pidfile_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, std::process::id().to_string())?;
    Ok(PidGuard(path))
}

fn now_ts() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

async fn mark_channel_active(status_reply: &Arc<Mutex<StatusReply>>, channel_id: &str) {
    let mut r = status_reply.lock().await;
    if let Some(channel) = r.channels.iter_mut().find(|c| c.channel_id == channel_id) {
        channel.active_session = true;
        channel.last_active_at = Some(now_ts());
    } else {
        r.channels.push(ChannelStatus {
            channel_id: channel_id.to_string(),
            alias: None,
            active_session: true,
            last_active_at: Some(now_ts()),
        });
    }
}

async fn mark_channel_inactive(status_reply: &Arc<Mutex<StatusReply>>, channel_id: &str) {
    let mut r = status_reply.lock().await;
    if let Some(channel) = r.channels.iter_mut().find(|c| c.channel_id == channel_id) {
        channel.active_session = false;
    }
}

const HELP_TEXT: &str = "/hm store text:<text>  — directly store a memory, skipping the agent\n\
                          /hm reset              — start a fresh conversation in this channel\n\
                          /hm help                — show this message\n\
                          Mention me in a channel, or DM me directly, to chat freely.";
```

- [ ] **Step 2: Add the `Handler` struct and command builder**

```rust
struct Handler {
    settings: Arc<DiscordSettings>,
    agent: Arc<AgentSettings>,
    hivemind_bin: Arc<String>,
    sessions: crate::discord::session::SessionMap,
    status_reply: Arc<Mutex<StatusReply>>,
    permission_gate: Option<serenity::model::Permissions>,
}

fn build_hm_command(permission_gate: Option<serenity::model::Permissions>) -> CreateCommand {
    let mut cmd = CreateCommand::new("hm")
        .description("HiveMind memory bot")
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "store", "Directly store a memory")
                .add_sub_option(
                    CreateCommandOption::new(CommandOptionType::String, "text", "Memory text")
                        .required(true),
                ),
        )
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "reset",
            "Reset this channel's conversation",
        ))
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "help",
            "List /hm commands",
        ));
    if let Some(perm) = permission_gate {
        cmd = cmd.default_member_permissions(perm);
    }
    cmd
}

async fn respond_ephemeral(ctx: &Context, command: &serenity::all::CommandInteraction, text: &str) {
    let _ = command
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(text)
                    .ephemeral(true),
            ),
        )
        .await;
}
```

- [ ] **Step 3: Implement the `EventHandler`**

```rust
#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _ready: Ready) {
        tracing::debug!("discord gateway ready, registering /hm command");
        if let Err(e) = Command::create_global_command(&ctx.http, build_hm_command(self.permission_gate)).await {
            tracing::warn!("failed to register /hm command: {e:#}");
        }
        let mut r = self.status_reply.lock().await;
        r.sync_state = "connected".to_string();
        r.last_sync_at = Some(now_ts());
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }
        let is_dm = msg.guild_id.is_none();
        let bot_id = ctx.cache.current_user().id;
        let mentions_bot = msg.mentions_user_id(bot_id);
        let decision = crate::discord::daemon::decide(
            &self.settings,
            is_dm,
            msg.author.bot,
            &msg.author.id.to_string(),
            mentions_bot,
        );
        if !decision.should_handle {
            return;
        }

        let channel_id = msg.channel_id.to_string();
        {
            let mut r = self.status_reply.lock().await;
            r.last_sync_at = Some(now_ts());
        }
        let target = crate::discord::channels::resolve_target(&self.settings, &channel_id, is_dm);
        let system_prompt = crate::discord::channels::context_system_prompt(&target);
        let resume = self.sessions.get(&channel_id).await;
        match crate::chat_bot::agent::run_turn(
            &self.agent,
            &self.hivemind_bin,
            &msg.content,
            resume.as_deref(),
            Some(&system_prompt),
        )
        .await
        {
            Ok(result) => {
                self.sessions.set(&channel_id, result.session_id).await;
                mark_channel_active(&self.status_reply, &channel_id).await;
                let _ = msg.channel_id.say(&ctx.http, result.reply_text).await;
            }
            Err(e) => {
                self.sessions.reset(&channel_id).await;
                mark_channel_inactive(&self.status_reply, &channel_id).await;
                let _ = msg
                    .channel_id
                    .say(&ctx.http, format!("hivemind discord hit an error: {e}"))
                    .await;
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let Interaction::Command(command) = interaction else {
            return;
        };
        if command.data.name != "hm" {
            return;
        }
        let is_dm = command.guild_id.is_none();
        let author_id = command.user.id.to_string();
        if is_dm && !self.settings.allowed_users.iter().any(|u| u == &author_id) {
            return;
        }

        let Some(top) = command.data.options().into_iter().next() else {
            return;
        };
        let ResolvedValue::SubCommand(sub_opts) = top.value else {
            return;
        };
        let channel_id = command.channel_id.to_string();

        match top.name {
            "help" => respond_ephemeral(&ctx, &command, HELP_TEXT).await,
            "reset" => {
                self.sessions.reset(&channel_id).await;
                mark_channel_inactive(&self.status_reply, &channel_id).await;
                respond_ephemeral(&ctx, &command, "Reset.").await;
            }
            "store" => {
                let text = sub_opts.iter().find_map(|o| match &o.value {
                    ResolvedValue::String(s) if o.name == "text" => Some(s.to_string()),
                    _ => None,
                });
                let Some(text) = text else {
                    respond_ephemeral(&ctx, &command, "Missing text.").await;
                    return;
                };
                let target = crate::discord::channels::resolve_target(&self.settings, &channel_id, is_dm);
                match crate::discord::store_direct::store_memory(&self.hivemind_bin, &text, &target).await {
                    Ok(()) => {
                        mark_channel_active(&self.status_reply, &channel_id).await;
                        respond_ephemeral(&ctx, &command, "Stored.").await;
                    }
                    Err(e) => {
                        respond_ephemeral(
                            &ctx,
                            &command,
                            &format!("hivemind discord failed to store that: {e}"),
                        )
                        .await;
                    }
                }
            }
            _ => {}
        }
    }
}
```

- [ ] **Step 4: Implement `run` and `send_direct_message`**

```rust
pub async fn run(settings: DiscordSettings, agent: AgentSettings, hivemind_bin: String) -> Result<()> {
    let token = tokio::task::spawn_blocking({
        let application_id = settings.application_id.clone();
        move || KeyringTokenStore.load(&application_id)
    })
    .await??
    .ok_or_else(|| anyhow::anyhow!("no saved bot token — run `hivemind discord login` first"))?;

    let _pid_guard = write_pidfile()?;

    let permission_gate = settings
        .permission_gate
        .as_deref()
        .map(crate::discord::daemon::parse_permission_gate)
        .transpose()
        .map_err(|e| anyhow::anyhow!(e))?;

    let status_reply = Arc::new(Mutex::new(StatusReply {
        logged_in: true,
        application_id: settings.application_id.clone(),
        sync_state: "connecting".to_string(),
        last_sync_at: None,
        channels: settings
            .channels
            .iter()
            .map(|c| ChannelStatus {
                channel_id: c.channel_id.clone(),
                alias: c.alias.clone(),
                active_session: false,
                last_active_at: None,
            })
            .collect(),
    }));
    let socket_status = status_reply.clone();
    let socket_path = crate::discord::status::socket_path();
    tokio::spawn(async move {
        if let Err(e) = crate::discord::status::serve_status(&socket_path, socket_status).await {
            tracing::warn!("status socket exited: {e:#}");
        }
    });

    let sessions = crate::discord::session::SessionMap::new(std::time::Duration::from_secs(
        settings.session_ttl_seconds,
    ));

    let handler = Handler {
        settings: Arc::new(settings),
        agent: Arc::new(agent),
        hivemind_bin: Arc::new(hivemind_bin),
        sessions,
        status_reply,
        permission_gate,
    };

    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;
    let mut client = serenity::Client::builder(&token, intents)
        .event_handler(handler)
        .await?;
    client.start().await?;
    Ok(())
}

/// Sends a text message to the given user's DM channel, opening one if
/// needed. Used for one-off connectivity checks (`hivemind discord send`)
/// independent of the daemon's gateway connection.
pub async fn send_direct_message(
    settings: &DiscordSettings,
    to_user_id: &str,
    message: &str,
) -> Result<()> {
    let token = tokio::task::spawn_blocking({
        let application_id = settings.application_id.clone();
        move || KeyringTokenStore.load(&application_id)
    })
    .await??
    .ok_or_else(|| anyhow::anyhow!("no saved bot token — run `hivemind discord login` first"))?;

    let http = serenity::http::Http::new(&token);
    let user_id: serenity::model::id::UserId = to_user_id
        .parse::<u64>()
        .map_err(|e| anyhow::anyhow!("invalid Discord user id {to_user_id:?}: {e}"))?
        .into();
    let dm_channel = user_id.create_dm_channel(&http).await?;
    dm_channel.say(&http, message).await?;
    Ok(())
}
```

- [ ] **Step 5: Run the full test suite to verify the crate still compiles and every existing test still passes**

Run: `cargo test --lib`
Expected: PASS — this task adds no new unit tests (per the spec's stated coverage boundary for gateway code) but must not break any of the pure-logic tests from Tasks 1–11.

- [ ] **Step 6: Commit**

```bash
git add src/discord/daemon.rs
git commit -m "feat: add Discord gateway daemon (message/slash-command handling, run(), send_direct_message)"
```

---

## Task 13: `src/discord/mod.rs` final form + crate wiring sanity check

**Files:**
- Modify: `src/discord/mod.rs`

**Interfaces:**
- Produces: the complete `discord` module surface consumed by Task 14 onward.

- [ ] **Step 1: Verify the module file lists every submodule**

`src/discord/mod.rs` should now read:
```rust
pub mod channels;
pub mod daemon;
pub mod login;
pub mod session;
pub mod status;
pub mod store_direct;
pub mod token_store;
```

- [ ] **Step 2: Verify the whole crate builds clean**

Run: `cargo build --lib`
Expected: succeeds with no warnings about unused modules.

- [ ] **Step 3: Commit (only if Step 1 required an edit)**

```bash
git add src/discord/mod.rs
git commit -m "chore: finalize discord module structure"
```

---

## Task 14: `src/cli/mod.rs` — `Discord` subcommand and `DiscordAction` enum

**Files:**
- Modify: `src/cli/mod.rs`

**Interfaces:**
- Produces: `cli::Command::Discord { action: DiscordAction }` and `cli::DiscordAction { Login, Run { debug: bool }, Status, Send { user_id: String, message: String } }`. Consumed by Task 16 (`main.rs`).

- [ ] **Step 1: Add the `Discord` command variant**

In `src/cli/mod.rs`, in the `Command` enum, right after the `Matrix` variant (around line 53):
```rust
    /// Discord chat interface: capture/recall HiveMind memories from a channel or DM
    Discord {
        #[command(subcommand)]
        action: DiscordAction,
    },
```

- [ ] **Step 2: Add the `DiscordAction` enum**

Right after the `MatrixAction` enum (around line 100):
```rust
#[derive(Subcommand)]
pub enum DiscordAction {
    /// Log into a Discord bot account once; persists the token to the OS keyring
    Login,
    /// Run the Discord bot daemon (requires `hivemind discord login` first)
    Run {
        /// Print verbose connection/message logs to stderr
        #[arg(long)]
        debug: bool,
    },
    /// Show whether the daemon is running and its sync/session state
    Status,
    /// Send a one-off DM to a user (connectivity smoke test, no daemon needed)
    Send {
        /// Recipient's Discord user ID (snowflake, e.g. 111111111111111111)
        user_id: String,
        /// Message text to send
        message: String,
    },
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check --lib`
Expected: succeeds — `Command::Discord`/`DiscordAction` are self-contained clap derive types with nothing referencing them yet, so this compiles cleanly on its own. (The `discord_cmds` module wiring and its `mod`/`pub use` declarations are added in Task 15, alongside the file they refer to, so that task also ends in a compiling state.)

- [ ] **Step 4: Commit**

```bash
git add src/cli/mod.rs
git commit -m "feat: add hivemind discord subcommand surface"
```

---

## Task 15: `src/cli/discord_cmds.rs` — interactive login and status printing

**Files:**
- Create: `src/cli/discord_cmds.rs`
- Modify: `src/cli/mod.rs` (add the `mod discord_cmds;` declaration and `pub use discord_cmds::*;` re-export left out of Task 14)

**Interfaces:**
- Consumes: `discord::login::persist_login` (Task 5), `discord::token_store::KeyringTokenStore` (Task 4), `discord::status::{query_status, socket_path, QueryError}` (Task 9), `config::global_config_path`.
- Produces: `cli::cmd_discord_login() -> Result<()>`, `cli::cmd_discord_status() -> Result<()>`. Consumed by Task 16 (`main.rs`).

- [ ] **Step 1: Implement `cmd_discord_login`**

`src/cli/discord_cmds.rs`:
```rust
use anyhow::Result;
use std::io::Write as _;

// ── discord ──────────────────────────────────────────────────────────────

pub fn cmd_discord_login() -> Result<()> {
    print!("Bot token (from the Discord Developer Portal): ");
    std::io::stdout().flush()?;
    let bot_token = rpassword::prompt_password("")?;
    let bot_token = bot_token.trim().to_string();

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let http = serenity::http::Http::new(&bot_token);
            let current_user = http
                .get_current_user()
                .await
                .map_err(|e| anyhow::anyhow!("token rejected by Discord: {e}"))?;
            let application_id = current_user.id.to_string();

            let store = crate::discord::token_store::KeyringTokenStore;
            crate::discord::login::persist_login(
                &application_id,
                &bot_token,
                &store,
                &crate::config::global_config_path(),
            )?;
            println!(
                "Logged in as {} (application id {application_id}).",
                current_user.name
            );
            println!(
                "Token saved to the OS keyring. Run `hivemind discord run` to start the bot."
            );
            anyhow::Ok(())
        })
}

pub fn cmd_discord_status() -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let socket_path = crate::discord::status::socket_path();
            match crate::discord::status::query_status(&socket_path).await {
                Ok(reply) => {
                    println!("logged_in:      {}", reply.logged_in);
                    println!("application_id: {}", reply.application_id);
                    println!("sync_state:     {}", reply.sync_state);
                    if let Some(t) = &reply.last_sync_at {
                        println!("last_sync:      {t}");
                    }
                    if reply.channels.is_empty() {
                        println!("channels:       (none)");
                    } else {
                        println!("channels:");
                        for channel in &reply.channels {
                            let label = channel.alias.as_deref().unwrap_or(&channel.channel_id);
                            let session = if channel.active_session {
                                "active session"
                            } else {
                                "no active session"
                            };
                            println!("  {label}  ({session})");
                        }
                    }
                    Ok(())
                }
                Err(crate::discord::status::QueryError::NotRunning) => {
                    println!("hivemind discord is not running.");
                    println!("Start it with: hivemind discord run");
                    Ok(())
                }
                Err(crate::discord::status::QueryError::Protocol(msg)) => {
                    println!(
                        "hivemind discord appears to be running but returned invalid status data: {msg}"
                    );
                    Ok(())
                }
            }
        })
}
```

- [ ] **Step 2: Wire the module into `cli/mod.rs`**

Near the existing `mod matrix_cmds;`/`pub use matrix_cmds::*;` lines (around lines 112 and 120):
```rust
mod discord_cmds;
```
and
```rust
pub use discord_cmds::*;
```

- [ ] **Step 3: Verify the crate compiles**

Run: `cargo check --lib`
Expected: succeeds (this task has no independent unit tests — it's interactive I/O and live network calls, same coverage boundary as `matrix_cmds.rs::cmd_matrix_login`/`cmd_matrix_status`, which aren't unit-tested either).

- [ ] **Step 4: Commit**

```bash
git add src/cli/discord_cmds.rs src/cli/mod.rs
git commit -m "feat: add hivemind discord login/status CLI commands"
```

---

## Task 16: `src/main.rs` — dispatch wiring

**Files:**
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `cli::DiscordAction` (Task 14), `cli::cmd_discord_login`/`cmd_discord_status` (Task 15), `discord::daemon::{run, send_direct_message}` (Task 12), `config::load_discord_settings` (Task 3).
- Produces: working `hivemind discord login|run|status|send` CLI commands.

- [ ] **Step 1: Add the `Discord` match arm**

In `src/main.rs`, in `fn main()`'s `match cli.command`, right after the `Matrix` arm (around line 34):
```rust
        Some(Command::Discord { action }) => match action {
            cli::DiscordAction::Login => cli::cmd_discord_login(),
            cli::DiscordAction::Run { debug } => run_discord(debug),
            cli::DiscordAction::Status => cli::cmd_discord_status(),
            cli::DiscordAction::Send { user_id, message } => run_discord_send(user_id, message),
        },
```

- [ ] **Step 2: Add `run_discord` and `run_discord_send`**

At the end of `src/main.rs`, after `run_matrix_send` (around line 257):
```rust
#[tokio::main]
async fn run_discord(debug: bool) -> Result<()> {
    if debug {
        init_tracing_with_default("hivemind=debug,oxhivemind=debug");
    } else {
        init_tracing();
    }
    tracing::debug!("loading discord config");
    let settings =
        config::load_discord_settings(&config::global_config_path())?.ok_or_else(|| {
            anyhow::anyhow!("no [discord] config found — run `hivemind discord login` first")
        })?;
    let server_settings = config::load_server_settings(&config::global_config_path())?;
    let hivemind_bin = std::env::current_exe()?.to_string_lossy().into_owned();
    tracing::debug!("starting discord daemon");
    oxhivemind::discord::daemon::run(settings, server_settings.agent, hivemind_bin).await
}

#[tokio::main]
async fn run_discord_send(user_id: String, message: String) -> Result<()> {
    init_tracing_with_default("hivemind=debug,oxhivemind=debug");
    let settings =
        config::load_discord_settings(&config::global_config_path())?.ok_or_else(|| {
            anyhow::anyhow!("no [discord] config found — run `hivemind discord login` first")
        })?;
    oxhivemind::discord::daemon::send_direct_message(&settings, &user_id, &message).await
}
```

- [ ] **Step 3: Verify the crate builds**

Run: `cargo build`
Expected: succeeds. `hivemind discord --help` should list `login`, `run`, `status`, `send`.

- [ ] **Step 4: Manually verify the CLI surface**

Run: `cargo run -- discord --help`
Expected: shows all four subcommands with their descriptions from Task 14's doc comments.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire hivemind discord login|run|status|send into main"
```

---

## Task 17: `src/cli/service.rs` — `--discord` service-install support

**Files:**
- Modify: `src/cli/service.rs`
- Modify: `src/cli/mod.rs` (the `ServiceAction::Install` variant gains a `discord: bool` field)
- Modify: `src/main.rs` (the `ServiceAction::Install` match arm passes the new field through)
- Test: `src/cli/service.rs` (`#[cfg(all(test, target_os = "linux"))] mod matrix_service_tests`, extended)

**Interfaces:**
- Produces: `cli::cmd_service_install(dashboard: bool, matrix: bool, discord: bool) -> Result<()>` (signature change — all three call sites in this task update together).

- [ ] **Step 1: Write the failing test**

In `src/cli/service.rs`'s `#[cfg(all(test, target_os = "linux"))] mod matrix_service_tests` (around line 222), add:
```rust
    #[test]
    fn systemd_unit_content_for_discord_names_the_unit_and_subcommand() {
        let content = systemd_unit_content(
            "HiveMind Discord chat bot",
            &std::path::PathBuf::from("/usr/local/bin/hivemind"),
            &["discord", "run"],
        );
        assert!(content.contains("Description=HiveMind Discord chat bot"));
        assert!(content.contains("ExecStart=/usr/local/bin/hivemind discord run"));
        assert!(content.contains("WantedBy=default.target"));
    }
```

- [ ] **Step 2: Run the test to verify it fails to compile**

Run: `cargo test --lib service::`
Expected: this specific test actually passes already (it only calls the existing `systemd_unit_content` helper) — but the surrounding crate will fail to compile once Step 3 below changes `cmd_service_install`'s signature without updating call sites, so do Steps 3–6 together before re-running.

- [ ] **Step 3: Update `cmd_service_install` and the platform-specific install functions**

In `src/cli/service.rs`:
```rust
pub fn cmd_service_install(dashboard: bool, matrix: bool, discord: bool) -> Result<()> {
    #[cfg(target_os = "macos")]
    return service_install_macos(dashboard, matrix, discord);
    #[cfg(target_os = "linux")]
    return service_install_linux(dashboard, matrix, discord);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    anyhow::bail!("hivemind service install is only supported on Linux and macOS");
}
```

Linux (`service_install_linux`, around line 163):
```rust
#[cfg(target_os = "linux")]
fn service_install_linux(dashboard: bool, matrix: bool, discord: bool) -> Result<()> {
    let (args, desc): (&[&str], &str) = if dashboard {
        (&["up"], "HiveMind server (API + dashboard)")
    } else {
        (&["up", "--headless"], "HiveMind server (API only)")
    };
    service_install_unit_linux("hivemind", desc, args)?;

    if matrix {
        let configured = crate::config::load_matrix_settings(&crate::config::global_config_path())
            .ok()
            .flatten()
            .is_some();
        if !configured {
            anyhow::bail!(
                "--matrix was passed but Matrix is not configured.\n\
                 Run `hivemind matrix login` first, then re-run `hivemind service install --matrix`."
            );
        }
        service_install_unit_linux(
            "hivemind-matrix",
            "HiveMind Matrix chat bot",
            &["matrix", "run"],
        )?;
    }

    if discord {
        let configured = crate::config::load_discord_settings(&crate::config::global_config_path())
            .ok()
            .flatten()
            .is_some();
        if !configured {
            anyhow::bail!(
                "--discord was passed but Discord is not configured.\n\
                 Run `hivemind discord login` first, then re-run `hivemind service install --discord`."
            );
        }
        service_install_unit_linux(
            "hivemind-discord",
            "HiveMind Discord chat bot",
            &["discord", "run"],
        )?;
    }

    println!();
    println!("HiveMind will now start automatically on login.");
    if dashboard {
        let port = crate::config::load_server_settings(&crate::config::global_config_path())
            .map(|s| s.dashboard_port)
            .unwrap_or(3457);
        println!("Dashboard: http://127.0.0.1:{port}");
    }
    println!("Check status: hivemind service status");
    Ok(())
}
```

Extend `service_uninstall_linux`/`service_status_linux` (around lines 202/213):
```rust
#[cfg(target_os = "linux")]
fn service_uninstall_linux() -> Result<()> {
    service_uninstall_unit_linux("hivemind")?;
    if systemd_unit_path("hivemind-matrix").exists() {
        service_uninstall_unit_linux("hivemind-matrix")?;
    }
    if systemd_unit_path("hivemind-discord").exists() {
        service_uninstall_unit_linux("hivemind-discord")?;
    }

    println!("HiveMind service uninstalled.");
    Ok(())
}

#[cfg(target_os = "linux")]
fn service_status_linux() -> Result<()> {
    service_status_unit_linux("hivemind")?;
    if systemd_unit_path("hivemind-matrix").exists() {
        service_status_unit_linux("hivemind-matrix")?;
    }
    if systemd_unit_path("hivemind-discord").exists() {
        service_status_unit_linux("hivemind-discord")?;
    }
    Ok(())
}
```

macOS (`service_install_macos`, around line 369), mirroring the same three additions:
```rust
#[cfg(target_os = "macos")]
const DISCORD_LAUNCH_AGENT_LABEL: &str = "com.oxhive.hivemind-discord";
```
(add right after `MATRIX_LAUNCH_AGENT_LABEL`, around line 258)

```rust
#[cfg(target_os = "macos")]
fn service_install_macos(dashboard: bool, matrix: bool, discord: bool) -> Result<()> {
    let (args, desc): (&[&str], &str) = if dashboard {
        (&["up"], "HiveMind server (API + dashboard)")
    } else {
        (&["up", "--headless"], "HiveMind server (API only)")
    };
    service_install_unit_macos(LAUNCH_AGENT_LABEL, args, desc)?;

    if matrix {
        let configured = crate::config::load_matrix_settings(&crate::config::global_config_path())
            .ok()
            .flatten()
            .is_some();
        if !configured {
            anyhow::bail!(
                "--matrix was passed but Matrix is not configured.\n\
                 Run `hivemind matrix login` first, then re-run `hivemind service install --matrix`."
            );
        }
        service_install_unit_macos(
            MATRIX_LAUNCH_AGENT_LABEL,
            &["matrix", "run"],
            "HiveMind Matrix chat bot",
        )?;
    }

    if discord {
        let configured = crate::config::load_discord_settings(&crate::config::global_config_path())
            .ok()
            .flatten()
            .is_some();
        if !configured {
            anyhow::bail!(
                "--discord was passed but Discord is not configured.\n\
                 Run `hivemind discord login` first, then re-run `hivemind service install --discord`."
            );
        }
        service_install_unit_macos(
            DISCORD_LAUNCH_AGENT_LABEL,
            &["discord", "run"],
            "HiveMind Discord chat bot",
        )?;
    }

    println!();
    println!("HiveMind will now start automatically on login.");
    if dashboard {
        let port = crate::config::load_server_settings(&crate::config::global_config_path())
            .map(|s| s.dashboard_port)
            .unwrap_or(3457);
        println!("Dashboard: http://127.0.0.1:{port}");
    }
    println!("Logs: ~/Library/Logs/hivemind.log");
    println!("Check status: hivemind service status");
    Ok(())
}

#[cfg(target_os = "macos")]
fn service_uninstall_macos() -> Result<()> {
    service_uninstall_unit_macos(LAUNCH_AGENT_LABEL)?;
    if launch_agent_path(MATRIX_LAUNCH_AGENT_LABEL).exists() {
        service_uninstall_unit_macos(MATRIX_LAUNCH_AGENT_LABEL)?;
    }
    if launch_agent_path(DISCORD_LAUNCH_AGENT_LABEL).exists() {
        service_uninstall_unit_macos(DISCORD_LAUNCH_AGENT_LABEL)?;
    }

    println!("HiveMind service uninstalled.");
    Ok(())
}

#[cfg(target_os = "macos")]
fn service_status_macos() -> Result<()> {
    service_status_unit_macos(LAUNCH_AGENT_LABEL)?;
    if launch_agent_path(MATRIX_LAUNCH_AGENT_LABEL).exists() {
        service_status_unit_macos(MATRIX_LAUNCH_AGENT_LABEL)?;
    }
    if launch_agent_path(DISCORD_LAUNCH_AGENT_LABEL).exists() {
        service_status_unit_macos(DISCORD_LAUNCH_AGENT_LABEL)?;
    }
    Ok(())
}
```

- [ ] **Step 4: Update the `ServiceAction::Install` CLI variant**

In `src/cli/mod.rs`, the `ServiceAction::Install` variant (around line 67):
```rust
    Install {
        /// Also serve the dashboard from the background service
        #[arg(long)]
        dashboard: bool,
        /// Also install the Matrix bot unit (requires `hivemind matrix login` first)
        #[arg(long)]
        matrix: bool,
        /// Also install the Discord bot unit (requires `hivemind discord login` first)
        #[arg(long)]
        discord: bool,
    },
```

- [ ] **Step 5: Update the `main.rs` call site**

In `src/main.rs`, the `ServiceAction::Install` arm (around line 23):
```rust
            ServiceAction::Install { dashboard, matrix, discord } => {
                cli::cmd_service_install(dashboard, matrix, discord)
            }
```

- [ ] **Step 6: Run the tests to verify everything passes**

Run: `cargo test --lib service::`
Expected: PASS — including the existing `systemd_unit_content_for_matrix_*`/`systemd_unit_content_for_up_*` tests and the new `systemd_unit_content_for_discord_*` test from Step 1.

Run: `cargo build`
Expected: succeeds crate-wide (confirms both call-site updates in Steps 4–5 are consistent with the new three-argument signature).

- [ ] **Step 7: Commit**

```bash
git add src/cli/service.rs src/cli/mod.rs src/main.rs
git commit -m "feat: add --discord flag to hivemind service install"
```

---

## Task 18: `src/cli/status.rs` — `DiscordStatusLine` and `StatusData` wiring

**Files:**
- Modify: `src/cli/status.rs`

**Interfaces:**
- Produces: `cli::status::DiscordStatusLine { NotConfigured, NotRunning, Running { application_id: String, sync_state: String, channel_count: usize, active_sessions: usize } }`, and `StatusData.discord: DiscordStatusLine`. Consumed by Task 19 (`tui/status_view.rs`, `tui/header.rs`).

- [ ] **Step 1: Add the `DiscordStatusLine` enum and `StatusData` field**

In `src/cli/status.rs`, right after the `MatrixStatusLine` enum (around line 332):
```rust
pub enum DiscordStatusLine {
    /// No `[discord]` section in the global config — discord isn't set up.
    NotConfigured,
    /// Configured, but `hivemind discord run` isn't currently up.
    NotRunning,
    Running {
        application_id: String,
        sync_state: String,
        channel_count: usize,
        active_sessions: usize,
    },
}
```

Add a `discord: DiscordStatusLine` field to `StatusData` (around line 318), right after `matrix: MatrixStatusLine,`:
```rust
    pub discord: DiscordStatusLine,
```

- [ ] **Step 2: Build the field in `build_status_data`**

In `build_status_data`, right after the existing `let matrix = match ... ;` block (around line 373):
```rust
    let discord = match crate::config::load_discord_settings(global_path)? {
        None => DiscordStatusLine::NotConfigured,
        Some(_) => {
            let socket_path = crate::discord::status::socket_path();
            match crate::discord::status::query_status(&socket_path).await {
                Ok(reply) => DiscordStatusLine::Running {
                    application_id: reply.application_id,
                    sync_state: reply.sync_state,
                    channel_count: reply.channels.len(),
                    active_sessions: reply.channels.iter().filter(|c| c.active_session).count(),
                },
                Err(_) => DiscordStatusLine::NotRunning,
            }
        }
    };
```

Add `discord,` to the `StatusData { ... }` literal (around line 375, alongside `matrix,`):
```rust
        matrix,
        discord,
```

- [ ] **Step 3: Render the line in `format_status_text`**

In `format_status_text`, right after the existing `match &data.matrix { ... }` block (around line 513):
```rust
    match &data.discord {
        DiscordStatusLine::NotConfigured => {}
        DiscordStatusLine::NotRunning => {
            writeln!(
                out,
                "Discord:    configured, not running (hivemind discord run)"
            )
            .unwrap();
        }
        DiscordStatusLine::Running {
            application_id,
            sync_state,
            channel_count,
            active_sessions,
        } => {
            writeln!(
                out,
                "Discord:    {application_id} ({sync_state}), {channel_count} channel(s), \
                 {active_sessions} active session(s)"
            )
            .unwrap();
        }
    }
```

- [ ] **Step 4: Fix the `header.rs` test fixture so the crate compiles**

Adding the `discord` field to `StatusData` breaks `src/tui/header.rs`'s test `sample_data()`, which constructs a `StatusData` literal. In `src/tui/header.rs`'s `sample_data()` (around line 104), add the field right after `matrix: crate::cli::MatrixStatusLine::NotConfigured,`:
```rust
            discord: crate::cli::DiscordStatusLine::NotConfigured,
```

- [ ] **Step 5: Run the full test suite to verify everything compiles and passes**

Run: `cargo test --lib`
Expected: PASS crate-wide.

- [ ] **Step 6: Commit**

```bash
git add src/cli/status.rs src/tui/header.rs
git commit -m "feat: add DiscordStatusLine to hivemind status"
```

---

## Task 19: TUI — render the Discord status line

**Files:**
- Modify: `src/tui/status_view.rs` (the header itself doesn't render platform status lines, only `status_view.rs`'s body panel does — `header.rs`'s test fixture was already updated in Task 18)

**Interfaces:**
- Consumes: `cli::status::DiscordStatusLine` (Task 18).

- [ ] **Step 1: Render the Discord line in `status_view.rs`'s `draw()`**

In `src/tui/status_view.rs`, update the import at the top (line 1):
```rust
use crate::cli::{DiscordStatusLine, MatrixStatusLine, StatusData, build_status_data};
```

Right after the existing `match &data.matrix { ... }` block inside `draw()` (around line 287):
```rust
    match &data.discord {
        DiscordStatusLine::NotConfigured => {}
        DiscordStatusLine::NotRunning => {
            lines.push(Line::from("Discord    configured, not running"));
        }
        DiscordStatusLine::Running {
            application_id,
            sync_state,
            channel_count,
            active_sessions,
        } => {
            lines.push(Line::from(format!(
                "Discord    {application_id} ({sync_state}), {channel_count} channel(s), \
                 {active_sessions} active session(s)"
            )));
        }
    }
```

- [ ] **Step 2: Run the full test suite to verify everything compiles and passes**

Run: `cargo test --lib`
Expected: PASS crate-wide.

- [ ] **Step 3: Manually verify the interactive status view**

Run: `cargo run -- status` (in a terminal, without `--plain`)
Expected: the TUI status panel renders without a Discord line (since `[discord]` isn't configured in your local `~/.config/hivemind/config.toml` yet) — confirms the `NotConfigured` branch's silent no-op is correct.

- [ ] **Step 4: Commit**

```bash
git add src/tui/status_view.rs
git commit -m "feat: render Discord status line in hivemind status TUI"
```

---

## Task 20: README documentation

**Files:**
- Modify: `README.md`

**Interfaces:**
- None (documentation only).

- [ ] **Step 1: Add the command list entries**

In `README.md`'s command list (around line 345, right after the existing `hivemind matrix status` line):
```
hivemind discord login           Log into a Discord bot account (once); token saved to OS keyring
hivemind discord run              Run the Discord bot daemon
hivemind discord status           Show Discord bot login/sync/session state
```

- [ ] **Step 2: Add a "Discord chat interface" section**

Right after the existing "## Matrix chat interface (optional)" section (after its "Agent compatibility" subsection ends, before the `---` at what is currently line 557), add:

```markdown
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
```

- [ ] **Step 3: Proofread the rendered section**

Run: no automated check for README prose; open the file and confirm the new section's heading level, code fences, and internal anchor link (`#agent-compatibility`) render correctly.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: document the Discord chat interface"
```

---

## Final Verification

- [ ] Run the entire test suite once more end to end: `cargo test --lib`
- [ ] Run `cargo build --release` to confirm the release profile also compiles with the new `serenity` dependency.
- [ ] Run `cargo run -- discord --help` and `cargo run -- service install --help` to confirm the new flags/subcommands are visible and documented.
