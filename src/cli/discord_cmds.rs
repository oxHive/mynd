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
            print!(
                "{}",
                format_login_success(&current_user.name, &application_id)
            );
            anyhow::Ok(())
        })
}

/// Pure formatting for `cmd_discord_login`'s success output, split out so the
/// message text can be unit tested without a real Discord token exchange.
fn format_login_success(user_name: &str, application_id: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Logged in as {user_name} (application id {application_id})."
    );
    let _ = writeln!(
        out,
        "Token saved to the OS keyring. Run `hivemind discord run` to start the bot."
    );
    out
}

pub fn cmd_discord_status() -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let socket_path = crate::discord::status::socket_path();
            match crate::discord::status::query_status(&socket_path).await {
                Ok(reply) => {
                    print!("{}", format_status_reply(&reply));
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

/// Pure formatting for `cmd_discord_status`'s successful-reply output, split
/// out so the various branches (empty/populated channels, alias fallback,
/// active-session labeling) can be unit tested without a real status socket.
fn format_status_reply(reply: &crate::discord::status::StatusReply) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "logged_in:      {}", reply.logged_in);
    let _ = writeln!(out, "application_id: {}", reply.application_id);
    let _ = writeln!(out, "sync_state:     {}", reply.sync_state);
    if let Some(t) = &reply.last_sync_at {
        let _ = writeln!(out, "last_sync:      {t}");
    }
    if reply.channels.is_empty() {
        let _ = writeln!(out, "channels:       (none)");
    } else {
        let _ = writeln!(out, "channels:");
        for channel in &reply.channels {
            let label = channel.alias.as_deref().unwrap_or(&channel.channel_id);
            let session = if channel.active_session {
                "active session"
            } else {
                "no active session"
            };
            let _ = writeln!(out, "  {label}  ({session})");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::status::{ChannelStatus, StatusReply};

    #[test]
    fn format_login_success_includes_name_and_application_id() {
        let out = format_login_success("hivemind-bot", "999999999999999999");
        assert!(out.contains("Logged in as hivemind-bot (application id 999999999999999999)."));
        assert!(out.contains("Run `hivemind discord run` to start the bot."));
    }

    fn base_reply() -> StatusReply {
        StatusReply {
            logged_in: true,
            application_id: "999999999999999999".into(),
            sync_state: "connected".into(),
            last_sync_at: None,
            channels: vec![],
        }
    }

    #[test]
    fn format_status_reply_shows_none_when_there_are_no_channels() {
        let out = format_status_reply(&base_reply());
        assert!(out.contains("channels:       (none)"));
    }

    #[test]
    fn format_status_reply_omits_last_sync_when_absent() {
        let out = format_status_reply(&base_reply());
        assert!(!out.contains("last_sync:"));
    }

    #[test]
    fn format_status_reply_includes_last_sync_when_present() {
        let mut reply = base_reply();
        reply.last_sync_at = Some("1700000000".into());
        let out = format_status_reply(&reply);
        assert!(out.contains("last_sync:      1700000000"));
    }

    #[test]
    fn format_status_reply_uses_alias_when_present() {
        let mut reply = base_reply();
        reply.channels.push(ChannelStatus {
            channel_id: "222222222222222222".into(),
            alias: Some("project-hivemind".into()),
            active_session: true,
            last_active_at: None,
        });
        let out = format_status_reply(&reply);
        assert!(out.contains("project-hivemind  (active session)"));
        assert!(!out.contains("222222222222222222"));
    }

    #[test]
    fn format_status_reply_falls_back_to_channel_id_without_alias() {
        let mut reply = base_reply();
        reply.channels.push(ChannelStatus {
            channel_id: "222222222222222222".into(),
            alias: None,
            active_session: false,
            last_active_at: None,
        });
        let out = format_status_reply(&reply);
        assert!(out.contains("222222222222222222  (no active session)"));
    }

    #[test]
    fn format_status_reply_lists_multiple_channels() {
        let mut reply = base_reply();
        reply.channels.push(ChannelStatus {
            channel_id: "111".into(),
            alias: Some("a".into()),
            active_session: true,
            last_active_at: None,
        });
        reply.channels.push(ChannelStatus {
            channel_id: "222".into(),
            alias: Some("b".into()),
            active_session: false,
            last_active_at: None,
        });
        let out = format_status_reply(&reply);
        assert!(out.contains("a  (active session)"));
        assert!(out.contains("b  (no active session)"));
    }
}
