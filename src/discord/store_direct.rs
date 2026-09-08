const MAX_TITLE_CHARS: usize = 60;

/// Derives a short title from free-form content for direct-store memories
/// (the `/hm store` path has no separate title field — just the text option).
pub fn derive_title(content: &str) -> String {
    let trimmed = content.trim();
    let char_count = trimmed.chars().count();
    if char_count <= MAX_TITLE_CHARS {
        return trimmed.to_string();
    }
    let truncated: String = trimmed.chars().take(MAX_TITLE_CHARS).collect();
    format!("{truncated}…")
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
