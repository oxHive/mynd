use crate::config::MatrixSettings;

pub struct MemoryTarget {
    pub layer: &'static str,
    pub tags: Vec<String>,
}

pub fn resolve_target(settings: &MatrixSettings, room_id: &str, is_dm: bool) -> MemoryTarget {
    if is_dm {
        return MemoryTarget {
            layer: "personal",
            tags: vec!["source:matrix".to_string()],
        };
    }
    if let Some(mapping) = settings.rooms.iter().find(|r| r.room_id == room_id) {
        return MemoryTarget {
            layer: "workspace",
            tags: mapping.base_tags.clone(),
        };
    }
    MemoryTarget {
        layer: "workspace",
        tags: vec![format!("room:{room_id}"), "source:matrix".to_string()],
    }
}

/// Instruction for the agent's system prompt (not spliced into the user
/// message) so it can't be confused with attacker-controlled text arriving
/// in the DM/room message itself.
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
    use crate::config::MatrixRoomMapping;

    fn settings_with_room(mapping: MatrixRoomMapping) -> MatrixSettings {
        MatrixSettings {
            homeserver_url: "https://matrix.org".into(),
            user_id: "@bot:matrix.org".into(),
            allowed_users: vec![],
            rooms: vec![mapping],
            session_ttl_seconds: crate::config::DEFAULT_SESSION_TTL_SECONDS,
        }
    }

    #[test]
    fn dm_maps_to_personal_layer_with_source_tag() {
        let settings = settings_with_room(MatrixRoomMapping {
            room_id: "!other:matrix.org".into(),
            alias: None,
            base_tags: vec!["project:mynd".into()],
        });
        let target = resolve_target(&settings, "!dm-room:matrix.org", true);
        assert_eq!(target.layer, "personal");
        assert_eq!(target.tags, vec!["source:matrix".to_string()]);
    }

    #[test]
    fn mapped_room_uses_configured_base_tags() {
        let settings = settings_with_room(MatrixRoomMapping {
            room_id: "!abc:matrix.org".into(),
            alias: Some("mynd-project".into()),
            base_tags: vec!["project:mynd".into(), "topic:matrix".into()],
        });
        let target = resolve_target(&settings, "!abc:matrix.org", false);
        assert_eq!(target.layer, "workspace");
        assert_eq!(
            target.tags,
            vec!["project:mynd".to_string(), "topic:matrix".to_string()]
        );
    }

    #[test]
    fn unmapped_room_falls_back_to_room_id() {
        let settings = MatrixSettings {
            homeserver_url: "https://matrix.org".into(),
            user_id: "@bot:matrix.org".into(),
            allowed_users: vec![],
            rooms: vec![],
            session_ttl_seconds: crate::config::DEFAULT_SESSION_TTL_SECONDS,
        };
        let target = resolve_target(&settings, "!unmapped:matrix.org", false);
        assert_eq!(target.layer, "workspace");
        assert_eq!(
            target.tags,
            vec![
                "room:!unmapped:matrix.org".to_string(),
                "source:matrix".to_string()
            ]
        );
    }

    #[test]
    fn context_system_prompt_includes_layer_and_tags() {
        let target = MemoryTarget {
            layer: "workspace",
            tags: vec!["project:mynd".to_string(), "topic:matrix".to_string()],
        };
        let prompt = context_system_prompt(&target);
        assert!(prompt.contains("workspace"));
        assert!(prompt.contains("project:mynd"));
        assert!(prompt.contains("topic:matrix"));
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
