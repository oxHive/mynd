use crate::config::{AgentSettings, MatrixSettings};
use crate::matrix::keyring_store::{KeyringSessionStore, SessionStore};
use crate::matrix::status::StatusReply;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

struct PidGuard(std::path::PathBuf);

impl Drop for PidGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Records this process's PID at `crate::db::matrix_pidfile_path()` while the
/// daemon is running, mirroring `mynd up`'s pidfile in `http.rs` — kept
/// as a small self-contained duplicate rather than sharing that module's
/// private guard across modules.
fn write_pidfile() -> Result<PidGuard> {
    let path = crate::db::matrix_pidfile_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, std::process::id().to_string())?;
    Ok(PidGuard(path))
}

pub struct EventDecision {
    pub should_handle: bool,
    pub is_dm: bool,
}

pub fn decide(
    settings: &MatrixSettings,
    _room_id: &str,
    is_dm: bool,
    sender_user_id: &str,
    is_own_message: bool,
    mentions_bot: bool,
) -> EventDecision {
    if is_own_message {
        return EventDecision {
            should_handle: false,
            is_dm,
        };
    }
    let should_handle = if is_dm {
        settings.allowed_users.iter().any(|u| u == sender_user_id)
    } else {
        mentions_bot
    };
    EventDecision {
        should_handle,
        is_dm,
    }
}

/// Current time as a string, matching the epoch-seconds-as-string pattern
/// already used elsewhere in this codebase (see `sync.rs`, `store.rs`).
fn now_ts() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

/// Marks a room as having an active session with a fresh `last_active_at`
/// timestamp, adding a new `RoomStatus` entry if the room isn't already
/// tracked (e.g. an unmapped room that isn't in `settings.rooms`).
async fn mark_room_active(status_reply: &Arc<Mutex<StatusReply>>, room_id: &str) {
    let mut r = status_reply.lock().await;
    if let Some(room) = r.rooms.iter_mut().find(|room| room.room_id == room_id) {
        room.active_session = true;
        room.last_active_at = Some(now_ts());
    } else {
        r.rooms.push(crate::matrix::status::RoomStatus {
            room_id: room_id.to_string(),
            alias: None,
            active_session: true,
            last_active_at: Some(now_ts()),
        });
    }
}

/// Marks a room's session as no longer active (e.g. after `!hm reset`),
/// leaving `last_active_at` untouched since it still reflects the last time
/// the room was genuinely active.
async fn mark_room_inactive(status_reply: &Arc<Mutex<StatusReply>>, room_id: &str) {
    let mut r = status_reply.lock().await;
    if let Some(room) = r.rooms.iter_mut().find(|room| room.room_id == room_id) {
        room.active_session = false;
    }
}

/// Loads the saved session from the OS keyring (bounded by a timeout so a
/// dead/unreachable Secret Service fails fast instead of hanging forever)
/// and restores a logged-in [`matrix_sdk::Client`] from it.
pub async fn restore_client(settings: &MatrixSettings) -> Result<matrix_sdk::Client> {
    use matrix_sdk::Client;

    tracing::debug!(user_id = %settings.user_id, "loading saved session from OS keyring");
    let keyring_user_id = settings.user_id.clone();
    let session_json = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::task::spawn_blocking(move || KeyringSessionStore.load(&keyring_user_id)),
    )
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "timed out reading session from the OS keyring after 10s — is a keyring daemon \
             running and unlocked? (e.g. `systemctl --user start gnome-keyring-daemon`)"
        )
    })???
    .ok_or_else(|| anyhow::anyhow!("no saved session — run `mynd matrix login` first"))?;
    let session: matrix_sdk::authentication::matrix::MatrixSession =
        serde_json::from_str(&session_json)?;
    tracing::debug!("session loaded from keyring");

    tracing::debug!(homeserver = %settings.homeserver_url, "building matrix client");
    let client = Client::builder()
        .homeserver_url(&settings.homeserver_url)
        .sqlite_store(crate::db::xdg_data_dir().join("matrix-store"), None)
        .build()
        .await?;
    tracing::debug!("client built, restoring session");
    client.restore_session(session).await?;
    tracing::info!(user_id = %settings.user_id, "matrix login succeeded, session restored");
    Ok(client)
}

/// Marks a room as a DM with `user_id` in the bot's `m.direct` account data,
/// so future calls to `Client::get_dm_room` find it instead of spawning a
/// duplicate DM. Failures are logged, not fatal — the room is still usable.
async fn mark_as_dm(
    client: &matrix_sdk::Client,
    room: &matrix_sdk::Room,
    user_id: &matrix_sdk::ruma::UserId,
) {
    if let Err(e) = client
        .account()
        .mark_as_dm(room.room_id(), &[user_id.to_owned()])
        .await
    {
        tracing::warn!(room_id = %room.room_id(), error = %e, "failed to mark room as DM");
    }
}

/// Finds an existing DM with `user_id`: first via `m.direct` account data,
/// then by joining a pending invite from that user if one exists, and only
/// creates a brand-new DM room as a last resort. Without the invite check, a
/// DM the bot was invited to but never joined would be invisible to
/// `get_dm_room`, so every call would spawn a fresh duplicate room.
async fn find_or_join_dm_room(
    client: &matrix_sdk::Client,
    user_id: &matrix_sdk::ruma::UserId,
) -> Result<matrix_sdk::Room> {
    if let Some(room) = client.get_dm_room(user_id) {
        tracing::debug!(room_id = %room.room_id(), "found existing DM room");
        return Ok(room);
    }

    for room in client.invited_rooms() {
        if let Ok(invite) = room.invite_details().await
            && invite.inviter_id == user_id
        {
            tracing::info!(room_id = %room.room_id(), sender = %user_id, "found pending invite from target user, joining");
            room.join().await?;
            mark_as_dm(client, &room, user_id).await;
            return Ok(room);
        }
    }

    tracing::debug!(to = %user_id, "no existing DM room, creating one");
    let room = client.create_dm(user_id).await?;
    mark_as_dm(client, &room, user_id).await;
    Ok(room)
}

/// Sends a text message to the given user's DM room, creating the DM if one
/// doesn't already exist. Used for one-off connectivity checks (`mynd
/// matrix send`) independent of the daemon's sync loop.
pub async fn send_direct_message(
    settings: &MatrixSettings,
    to_user_id: &str,
    message: &str,
) -> Result<()> {
    use matrix_sdk::ruma::UserId;
    use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;

    let client = restore_client(settings).await?;
    let user_id = <&UserId>::try_from(to_user_id)
        .map_err(|e| anyhow::anyhow!("invalid user id {to_user_id:?}: {e}"))?;

    tracing::debug!("running initial sync before sending");
    client
        .sync_once(matrix_sdk::config::SyncSettings::default())
        .await?;

    let room = find_or_join_dm_room(&client, user_id).await?;

    tracing::debug!(room_id = %room.room_id(), "sending message");
    room.send(RoomMessageEventContent::text_plain(message))
        .await?;
    tracing::info!(room_id = %room.room_id(), to = %user_id, "message sent");
    Ok(())
}

pub async fn run(settings: MatrixSettings, agent: AgentSettings, mynd_bin: String) -> Result<()> {
    use matrix_sdk::config::SyncSettings as MatrixSyncSettings;
    use matrix_sdk::ruma::events::room::member::StrippedRoomMemberEvent;
    use matrix_sdk::ruma::events::room::message::{MessageType, OriginalSyncRoomMessageEvent};
    use matrix_sdk::{Room, RoomState};

    let client = restore_client(&settings).await?;
    tracing::debug!("starting initial sync");
    let _pid_guard = write_pidfile()?;

    let status_reply = Arc::new(Mutex::new(StatusReply {
        logged_in: true,
        user_id: settings.user_id.clone(),
        sync_state: "connecting".to_string(),
        last_sync_at: None,
        rooms: settings
            .rooms
            .iter()
            .map(|r| crate::matrix::status::RoomStatus {
                room_id: r.room_id.clone(),
                alias: r.alias.clone(),
                active_session: false,
                last_active_at: None,
            })
            .collect(),
    }));
    let socket_status = status_reply.clone();
    let socket_path = crate::matrix::status::socket_path();
    tokio::spawn(async move {
        if let Err(e) = crate::matrix::status::serve_status(&socket_path, socket_status).await {
            tracing::warn!("status socket exited: {e:#}");
        }
    });

    let sessions = crate::matrix::session::SessionMap::new(std::time::Duration::from_secs(
        settings.session_ttl_seconds,
    ));
    let bot_user_id = settings.user_id.clone();
    let settings = Arc::new(settings);
    let agent = Arc::new(agent);
    let mynd_bin = Arc::new(mynd_bin);

    let invite_bot_user_id = bot_user_id.clone();
    let invite_client = client.clone();
    client.add_event_handler(move |room_member: StrippedRoomMemberEvent, room: Room| {
        let bot_user_id = invite_bot_user_id.clone();
        let client = invite_client.clone();
        async move {
            if room_member.state_key.as_str() != bot_user_id || room.state() != RoomState::Invited
            {
                return;
            }
            let is_direct = room_member.content.is_direct.unwrap_or(false);
            tracing::info!(room_id = %room.room_id(), sender = %room_member.sender, is_direct, "invited to room, auto-joining");
            match room.join().await {
                Ok(()) => {
                    tracing::info!(room_id = %room.room_id(), "joined room");
                    if is_direct {
                        mark_as_dm(&client, &room, &room_member.sender).await;
                    }
                }
                Err(e) => tracing::warn!(room_id = %room.room_id(), error = %e, "failed to join invited room"),
            }
        }
    });

    let handler_status_reply = status_reply.clone();
    client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: Room| {
        let settings = settings.clone();
        let agent = agent.clone();
        let mynd_bin = mynd_bin.clone();
        let sessions = sessions.clone();
        let bot_user_id = bot_user_id.clone();
        let status_reply = handler_status_reply.clone();
        async move {
            if room.state() != RoomState::Joined {
                tracing::debug!(
                    room_id = %room.room_id(),
                    state = ?room.state(),
                    "ignoring message in room bot has not joined"
                );
                return;
            }
            let is_own_message = event.sender.as_str() == bot_user_id;
            let is_dm = room.is_direct().await.unwrap_or(false);
            let MessageType::Text(text) = &event.content.msgtype else { return };
            let mentions_bot = text.body.contains(&bot_user_id);
            tracing::debug!(
                room_id = %room.room_id(),
                sender = %event.sender,
                is_dm,
                mentions_bot,
                "message received"
            );
            let decision = decide(
                &settings,
                room.room_id().as_str(),
                is_dm,
                event.sender.as_str(),
                is_own_message,
                mentions_bot,
            );
            if !decision.should_handle {
                if is_dm && !is_own_message {
                    tracing::debug!(sender = %event.sender, "DM from non-allowed user, ignoring");
                } else {
                    tracing::debug!(sender = %event.sender, "message not handled (no mention or own message)");
                }
                return;
            }
            tracing::debug!(sender = %event.sender, room_id = %room.room_id(), "sender authorized, handling message");
            {
                let mut r = status_reply.lock().await;
                r.last_sync_at = Some(now_ts());
            }
            match crate::matrix::commands::parse(&text.body) {
                crate::matrix::commands::Command::Reset => {
                    sessions.reset(room.room_id().as_str()).await;
                    mark_room_inactive(&status_reply, room.room_id().as_str()).await;
                }
                crate::matrix::commands::Command::Store(memory_text) => {
                    let target = crate::matrix::rooms::resolve_target(&settings, room.room_id().as_str(), is_dm);
                    tracing::debug!(room_id = %room.room_id(), "storing memory");
                    match crate::matrix::store_direct::store_memory(&mynd_bin, &memory_text, &target).await {
                        Ok(()) => {
                            mark_room_active(&status_reply, room.room_id().as_str()).await;
                            let _ = room.send(matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain("Stored.")).await;
                        }
                        Err(e) => {
                            let _ = room.send(matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(format!("mynd matrix failed to store that: {e}"))).await;
                        }
                    }
                }
                crate::matrix::commands::Command::Chat(message) => {
                    let target = crate::matrix::rooms::resolve_target(&settings, room.room_id().as_str(), is_dm);
                    let system_prompt = crate::matrix::rooms::context_system_prompt(&target);
                    let resume = sessions.get(room.room_id().as_str()).await;
                    match &resume {
                        Some(id) => tracing::debug!(room_id = %room.room_id(), session_id = %id, "resuming session"),
                        None => tracing::debug!(room_id = %room.room_id(), "spawning new session"),
                    }
                    match crate::chat_bot::agent::run_turn(&agent, &mynd_bin, &message, resume.as_deref(), Some(&system_prompt)).await {
                        Ok(result) => {
                            tracing::debug!(
                                room_id = %room.room_id(),
                                session_id = %result.session_id,
                                reply = %result.reply_text,
                                "agent response"
                            );
                            sessions.set(room.room_id().as_str(), result.session_id).await;
                            mark_room_active(&status_reply, room.room_id().as_str()).await;
                            let _ = room.send(matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(result.reply_text)).await;
                        }
                        Err(e) => {
                            tracing::debug!(room_id = %room.room_id(), error = %e, "agent turn failed");
                            sessions.reset(room.room_id().as_str()).await;
                            mark_room_inactive(&status_reply, room.room_id().as_str()).await;
                            let _ = room.send(matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(format!("mynd matrix hit an error: {e}"))).await;
                        }
                    }
                }
            }
        }
    });

    {
        let mut r = status_reply.lock().await;
        r.sync_state = "synced".to_string();
        r.last_sync_at = Some(now_ts());
    }
    client.sync(MatrixSyncSettings::default()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MatrixRoomMapping;

    fn settings() -> MatrixSettings {
        MatrixSettings {
            homeserver_url: "https://matrix.org".into(),
            user_id: "@bot:matrix.org".into(),
            allowed_users: vec!["@you:matrix.org".into()],
            rooms: vec![MatrixRoomMapping {
                room_id: "!abc:matrix.org".into(),
                alias: None,
                base_tags: vec!["project:mynd".into()],
            }],
            session_ttl_seconds: crate::config::DEFAULT_SESSION_TTL_SECONDS,
        }
    }

    #[test]
    fn own_messages_are_never_handled() {
        let d = decide(
            &settings(),
            "!abc:matrix.org",
            false,
            "@bot:matrix.org",
            true,
            true,
        );
        assert!(!d.should_handle);
    }

    #[test]
    fn dm_from_allowed_user_is_handled() {
        let d = decide(
            &settings(),
            "!dm:matrix.org",
            true,
            "@you:matrix.org",
            false,
            false,
        );
        assert!(d.should_handle);
        assert!(d.is_dm);
    }

    #[test]
    fn dm_from_non_allowed_user_is_silently_ignored() {
        let d = decide(
            &settings(),
            "!dm:matrix.org",
            true,
            "@stranger:matrix.org",
            false,
            false,
        );
        assert!(!d.should_handle);
    }

    #[test]
    fn room_message_without_mention_is_ignored() {
        let d = decide(
            &settings(),
            "!abc:matrix.org",
            false,
            "@you:matrix.org",
            false,
            false,
        );
        assert!(!d.should_handle);
    }

    #[test]
    fn room_message_with_mention_is_handled_regardless_of_sender() {
        let d = decide(
            &settings(),
            "!abc:matrix.org",
            false,
            "@anyone:matrix.org",
            false,
            true,
        );
        assert!(d.should_handle);
        assert!(!d.is_dm);
    }
}
