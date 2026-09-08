use crate::config::DiscordSettings;

pub struct MemoryTarget {
    pub layer: &'static str,
    pub tags: Vec<String>,
}

pub fn resolve_target(settings: &DiscordSettings, channel_id: &str, is_dm: bool) -> MemoryTarget {
    if is_dm {
        return MemoryTarget {
            layer: "personal",
            tags: vec!["source:discord".to_string()],
        };
    }
    if let Some(mapping) = settings
        .channels
        .iter()
        .find(|c| c.channel_id == channel_id)
    {
        return MemoryTarget {
            layer: "workspace",
            tags: mapping.base_tags.clone(),
        };
    }
    MemoryTarget {
        layer: "workspace",
        tags: vec![
            format!("channel:{channel_id}"),
            "source:discord".to_string(),
        ],
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
