use crate::store::SqliteStore;
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ErrorData, PromptMessage, Role},
    prompt, prompt_handler, prompt_router, schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemoryStoreInput {
    /// Short descriptive title for this memory
    pub title: String,
    /// Full content to store
    pub content: String,
    /// Tags for search and auto-linking
    #[serde(default)]
    pub tags: Vec<String>,
    /// Optional token count hint
    #[serde(default)]
    pub token_count: Option<i64>,
    /// Memory layer: "personal" (follows the user) or "workspace" (project-scoped). Default: workspace.
    #[serde(default)]
    pub layer: Option<String>,
    /// Memory type: "preference" | "project" | "history". Default: project.
    #[serde(default)]
    pub memory_type: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemoryRecallInput {
    /// Memory ID (mem_xxx) — use this or title, not both
    #[serde(default)]
    pub id: Option<String>,
    /// Exact title — use this or id, not both
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemorySearchInput {
    /// Keywords to search memory titles and content. Optional if `tags` is
    /// provided — a pure tag-boolean search with no keyword component.
    #[serde(default)]
    pub query: Option<String>,
    /// Require all of these tags (namespace:value form, e.g. "lang:rust").
    /// ANDed together, and ANDed with `query` if both are provided.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Max results (default 5, capped at 10)
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemoryUpdateInput {
    /// ID of the memory to update (mem_xxx)
    pub id: String,
    /// New title (omit to keep current)
    #[serde(default)]
    pub title: Option<String>,
    /// New content (omit to keep current)
    #[serde(default)]
    pub content: Option<String>,
    /// Replace all tags with these (omit to keep current)
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemoryDeleteInput {
    /// ID of the memory to delete (mem_xxx)
    pub id: String,
    /// Must be true. Confirm with the user before deleting — deletion is permanent.
    pub confirm: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemorySearchPromptInput {
    /// Keywords to search for in memory titles and content
    pub query: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemoryIdInput {
    /// Memory ID to target (e.g. mem_abc123)
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemoryFlagInput {
    /// Memory ID to flag
    pub id: String,
    /// Reason for flagging: "incorrect", "outdated", "duplicate", "other"
    #[serde(default = "default_flag_reason")]
    pub reason: String,
    /// Optional note explaining the issue
    #[serde(default)]
    pub note: Option<String>,
}

fn default_flag_reason() -> String {
    "other".to_string()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConflictIdInput {
    /// Conflict ID to merge (conflict_xxx)
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TagNamespacesListInput {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SessionStartInput {
    /// Absolute path to the project root where .mynd.toml lives.
    pub project_path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemoryStoreEdgeInput {
    /// Source memory ID (mem_xxx)
    pub source_id: String,
    /// Target memory ID (mem_xxx)
    pub target_id: String,
    /// Relationship type: "parent" (target is a broader principle/context this
    /// falls under) | "child" (target is a specific instance of this) |
    /// "sibling" (a peer, no hierarchy implied)
    pub relationship: String,
    /// Optional lifecycle status: "active" (default, confirmed) or "pending"
    /// (a suggestion awaiting user review in the dashboard).
    #[serde(default)]
    pub status: Option<String>,
    /// Short rationale for the connection, shown to the user during review.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemoryUpdateEdgeInput {
    /// Edge ID to update (edge_xxx)
    pub id: String,
    /// New relationship: "parent" | "child" | "sibling". Omit to keep current.
    #[serde(default)]
    pub relationship: Option<String>,
    /// New rationale shown during review. Omit to keep current.
    #[serde(default)]
    pub reason: Option<String>,
    /// New mention anchor text. Omit to keep current.
    #[serde(default)]
    pub link_text: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MemoryGetEdgesInput {
    /// Memory ID (mem_xxx) to get connections for
    pub memory_id: String,
}

#[derive(Clone)]
pub struct Mynd {
    store: Arc<SqliteStore>,
    sync_trigger: Option<Arc<tokio::sync::Notify>>,
    events: Option<tokio::sync::broadcast::Sender<serde_json::Value>>,
}

impl Mynd {
    #[cfg(test)]
    pub fn new(store: SqliteStore) -> Self {
        Self {
            store: Arc::new(store),
            sync_trigger: None,
            events: None,
        }
    }

    pub fn with_store(store: Arc<SqliteStore>) -> Self {
        Self {
            store,
            sync_trigger: None,
            events: None,
        }
    }

    pub fn with_sync(store: Arc<SqliteStore>, trigger: Arc<tokio::sync::Notify>) -> Self {
        Self {
            store,
            sync_trigger: Some(trigger),
            events: None,
        }
    }

    /// Broadcasts a "changed" signal to dashboard SSE subscribers whenever a
    /// memory or edge is created/updated/deleted through this MCP handle.
    pub fn with_events(mut self, tx: tokio::sync::broadcast::Sender<serde_json::Value>) -> Self {
        self.events = Some(tx);
        self
    }

    fn notify_change(&self) {
        if let Some(tx) = &self.events {
            let _ = tx.send(serde_json::json!({ "type": "changed" }));
        }
    }

    /// Rejects content whose title+content token count exceeds the
    /// dashboard-configurable `max_content_tokens` guardrail (default
    /// `DEFAULT_MAX_CONTENT_TOKENS`). Applied to both `memory_store` and
    /// `memory_update` so the limit holds on edits too, not just creation.
    async fn check_content_size(&self, title: &str, content: &str) -> Result<(), ErrorData> {
        let tokens = crate::budget::count_entry_tokens(title, content) as i64;
        let limit = self.store.max_content_tokens().await;
        if tokens > limit {
            return Err(ErrorData::invalid_params(
                format!(
                    "content is {tokens} tokens, exceeds max_content_tokens ({limit}). \
                     Split into an index memory plus child memories, linked via \
                     [phrase](child:mem_xxx) — store each child first, then reference \
                     their real returned ids from the index's content."
                ),
                None,
            ));
        }
        Ok(())
    }

    pub async fn do_memory_store(&self, p: MemoryStoreInput) -> Result<CallToolResult, ErrorData> {
        let id = format!("mem_{}", uuid::Uuid::new_v4().simple());
        let title = p.title.clone();

        let layer = p.layer.as_deref().unwrap_or("workspace");
        layer
            .parse::<crate::model::Layer>()
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;

        let memory_type = p.memory_type.as_deref().unwrap_or("project");
        memory_type
            .parse::<crate::model::MemoryType>()
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;

        self.check_content_size(&p.title, &p.content).await?;

        self.store
            .store(&crate::store::NewMemoryRow {
                id: &id,
                title: &p.title,
                content: &p.content,
                tags: &p.tags,
                token_count: p.token_count,
                layer,
                memory_type,
            })
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        if let Some(t) = &self.sync_trigger {
            t.notify_one();
        }
        self.notify_change();
        Ok(CallToolResult::structured(json!({
            "id": id,
            "title": title,
        })))
    }

    pub async fn do_memory_recall(
        &self,
        p: MemoryRecallInput,
    ) -> Result<CallToolResult, ErrorData> {
        let entry = if let Some(ref id) = p.id {
            self.store.recall_by_id(id).await
        } else if let Some(ref title) = p.title {
            self.store.recall_by_title(title).await
        } else {
            return Err(ErrorData::invalid_params(
                "provide either 'id' or 'title'",
                None,
            ));
        }
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        match entry {
            None => Ok(CallToolResult::structured(json!({ "found": false }))),
            Some(e) => Ok(CallToolResult::structured(json!({
                "found": true,
                "id": e.id,
                "title": e.title,
                "content": e.content,
                "tags": e.tags,
                "created_at": e.created_at,
                "updated_at": e.updated_at,
                "layer": e.layer,
                "memory_type": e.memory_type,
            }))),
        }
    }

    pub async fn do_memory_search(
        &self,
        p: MemorySearchInput,
    ) -> Result<CallToolResult, ErrorData> {
        let limit = p.limit.unwrap_or(5).clamp(1, 10);
        let query = p.query.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let tags = p.tags.filter(|t| !t.is_empty());

        if query.is_none() && tags.is_none() {
            return Ok(CallToolResult::structured(json!({
                "count": 0,
                "results": [],
            })));
        }

        let hits = match (query, tags) {
            (Some(q), Some(tags)) => {
                let expr = crate::tag_query::TagExpr::and_all(&tags)
                    .expect("tags checked non-empty above");
                let candidates = self
                    .store
                    .search(q, 50)
                    .await
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                let mut filtered: Vec<_> = candidates
                    .into_iter()
                    .filter(|e| expr.eval(&e.tags))
                    .collect();
                filtered.truncate(limit as usize);
                filtered
            }
            (Some(q), None) => self
                .store
                .search(q, limit)
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
            (None, Some(tags)) => {
                let expr = crate::tag_query::TagExpr::and_all(&tags)
                    .expect("tags checked non-empty above");
                let mut results = self
                    .store
                    .find_by_tag_expr(&expr)
                    .await
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                results.truncate(limit as usize);
                results
            }
            (None, None) => unreachable!("handled by the early return above"),
        };

        let results: Vec<_> = hits
            .iter()
            .map(|h| {
                let snippet: String = h.content.chars().take(200).collect();
                json!({
                    "id": h.id,
                    "title": h.title,
                    "snippet": snippet,
                    "tags": h.tags,
                    "layer": h.layer,
                })
            })
            .collect();
        Ok(CallToolResult::structured(json!({
            "count": results.len(),
            "results": results,
        })))
    }

    /// Returns the tag namespace registry (color, allowed values,
    /// single_value flag, and human-written description per namespace) so
    /// an agent can pick correct namespaces/values instead of inventing new
    /// ones. Call this before tagging a new or edited memory.
    pub async fn do_tag_namespaces_list(
        &self,
        _p: TagNamespacesListInput,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::structured(
            self.store.tag_namespace_registry().await,
        ))
    }

    pub async fn do_memory_update(
        &self,
        p: MemoryUpdateInput,
    ) -> Result<CallToolResult, ErrorData> {
        // Fetch current state to fill in unchanged fields
        let current = self
            .store
            .recall_by_id(&p.id)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let current = match current {
            None => {
                return Ok(CallToolResult::structured(json!({
                    "updated": false,
                    "id": p.id,
                })));
            }
            Some(c) => c,
        };
        let title = p.title.as_deref().unwrap_or(&current.title);
        let content = p.content.as_deref().unwrap_or(&current.content);
        let tags = p.tags.as_deref().unwrap_or(&current.tags);

        self.check_content_size(title, content).await?;

        let updated = self
            .store
            .update(&p.id, title, content, tags)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        if updated {
            self.notify_change();
        }
        Ok(CallToolResult::structured(json!({
            "updated": updated,
            "id": p.id,
        })))
    }

    pub async fn do_memory_delete(
        &self,
        p: MemoryDeleteInput,
    ) -> Result<CallToolResult, ErrorData> {
        if !p.confirm {
            return Err(ErrorData::invalid_params(
                "Deletion is permanent and requires confirm: true. Confirm with the user first.",
                None,
            ));
        }
        let deleted = self
            .store
            .delete(&p.id)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        if deleted {
            self.notify_change();
        }
        Ok(CallToolResult::structured(json!({
            "deleted": deleted,
            "id": p.id,
        })))
    }

    async fn do_memory_list_prompt(&self) -> Result<Vec<PromptMessage>, ErrorData> {
        let memories = self
            .store
            .list_memories(50, 0)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let count = memories.len();
        let body = if memories.is_empty() {
            "No memories stored yet. Use memory_store to add some.".to_string()
        } else {
            let lines: Vec<String> = memories
                .iter()
                .map(|m| {
                    let tags = if m.tags.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", m.tags.join(", "))
                    };
                    format!("• {} — {}{}", m.id, m.title, tags)
                })
                .collect();
            format!(
                "Mynd Memory List ({count} memories):\n\n{}",
                lines.join("\n")
            )
        };
        Ok(vec![PromptMessage::new_text(Role::User, body)])
    }

    async fn do_memory_status_prompt(&self) -> Result<Vec<PromptMessage>, ErrorData> {
        let count = self
            .store
            .count()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let recent = self
            .store
            .list_memories(5, 0)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let recent_lines: Vec<String> = recent
            .iter()
            .map(|m| format!("  \u{2022} {} \u{2014} {}", m.id, m.title))
            .collect();

        let mut parts = vec![
            "Mynd Status".to_string(),
            "\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}".to_string(),
            format!("Total memories: {count}"),
        ];
        if !recent_lines.is_empty() {
            parts.push("\nRecent memories:".to_string());
            parts.extend(recent_lines);
        }
        parts.push("\nTip: Use /memory-list to browse all memories, or /memory-search <query> to find specific ones.".to_string());

        Ok(vec![PromptMessage::new_text(Role::User, parts.join("\n"))])
    }

    async fn do_memory_search_prompt(
        &self,
        p: MemorySearchPromptInput,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        let trimmed = p.query.trim().to_string();
        let hits = if trimmed.is_empty() {
            vec![]
        } else {
            self.store
                .search(&trimmed, 10)
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
        };
        let body = if hits.is_empty() {
            format!(
                "No memories found matching \"{}\".\n\nTip: Try broader keywords or use /memory-list to browse all memories.",
                p.query
            )
        } else {
            let lines: Vec<String> = hits
                .iter()
                .map(|h| {
                    let snippet: String = h.content.chars().take(200).collect();
                    format!("\u{2022} {} \u{2014} {}\n  {}", h.id, h.title, snippet)
                })
                .collect();
            format!(
                "Search results for \"{}\" ({} found):\n\n{}\n\nUse memory_recall with an ID for full content.",
                p.query,
                hits.len(),
                lines.join("\n\n")
            )
        };
        Ok(vec![PromptMessage::new_text(Role::User, body)])
    }

    async fn do_memory_edit_prompt(
        &self,
        p: MemoryIdInput,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        let mem = self
            .store
            .recall_by_id(&p.id)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
            .ok_or_else(|| ErrorData::invalid_params(format!("Memory {} not found", p.id), None))?;
        let tags = mem.tags.join(", ");
        let body = format!(
            "Memory to edit:\n\
             ━━━━━━━━━━━━━━\n\
             ID:      {}\n\
             Title:   {}\n\
             Tags:    {}\n\
             Content:\n{}\n\
             ━━━━━━━━━━━━━━\n\n\
             Ask the user what changes they want to make, then call memory_update with ID {} to save.\n\
             You can update content and/or tags. Omit fields you are not changing.",
            mem.id, mem.title, tags, mem.content, mem.id
        );
        Ok(vec![PromptMessage::new_text(Role::User, body)])
    }

    async fn do_memory_flag_prompt(
        &self,
        p: MemoryFlagInput,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        let mem = self
            .store
            .recall_by_id(&p.id)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
            .ok_or_else(|| ErrorData::invalid_params(format!("Memory {} not found", p.id), None))?;
        self.store
            .create_feedback(&p.id, &p.reason, p.note.as_deref())
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let body = format!(
            "Flagged memory \"{}\" ({}) as \"{}\".\n\
             A feedback record has been created and will appear in the dashboard under Feedback.\n\
             The memory has not been deleted — it remains available until a human reviews the flag.\n\
             {}",
            mem.title,
            mem.id,
            p.reason,
            p.note
                .as_ref()
                .map(|n| format!("Note: {n}"))
                .unwrap_or_default()
        );
        Ok(vec![PromptMessage::new_text(Role::User, body)])
    }

    async fn do_suggest_connections_prompt(&self) -> Result<Vec<PromptMessage>, ErrorData> {
        let body = build_suggest_prompt(&self.store)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(vec![PromptMessage::new_text(Role::User, body)])
    }

    async fn do_review_feedback_prompt(&self) -> Result<Vec<PromptMessage>, ErrorData> {
        let open_items = self
            .store
            .list_feedback(None, Some("pending"))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        if open_items.is_empty() {
            return Ok(vec![PromptMessage::new_text(
                Role::User,
                "No open feedback items. All flagged memories have been reviewed.\n\
                 Tip: Use /memory-flag <id> to flag a memory that needs attention."
                    .to_string(),
            )]);
        }

        let mut lines = vec![
            format!("Open Feedback Items ({} total)", open_items.len()),
            "\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}".to_string(),
        ];

        for (i, item) in open_items.iter().enumerate() {
            let note = item.note.as_deref().unwrap_or("(no note)");
            lines.push(format!(
                "\n{}. [{}] Memory: {} | {}\n   Note: {}",
                i + 1,
                item.signal,
                item.memory_id,
                item.id,
                note
            ));
        }

        lines.push("\n\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}".to_string());
        lines.push("For each item, choose an action:".to_string());
        lines.push(
            "  \u{2022} Fix the memory \u{2014} call memory_update with the memory ID".to_string(),
        );
        lines.push(
            "  \u{2022} Delete if truly wrong \u{2014} call memory_delete with confirm:true"
                .to_string(),
        );
        lines.push("\nAsk the user how to handle each item before taking action.".to_string());

        Ok(vec![PromptMessage::new_text(Role::User, lines.join("\n"))])
    }

    pub async fn do_session_start(
        &self,
        p: SessionStartInput,
    ) -> Result<CallToolResult, ErrorData> {
        let canon = std::fs::canonicalize(&p.project_path).map_err(|_| {
            ErrorData::invalid_params(
                format!("project_path does not exist: {}", p.project_path),
                None,
            )
        })?;
        if !canon.is_dir() {
            return Err(ErrorData::invalid_params(
                "project_path is not a directory",
                None,
            ));
        }

        let config = crate::config::load_config(&canon)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;

        let result = crate::session::execute_session_start(&config, &self.store)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        if let Err(e) = self
            .store
            .log_session_start(&canon.to_string_lossy(), &result)
            .await
        {
            tracing::warn!("failed to write session_start_log entry: {e:#}");
        }

        Ok(CallToolResult::structured(result.to_json()))
    }
}

#[tool_router]
impl Mynd {
    #[tool(
        description = "Store a memory, preference, or project context for future recall across sessions. Use when the user explicitly asks to remember something, or when important context should persist beyond this session. Call tag_namespaces_list first to pick tags that match the project's existing namespaces/values rather than inventing new ones."
    )]
    async fn memory_store(
        &self,
        Parameters(p): Parameters<MemoryStoreInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.do_memory_store(p).await
    }

    #[tool(
        description = "List the tag namespace registry: color, allowed values, single_value flag, and a human-written description per namespace (e.g. what \"topic\" vs \"status\" means for this project). Call before choosing tags for memory_store or memory_update — reuse an existing namespace/value when one fits; only fall back to a new bare (non-namespaced) tag for a genuine one-off."
    )]
    async fn tag_namespaces_list(
        &self,
        Parameters(p): Parameters<TagNamespacesListInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.do_tag_namespaces_list(p).await
    }

    #[tool(
        description = "Recall a memory by exact title or ID. Returns full content. Use memory_search to find candidates first."
    )]
    async fn memory_recall(
        &self,
        Parameters(p): Parameters<MemoryRecallInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.do_memory_recall(p).await
    }

    #[tool(
        description = "Search stored memories by keyword (FTS). Returns ranked snippets to conserve context — use memory_recall with an id for full content. Default 5 results, max 10."
    )]
    async fn memory_search(
        &self,
        Parameters(p): Parameters<MemorySearchInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.do_memory_search(p).await
    }

    #[tool(
        description = "Update an existing memory's content or tags by id. Providing tags replaces all tags on the memory — call tag_namespaces_list first if you're changing tags, to stay consistent with the project's registered namespaces."
    )]
    async fn memory_update(
        &self,
        Parameters(p): Parameters<MemoryUpdateInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.do_memory_update(p).await
    }

    #[tool(
        description = "Permanently delete a memory by id. Requires confirm=true; always confirm with the user before calling."
    )]
    async fn memory_delete(
        &self,
        Parameters(p): Parameters<MemoryDeleteInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.do_memory_delete(p).await
    }

    #[tool(
        description = "Call this once at the start of every session when .mynd.toml exists in the project root. Returns pre-configured memory context for this project."
    )]
    async fn mynd_session_start(
        &self,
        Parameters(p): Parameters<SessionStartInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.do_session_start(p).await
    }

    #[tool(
        description = "Store a confirmed connection between two memories. Use after the user or Claude explicitly decides two memories are related. Valid relationships: parent (target is a broader principle/context source falls under), child (target is a specific instance of source), sibling (a peer, no hierarchy)."
    )]
    async fn memory_store_edge(
        &self,
        Parameters(p): Parameters<MemoryStoreEdgeInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.do_memory_store_edge(p).await
    }

    pub async fn do_memory_store_edge(
        &self,
        p: MemoryStoreEdgeInput,
    ) -> Result<CallToolResult, ErrorData> {
        use crate::model::EdgeCreate;
        let status = p.status.as_deref().unwrap_or("active");
        if !["active", "pending"].contains(&status) {
            return Err(ErrorData::invalid_params(
                "status must be \"active\" or \"pending\"",
                None,
            ));
        }
        match self
            .store
            .create_edge_with_status(
                &p.source_id,
                &p.target_id,
                &p.relationship,
                status,
                None,
                p.reason.as_deref(),
            )
            .await
        {
            Ok(EdgeCreate::Created(id)) => {
                self.notify_change();
                Ok(CallToolResult::structured(json!({
                    "created": true, "id": id,
                    "source_id": p.source_id, "target_id": p.target_id, "relationship": p.relationship,
                    "status": status,
                })))
            }
            Ok(EdgeCreate::Duplicate) => Ok(CallToolResult::structured(json!({
                "created": false, "reason": "an edge between these memories with this relationship already exists",
            }))),
            Ok(EdgeCreate::MissingEndpoint) => Err(ErrorData::invalid_params(
                "source_id and target_id must both be existing, distinct memory IDs",
                None,
            )),
            Ok(EdgeCreate::InvalidRelationship) => Err(ErrorData::invalid_params(
                format!(
                    "relationship must be one of: {}",
                    crate::store::VALID_RELATIONSHIPS.join(", ")
                ),
                None,
            )),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    pub async fn do_memory_update_edge(
        &self,
        p: MemoryUpdateEdgeInput,
    ) -> Result<CallToolResult, ErrorData> {
        match self
            .store
            .update_edge(
                &p.id,
                p.relationship.as_deref(),
                p.reason.as_deref(),
                p.link_text.as_deref(),
            )
            .await
        {
            Ok(true) => {
                self.notify_change();
                Ok(CallToolResult::structured(
                    json!({ "updated": true, "id": p.id }),
                ))
            }
            Ok(false) => Ok(CallToolResult::structured(
                json!({ "updated": false, "id": p.id }),
            )),
            Err(e) => Err(ErrorData::invalid_params(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Update an existing connection between memories: change its relationship, reason, or link text. Use when revising a suggested connection after user feedback. Does not change status; the user approves or rejects in the dashboard."
    )]
    async fn memory_update_edge(
        &self,
        Parameters(p): Parameters<MemoryUpdateEdgeInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.do_memory_update_edge(p).await
    }

    #[tool(
        description = "Get a memory's connections to other memories, grouped by relationship: parents (broader context this falls under), children (specific instances of this), and siblings (peers, no hierarchy). Call after memory_recall to see how a memory connects to the rest of what's stored."
    )]
    async fn memory_get_edges(
        &self,
        Parameters(p): Parameters<MemoryGetEdgesInput>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.store.get_edges_grouped(&p.memory_id).await {
            Ok(grouped) => Ok(CallToolResult::structured(
                serde_json::to_value(grouped).unwrap(),
            )),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }
}

#[prompt_router]
impl Mynd {
    /// List all memories with titles and tags
    #[prompt(
        name = "memory-list",
        description = "List all stored memories with titles and tags. Use to browse what Mynd knows before searching or editing."
    )]
    async fn memory_list_prompt(&self) -> Result<Vec<PromptMessage>, ErrorData> {
        self.do_memory_list_prompt().await
    }

    /// Show the current memory count and recent activity
    #[prompt(
        name = "memory-status",
        description = "Show total memory count and recent memories. Use at the start of a session to understand what context is available."
    )]
    async fn memory_status_prompt(&self) -> Result<Vec<PromptMessage>, ErrorData> {
        self.do_memory_status_prompt().await
    }

    /// Search memories by keyword and present results
    #[prompt(
        name = "memory-search",
        description = "Search Mynd memories by keyword. Returns matching memories with content snippets. Follow up with memory_recall for full content."
    )]
    async fn memory_search_prompt(
        &self,
        Parameters(p): Parameters<MemorySearchPromptInput>,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        self.do_memory_search_prompt(p).await
    }

    /// Fetch a memory and return a prompt for editing its content or tags
    #[prompt(
        name = "memory-edit",
        description = "Fetch a specific memory by ID and present its current content for editing. After reviewing, call memory_update to save changes."
    )]
    async fn memory_edit_prompt(
        &self,
        Parameters(p): Parameters<MemoryIdInput>,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        self.do_memory_edit_prompt(p).await
    }

    /// Flag a memory as incorrect, outdated, or duplicate
    #[prompt(
        name = "memory-flag",
        description = "Flag a memory for review. Creates a feedback record with the specified reason."
    )]
    async fn memory_flag_prompt(
        &self,
        Parameters(p): Parameters<MemoryFlagInput>,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        self.do_memory_flag_prompt(p).await
    }

    /// Analyze the memory graph and suggest new connections between related memories
    #[prompt(
        name = "suggest-connections",
        description = "Fetch all memories and existing connections, then analyze them to suggest new edges."
    )]
    async fn suggest_connections_prompt(&self) -> Result<Vec<PromptMessage>, ErrorData> {
        self.do_suggest_connections_prompt().await
    }

    /// Surface all open feedback items for interactive review and resolution
    #[prompt(
        name = "review-feedback",
        description = "Fetch open feedback items (flagged memories) and present them for interactive resolution."
    )]
    async fn review_feedback_prompt(&self) -> Result<Vec<PromptMessage>, ErrorData> {
        self.do_review_feedback_prompt().await
    }
}

pub(crate) async fn build_suggest_prompt(store: &SqliteStore) -> anyhow::Result<String> {
    let memories = store.list_memories(100, 0).await?;
    let edges = store.list_edges(None).await?;

    if memories.is_empty() {
        return Ok(
            "No memories stored yet. Add some memories first with memory_store.".to_string(),
        );
    }

    let mem_lines: Vec<String> = memories
        .iter()
        .map(|m| {
            let tags = if m.tags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", m.tags.join(", "))
            };
            let snippet: String = m.content.chars().take(80).collect();
            let ellipsis = if m.content.len() > 80 { "…" } else { "" };
            format!("{} | {} | {}{}{}", m.id, m.title, snippet, ellipsis, tags)
        })
        .collect();

    let edge_lines: Vec<String> = edges
        .iter()
        .map(|e| {
            format!(
                "{} --[{}]--> {} ({})",
                e.source_id, e.relationship, e.target_id, e.status
            )
        })
        .collect();

    let edge_section = if edge_lines.is_empty() {
        "  (none yet)".to_string()
    } else {
        edge_lines.join("\n")
    };

    Ok(format!(
        "Mynd — Suggest Connections\n\
         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
         You have {} memories and {} existing connections.\n\n\
         MEMORIES:\n\
         {}\n\n\
         EXISTING CONNECTIONS:\n\
         {}\n\n\
         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
         STEP 1 — Audit existing connections.\n\
         Check each entry in EXISTING CONNECTIONS against the current memory content above.\n\
         Flag one as stale/irrelevant if its memories no longer support the stated relationship\n\
         (content was edited away from it, the link was a coincidence, or it duplicates/contradicts\n\
         another edge). You cannot reject an edge yourself — only the user can, in the dashboard.\n\
         For each stale one, call memory_update_edge with the edge id and a reason starting with\n\
         \"STALE:\" explaining why, so it's flagged for the user to review and reject.\n\
         Some parent/child edges are structural, not organic: they exist because a large\n\
         piece of content was chunked into an index memory plus child memories, linked via\n\
         a literal [phrase](child:mem_xxx) already present in the index's content. Identify\n\
         these by the part tag shown in MEMORIES above (part:index on the parent, part:fragment\n\
         on the children) rather than guessing from content alone. Do not flag these as stale\n\
         just because a fragment's narrow content doesn't on its own seem to \"support\" the\n\
         relationship — the relationship is structural (this fragment IS part of that document),\n\
         not a semantic claim that needs its own supporting evidence.\n\n\
         STEP 2 — Suggest new connections.\n\
         Identify meaningful connections not yet captured. For each one, call memory_store_edge\n\
         with: source_id, target_id, relationship, status: \"pending\", link_text (a short phrase\n\
         naming the relationship, e.g. \"n-layer architecture\"), and a one-sentence reason.\n\
         Choose link_text carefully: if the user approves this suggestion, the dashboard embeds it\n\
         verbatim as an inline markdown link in the source memory's content, e.g.:\n\
         \x20\x20See also: [n-layer architecture](parent:mem_xxx)\n\
         so it reads naturally as the label of that link. Do not edit memory content yourself to\n\
         add this link — approval in the dashboard does that, not you.\n\
         Relationship types:\n\
         \x20\x20parent  - target is a broader principle/context source falls under\n\
         \x20\x20child   - target is a specific instance of source\n\
         \x20\x20sibling - a peer, no hierarchy\n\
         Don't re-suggest a connection already implied by chunking structure — e.g. two\n\
         part:fragment memories of the same part:index are already related by sharing that\n\
         parent; proposing a new sibling link between them just because they're topically adjacent\n\
         is redundant. Focus on connections across genuinely different memories/documents.\n\
         Suggest 3-7 connections. Focus on cross-domain insights.\n\n\
         Keep each memory's own content short and focused on what it uniquely knows; these links\n\
         are how related context gets pointed at instead of duplicated inline. A future reader\n\
         should only follow a link when the current task actually needs that related memory, not\n\
         eagerly pull in every connected memory.",
        memories.len(),
        edges.len(),
        mem_lines.join("\n"),
        edge_section,
    ))
}

#[tool_handler]
#[prompt_handler]
impl rmcp::ServerHandler for Mynd {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo::new(
            rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .build(),
        )
        .with_server_info(rmcp::model::Implementation::new(
            "mynd",
            env!("CARGO_PKG_VERSION"),
        ))
    }
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
