use crate::config::AgentSettings;
use crate::config::DiscordSettings;
use crate::discord::status::{ChannelStatus, StatusReply};
use crate::discord::token_store::{KeyringTokenStore, TokenStore};
use anyhow::Result;
use serenity::all::{
    ChannelId, Command, CommandOptionType, Context, CreateCommand, CreateCommandOption,
    CreateInteractionResponse, CreateInteractionResponseFollowup, CreateInteractionResponseMessage,
    EventHandler, GatewayIntents, Interaction, Message, Ready, ResolvedValue,
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
    EventDecision {
        should_handle,
        is_dm,
    }
}

/// Whether a `/hm` interaction from `author_id` is authorized: DMs are
/// gated by `[discord] allowed_users` (mirroring `decide()`'s DM path
/// above), while guild interactions are always allowed here since Discord's
/// own `default_member_permissions` (`[discord] permission_gate`) already
/// gates who can invoke the command in a guild.
pub fn interaction_authorized(settings: &DiscordSettings, is_dm: bool, author_id: &str) -> bool {
    !is_dm || settings.allowed_users.iter().any(|u| u == author_id)
}

/// Maps the `[discord] permission_gate` config string to a Discord permission
/// bit, used as `/hm`'s `default_member_permissions` at registration time.
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
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "store",
                "Directly store a memory",
            )
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

/// Discord rejects message content over 2000 UTF-16 code units client-side
/// (`Error::Model(ModelError::MessageTooLong(..))`), before it ever reaches
/// the API. Splits `text` into chunks of at most `max_len` characters,
/// preferring to break on a newline near the limit so multi-line replies
/// don't get cut mid-line when a good break point is available.
fn chunk_message(text: &str, max_len: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_len {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let remaining = chars.len() - start;
        if remaining <= max_len {
            chunks.push(chars[start..].iter().collect());
            break;
        }
        let mut end = start + max_len;
        let search_start = start + max_len / 2;
        if let Some(rel_nl) = chars[search_start..end].iter().rposition(|&c| c == '\n') {
            end = search_start + rel_nl + 1;
        }
        chunks.push(chars[start..end].iter().collect());
        start = end;
    }
    chunks
}

/// Sends `text` to `channel_id`, splitting it into multiple messages if it's
/// over Discord's 2000-character limit (see [`chunk_message`]) instead of
/// letting `say` fail client-side and silently dropping the reply. Each
/// chunk is awaited before the next is sent, so ordering is preserved.
async fn send_chunked(ctx: &Context, channel_id: ChannelId, text: &str) {
    const MAX_CHUNK_LEN: usize = 1900;
    for chunk in chunk_message(text, MAX_CHUNK_LEN) {
        if let Err(e) = channel_id.say(&ctx.http, &chunk).await {
            tracing::warn!(%channel_id, error = %e, "failed to send Discord message chunk");
        }
    }
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

/// Sends an ephemeral deferred response, telling Discord "processing" before
/// the 3-second interaction-response deadline expires. Used ahead of slow
/// operations (like `/hm store`'s spawn-and-MCP-handshake) whose result is
/// then delivered via [`respond_followup`] instead of `create_response`,
/// since the initial response can only be sent once.
async fn defer_ephemeral(ctx: &Context, command: &serenity::all::CommandInteraction) {
    if let Err(e) = command
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Defer(
                CreateInteractionResponseMessage::new().ephemeral(true),
            ),
        )
        .await
    {
        tracing::warn!(error = %e, "failed to defer /hm interaction response");
    }
}

/// Delivers the actual result for an interaction whose response was already
/// deferred via [`defer_ephemeral`].
async fn respond_followup(ctx: &Context, command: &serenity::all::CommandInteraction, text: &str) {
    if let Err(e) = command
        .create_followup(
            &ctx.http,
            CreateInteractionResponseFollowup::new()
                .content(text)
                .ephemeral(true),
        )
        .await
    {
        tracing::warn!(error = %e, "failed to send /hm interaction followup");
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _ready: Ready) {
        tracing::debug!("discord gateway ready, registering /hm command");
        if let Err(e) =
            Command::create_global_command(&ctx.http, build_hm_command(self.permission_gate)).await
        {
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
        tracing::debug!(
            channel_id = %msg.channel_id,
            author = %msg.author.id,
            is_dm,
            mentions_bot,
            "message received"
        );
        let decision = crate::discord::daemon::decide(
            &self.settings,
            is_dm,
            msg.author.bot,
            &msg.author.id.to_string(),
            mentions_bot,
        );
        if !decision.should_handle {
            if is_dm {
                tracing::debug!(author = %msg.author.id, "DM from non-allowed user, ignoring");
            } else {
                tracing::debug!(author = %msg.author.id, "message not handled (no mention)");
            }
            return;
        }
        tracing::debug!(author = %msg.author.id, channel_id = %msg.channel_id, "sender authorized, handling message");

        let channel_id = msg.channel_id.to_string();
        {
            let mut r = self.status_reply.lock().await;
            r.last_sync_at = Some(now_ts());
        }
        let target = crate::discord::channels::resolve_target(&self.settings, &channel_id, is_dm);
        let system_prompt = crate::discord::channels::context_system_prompt(&target);
        let resume = self.sessions.get(&channel_id).await;
        match &resume {
            Some(id) => {
                tracing::debug!(channel_id = %channel_id, session_id = %id, "resuming session")
            }
            None => tracing::debug!(channel_id = %channel_id, "spawning new session"),
        }
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
                tracing::debug!(
                    channel_id = %channel_id,
                    session_id = %result.session_id,
                    reply = %result.reply_text,
                    "agent response"
                );
                self.sessions.set(&channel_id, result.session_id).await;
                mark_channel_active(&self.status_reply, &channel_id).await;
                send_chunked(&ctx, msg.channel_id, &result.reply_text).await;
            }
            Err(e) => {
                tracing::debug!(channel_id = %channel_id, error = %e, "agent turn failed");
                self.sessions.reset(&channel_id).await;
                mark_channel_inactive(&self.status_reply, &channel_id).await;
                send_chunked(
                    &ctx,
                    msg.channel_id,
                    &format!("hivemind discord hit an error: {e}"),
                )
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
        tracing::debug!(
            channel_id = %command.channel_id,
            author = %author_id,
            is_dm,
            "interaction received"
        );
        if !interaction_authorized(&self.settings, is_dm, &author_id) {
            tracing::debug!(
                author = %author_id,
                is_dm,
                "interaction not authorized (DM from non-allowed user), ignoring"
            );
            return;
        }

        let Some(top) = command.data.options().into_iter().next() else {
            return;
        };
        let ResolvedValue::SubCommand(sub_opts) = top.value else {
            return;
        };
        let channel_id = command.channel_id.to_string();
        tracing::debug!(channel_id = %channel_id, subcommand = top.name, "dispatching /hm subcommand");

        match top.name {
            "help" => respond_ephemeral(&ctx, &command, HELP_TEXT).await,
            "reset" => {
                tracing::debug!(channel_id = %channel_id, "resetting session");
                self.sessions.reset(&channel_id).await;
                mark_channel_inactive(&self.status_reply, &channel_id).await;
                respond_ephemeral(&ctx, &command, "Reset.").await;
            }
            "store" => {
                // `store_memory` spawns the `hivemind` binary and does an MCP
                // handshake, which can outlast Discord's 3-second
                // interaction-response deadline. Defer immediately so the
                // interaction token stays valid, then deliver the real
                // result via a followup once it's done.
                defer_ephemeral(&ctx, &command).await;
                let text = sub_opts.iter().find_map(|o| match &o.value {
                    ResolvedValue::String(s) if o.name == "text" => Some(s.to_string()),
                    _ => None,
                });
                let Some(text) = text else {
                    respond_followup(&ctx, &command, "Missing text.").await;
                    return;
                };
                let target =
                    crate::discord::channels::resolve_target(&self.settings, &channel_id, is_dm);
                tracing::debug!(channel_id = %channel_id, "storing memory via /hm store");
                match crate::discord::store_direct::store_memory(&self.hivemind_bin, &text, &target)
                    .await
                {
                    Ok(()) => {
                        tracing::debug!(channel_id = %channel_id, "/hm store succeeded");
                        mark_channel_active(&self.status_reply, &channel_id).await;
                        respond_followup(&ctx, &command, "Stored.").await;
                    }
                    Err(e) => {
                        tracing::debug!(channel_id = %channel_id, error = %e, "/hm store failed");
                        respond_followup(
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

pub async fn run(
    settings: DiscordSettings,
    agent: AgentSettings,
    hivemind_bin: String,
) -> Result<()> {
    tracing::debug!(application_id = %settings.application_id, "loading saved bot token from OS keyring");
    let application_id = settings.application_id.clone();
    let token = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::task::spawn_blocking(move || KeyringTokenStore.load(&application_id)),
    )
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "timed out reading bot token from the OS keyring after 10s: is a keyring daemon \
             running and unlocked? (e.g. `systemctl --user start gnome-keyring-daemon`)"
        )
    })???
    .ok_or_else(|| anyhow::anyhow!("no saved bot token — run `hivemind discord login` first"))?;
    tracing::debug!("bot token loaded from keyring");

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
    let application_id = settings.application_id.clone();
    let token = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::task::spawn_blocking(move || KeyringTokenStore.load(&application_id)),
    )
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "timed out reading bot token from the OS keyring after 10s: is a keyring daemon \
             running and unlocked? (e.g. `systemctl --user start gnome-keyring-daemon`)"
        )
    })???
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
    fn chunk_message_returns_single_chunk_when_under_the_limit() {
        let chunks = chunk_message("hello world", 1900);
        assert_eq!(chunks, vec!["hello world".to_string()]);
    }

    #[test]
    fn chunk_message_splits_text_over_the_limit() {
        let text = "a".repeat(50);
        let chunks = chunk_message(&text, 20);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].chars().count(), 20);
        assert_eq!(chunks[1].chars().count(), 20);
        assert_eq!(chunks[2].chars().count(), 10);
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn chunk_message_prefers_breaking_on_a_newline_near_the_limit() {
        let text = format!("{}\n{}", "a".repeat(15), "b".repeat(15));
        let chunks = chunk_message(&text, 20);
        assert_eq!(chunks[0], format!("{}\n", "a".repeat(15)));
        assert_eq!(chunks[1], "b".repeat(15));
    }

    #[test]
    fn interaction_from_dm_by_allowed_user_is_authorized() {
        assert!(interaction_authorized(
            &settings(),
            true,
            "111111111111111111"
        ));
    }

    #[test]
    fn interaction_from_dm_by_non_allowed_user_is_not_authorized() {
        assert!(!interaction_authorized(
            &settings(),
            true,
            "333333333333333333"
        ));
    }

    #[test]
    fn interaction_from_guild_is_always_authorized_regardless_of_allowed_users() {
        assert!(interaction_authorized(
            &settings(),
            false,
            "444444444444444444"
        ));
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

    #[test]
    fn parse_permission_gate_accepts_all_remaining_known_values() {
        use serenity::model::Permissions;
        assert_eq!(
            parse_permission_gate("manage_channels").unwrap(),
            Permissions::MANAGE_CHANNELS
        );
        assert_eq!(
            parse_permission_gate("manage_messages").unwrap(),
            Permissions::MANAGE_MESSAGES
        );
        assert_eq!(
            parse_permission_gate("kick_members").unwrap(),
            Permissions::KICK_MEMBERS
        );
        assert_eq!(
            parse_permission_gate("ban_members").unwrap(),
            Permissions::BAN_MEMBERS
        );
    }

    #[test]
    fn chunk_message_returns_single_chunk_when_exactly_at_the_limit() {
        let text = "a".repeat(20);
        let chunks = chunk_message(&text, 20);
        assert_eq!(chunks, vec![text]);
    }

    #[test]
    fn chunk_message_handles_empty_text() {
        let chunks = chunk_message("", 20);
        assert_eq!(chunks, vec!["".to_string()]);
    }

    #[test]
    fn now_ts_returns_a_parseable_unix_timestamp() {
        let ts = now_ts();
        let parsed: u64 = ts
            .parse()
            .expect("now_ts should return a plain integer string");
        // Sanity bound: any time after 2020-01-01 in unix seconds.
        assert!(parsed > 1_577_836_800);
    }

    #[tokio::test]
    async fn mark_channel_active_adds_a_new_channel_when_absent() {
        let status = Arc::new(Mutex::new(StatusReply {
            logged_in: true,
            application_id: "1".into(),
            sync_state: "connected".into(),
            last_sync_at: None,
            channels: vec![],
        }));
        mark_channel_active(&status, "123").await;
        let r = status.lock().await;
        assert_eq!(r.channels.len(), 1);
        assert_eq!(r.channels[0].channel_id, "123");
        assert!(r.channels[0].active_session);
        assert!(r.channels[0].last_active_at.is_some());
    }

    #[tokio::test]
    async fn mark_channel_active_updates_an_existing_channel() {
        let status = Arc::new(Mutex::new(StatusReply {
            logged_in: true,
            application_id: "1".into(),
            sync_state: "connected".into(),
            last_sync_at: None,
            channels: vec![ChannelStatus {
                channel_id: "123".into(),
                alias: Some("proj".into()),
                active_session: false,
                last_active_at: None,
            }],
        }));
        mark_channel_active(&status, "123").await;
        let r = status.lock().await;
        assert_eq!(r.channels.len(), 1);
        assert_eq!(r.channels[0].alias.as_deref(), Some("proj"));
        assert!(r.channels[0].active_session);
        assert!(r.channels[0].last_active_at.is_some());
    }

    #[tokio::test]
    async fn mark_channel_inactive_updates_an_existing_channel() {
        let status = Arc::new(Mutex::new(StatusReply {
            logged_in: true,
            application_id: "1".into(),
            sync_state: "connected".into(),
            last_sync_at: None,
            channels: vec![ChannelStatus {
                channel_id: "123".into(),
                alias: None,
                active_session: true,
                last_active_at: Some("100".into()),
            }],
        }));
        mark_channel_inactive(&status, "123").await;
        let r = status.lock().await;
        assert!(!r.channels[0].active_session);
    }

    #[tokio::test]
    async fn mark_channel_inactive_is_a_noop_for_an_unknown_channel() {
        let status = Arc::new(Mutex::new(StatusReply {
            logged_in: true,
            application_id: "1".into(),
            sync_state: "connected".into(),
            last_sync_at: None,
            channels: vec![],
        }));
        mark_channel_inactive(&status, "does-not-exist").await;
        let r = status.lock().await;
        assert!(r.channels.is_empty());
    }

    #[test]
    fn build_hm_command_without_permission_gate_has_no_default_permissions() {
        let cmd = build_hm_command(None);
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["name"], "hm");
        assert!(json.get("default_member_permissions").is_none());
        let options = json["options"].as_array().unwrap();
        let names: Vec<&str> = options
            .iter()
            .map(|o| o["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["store", "reset", "help"]);
    }

    #[test]
    fn build_hm_command_with_permission_gate_sets_default_permissions() {
        use serenity::model::Permissions;
        let cmd = build_hm_command(Some(Permissions::MANAGE_GUILD));
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(
            json["default_member_permissions"],
            Permissions::MANAGE_GUILD.bits().to_string()
        );
    }

    #[test]
    fn build_hm_command_store_option_requires_text() {
        let cmd = build_hm_command(None);
        let json = serde_json::to_value(&cmd).unwrap();
        let store = json["options"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == "store")
            .unwrap();
        let sub_opts = store["options"].as_array().unwrap();
        let text_opt = sub_opts.iter().find(|o| o["name"] == "text").unwrap();
        assert_eq!(text_opt["required"], true);
    }

    #[test]
    fn write_pidfile_writes_current_pid_and_removes_it_on_drop() {
        let _lock = crate::test_env_lock::ENV_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: test-only env mutation; serialised by ENV_MUTEX.
        unsafe { std::env::set_var("XDG_DATA_HOME", dir.path()) };

        let path = crate::db::discord_pidfile_path();
        {
            let guard = write_pidfile().unwrap();
            assert_eq!(guard.0, path);
            let contents = std::fs::read_to_string(&path).unwrap();
            assert_eq!(contents, std::process::id().to_string());
        }
        assert!(
            !path.exists(),
            "pidfile should be removed once the guard drops"
        );

        unsafe { std::env::remove_var("XDG_DATA_HOME") };
    }
}
