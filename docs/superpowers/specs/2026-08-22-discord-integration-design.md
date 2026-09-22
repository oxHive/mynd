---
title: Discord Chat Interface — Parity with the Matrix Integration
date: 2026-08-22
status: approved
---

## Overview

Adds a Discord bot as a second chat interface alongside the existing Matrix integration, with full functional parity: capture/recall HiveMind memories from a Discord channel or DM, backed by the same headless-agent mechanism (no bespoke NLU). It's a separate process from `hivemind up`, same as `hivemind matrix run` — each message/command spawns a short-lived agent turn that talks to HiveMind as an MCP client.

Where Discord's platform model differs meaningfully from Matrix (bot-token auth instead of session login, structured slash commands instead of a `!`-prefix parser, no on-disk crypto store), the design follows Discord's idioms rather than forcing a literal port. Where the platforms are equivalent (room ↔ channel mapping, DM allowlist, session continuity, status reporting, service install), the design mirrors Matrix's existing shape exactly.

## Module Structure

New `src/discord/` module, mirroring `src/matrix/`:

- `daemon.rs` — serenity `Client` + `EventHandler`, holds `Arc<Mutex<StatusReply>>`, runs the gateway connection, routes `message` events (freeform chat) and `interaction_create` events (slash commands).
- `token_store.rs` — keyring persistence for the bot token, service name `"hivemind-discord"`, keyed by `application_id`. Same `SessionStore`-shaped trait as `keyring_store.rs` (a `TokenStore` trait with `save`/`load`/`delete`), with a `FakeTokenStore` test double.
- `login.rs` — `hivemind discord login` prompts for a pasted bot token, calls Discord's `/users/@me` with it to fetch `application_id`, then persists token→keyring and `application_id`→config (`persist_login`-equivalent).
- `status.rs` — same `StatusReply`/Unix-socket broadcast pattern as Matrix's `status.rs`, new socket file `hivemind-discord.sock`.
- `channels.rs` — channel/DM → memory layer + tags, structurally identical to `rooms.rs`'s `resolve_target`/`context_system_prompt`.
- `session.rs` — per-channel agent-session TTL map, identical logic to `matrix::session::SessionMap` (channel_id replaces room_id as the key).

**Not shared with Matrix, despite looking generic at a glance:**

- `commands.rs` — Matrix's `!hm` text parser has zero Matrix-type coupling today, but Discord doesn't use prefix parsing at all (see Slash Commands below), so there's nothing on the Discord side for it to be shared with. It stays Matrix-only.
- `rooms.rs`/`channels.rs`, `keyring_store.rs`/`token_store.rs`, `status.rs`, `daemon.rs`, `session.rs` stay duplicated-but-adapted per platform — they touch genuinely different types (serenity's `ChannelId`/`UserId` vs matrix-sdk's `RoomId`/`UserId`, different keyring service names, different socket paths). Forcing a shared trait over them now would be premature abstraction for marginal gain.

**Shared**, extracted from `src/matrix/` into a new `src/chat_bot/` module:

- `agent.rs` — the agent-CLI turn runner (`run_turn`, `run_claude_turn`, `run_opencode_turn`, `spawn_and_wait`). It only depends on `AgentSettings`/`AgentKind` from `config.rs` and plain strings — no Matrix types anywhere in it — so it moves as-is and both `matrix::daemon` and `discord::daemon` call `chat_bot::agent::run_turn`.

## Auth & Config

```toml
[discord]
application_id = "123456789012345678"      # written automatically by `discord login`
allowed_users = ["111111111111111111"]     # Discord user IDs (snowflakes) — required for DMs
permission_gate = "manage_guild"           # optional; if set, only guild members holding this
                                            # permission can invoke /hm in a guild channel

[[discord.channels]]
channel_id = "222222222222222222"
alias = "hivemind-project"
base_tags = ["project:hivemind"]
```

Matches Matrix's split between secret and non-secret state: the bot token (the actual credential) lives only in the OS keyring; `application_id` (not secret — visible in the Developer Portal and OAuth invite links) lives in config, same role as `matrix.user_id`. Unlike Matrix, there's no session-restore step — Discord bot tokens are used directly to build the client on every run, so `discord::daemon::restore_client`-equivalent just loads the token from the keyring and constructs a `serenity::Client` with it (no serialize/deserialize of a session object).

Channels the bot is in but not listed in `[[discord.channels]]` still work — memories land in the `workspace` layer tagged `channel:<id-or-alias>` + `source:discord`, matching Matrix's fallback behavior for unmapped rooms. DMs always use the `personal` layer, same as Matrix.

No on-disk store is needed for the Discord side (unlike matrix-sdk's sqlite store for e2ee/room state) — serenity keeps guild/channel state in an in-memory cache that's rebuilt each run from the gateway's initial payload.

## Slash Commands

Discord's idiomatic command surface is slash commands, not `!`-prefixed text — so the direct-store and reset commands become a single top-level `/hm` command with three subcommands, registered **globally** at daemon startup (works in every guild the bot is in; global registration/update takes up to ~1hr to propagate, which only matters once at first rollout, not during normal operation):

- `/hm store text:<text>` — direct write, skips the agent (parity with `!hm store`).
- `/hm reset` — starts a fresh conversation in that channel (parity with `!hm reset`).
- `/hm help` — replies ephemerally (visible only to the invoker) with a short description of all three subcommands plus a note that freeform chat happens via @mention (channel) or DM, not a command.

These arrive through Discord's Interactions API as structured, typed payloads handled in a separate `interaction_create` event — not string-parsed — which is why `commands.rs`'s text parser doesn't carry over.

Global registration vs. per-guild is a visibility/propagation-speed choice only, not a security boundary: once registered, any member of a guild the bot is in can see and attempt `/hm`, the same trust model Matrix already uses for mapped rooms ("room message with mention is handled regardless of sender"). The optional `permission_gate` config value maps to serenity's `Permissions` bitflags and is passed as the command's `default_member_permissions` at registration time, restricting who can even see/invoke it in a guild — this has no Matrix equivalent and is a new, opt-in tightening beyond parity, left unset (open to all guild members) by default to match Matrix's existing default.

## Message Handling

Mirrors `matrix::daemon::decide`, adapted to serenity's structured event data instead of Matrix's string-matching:

- Bot's own messages ignored via `msg.author.bot` (cleaner than Matrix's `sender == bot_user_id` string check).
- DMs: handled only if `msg.author.id` is in `allowed_users` — identical allowlist semantics to Matrix.
- Guild channel freeform chat: handled when the bot is @mentioned, checked via serenity's `msg.mentions_user_id(bot_id)` (structured, not Matrix's "message text contains the user ID string" heuristic).
- No auto-join handler is needed. Matrix rooms require an explicit invite-accept protocol the daemon reacts to (`StrippedRoomMemberEvent`); a Discord bot's guild membership is already established via an OAuth invite link before the daemon's gateway connection ever starts, so there's nothing analogous to auto-join.

Once a message or interaction is authorized, dispatch is identical to Matrix in shape: `/hm store` calls a Discord-side `store_direct::store_memory` (same MCP `memory_store` call, tags from `channels::resolve_target`), `/hm reset` clears the channel's `SessionMap` entry, and freeform chat calls `chat_bot::agent::run_turn` with the channel's resumable session ID and the same kind of system-prompt injection Matrix uses (layer/tags told to the agent via `--append-system-prompt`, never spliced into user-controlled message text).

## CLI Surface

```
hivemind discord login
hivemind discord run [--debug]
hivemind discord status
hivemind discord send <user_id> <message>
```

Same shape as the `matrix` subcommand family. `discord send` opens a DM channel and sends a message without needing the gateway/daemon running, same connectivity-smoke-test role as `matrix send`.

## Service Install

`cmd_service_install` gains a `discord: bool` parameter alongside the existing `dashboard`/`matrix` ones: `hivemind service install --matrix --discord` installs the main unit plus both bot units side by side. A `hivemind-discord` systemd unit (`ExecStart=hivemind discord run`) / macOS launchd agent (`com.oxhive.hivemind-discord`) is added, gated on `[discord]` being configured first (same `"--discord was passed but Discord is not configured"` guard pattern as `--matrix`). `service uninstall`/`service status` already iterate over known optional units unconditionally (no flags on those subcommands) — that list grows to include `hivemind-discord`.

## Status Reporting (CLI + TUI)

`StatusData` in `src/cli/status.rs` gains a `discord: DiscordStatusLine` field, the same three-variant shape as `MatrixStatusLine`: `NotConfigured` / `NotRunning` / `Running { application_id: String, sync_state: String, channel_count: usize, active_sessions: usize }`, built the same way Matrix's is — querying `discord::status::query_status` against `hivemind-discord.sock`. Rendered as a parallel `Discord:` line in both the plain-text `hivemind status` output and `src/tui/status_view.rs`/`header.rs`.

## Dependency

`serenity`, latest stable 0.12.x line (exact patch resolved via `cargo add` at implementation time — not pinned here), with `client`/`gateway`/`cache`/`model`/`rustls_backend` features.

## Testing

Same coverage boundary as the existing Matrix tests — pure logic gets unit tests, live network/gateway code doesn't:

- Message-handling decision logic (own-message/DM-allowlist/mention/permission-gate branches), as pure functions independent of serenity's event types where possible.
- `channels.rs`'s `resolve_target`/`context_system_prompt`, same table of cases as `rooms.rs`'s tests (DM → personal, mapped channel → configured tags, unmapped channel → fallback tags).
- `token_store.rs`'s `FakeTokenStore`-backed `persist_login` round-trip, mirroring `login.rs`'s existing test.
- `status.rs`'s Unix-socket serve/query round-trip, reusing the exact test pattern already in `matrix::status`.
- `chat_bot::agent`'s existing test suite continues to cover both Matrix and Discord callers once moved, unchanged.
- `daemon.rs`'s gateway connection and `login.rs`'s live Discord API call are not unit-tested, matching `matrix::daemon::run`/`restore_client` today.

## Explicitly Out of Scope

- Slash-command permission gating beyond the single optional `permission_gate` value — no per-subcommand permissions, no role-list config beyond Discord's own permission bits.
- Per-guild command registration or guild-join event handling — global registration only.
- Any on-disk persistence for Discord gateway/cache state — in-memory only, rebuilt each run.
- Voice, embeds, reactions, threads, or any Discord feature beyond text messages, DMs, and the `/hm` command family.
