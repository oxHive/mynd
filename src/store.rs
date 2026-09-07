use anyhow::{Result, anyhow};
use libsql::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub token_count: Option<i64>,
    pub layer: String,
    pub memory_type: String,
}

pub struct NewMemoryRow<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub content: &'a str,
    pub tags: &'a [String],
    pub token_count: Option<i64>,
    pub layer: &'a str,
    pub memory_type: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeEntry {
    pub id: String,
    pub source_id: String,
    pub target_id: String,
    pub relationship: String,
    pub status: String,
    pub created_at: i64,
    pub link_text: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedMemory {
    pub id: String,
    pub title: String,
    pub link_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupedEdges {
    pub parents: Vec<RelatedMemory>,
    pub children: Vec<RelatedMemory>,
    pub siblings: Vec<RelatedMemory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackEntry {
    pub id: String,
    pub memory_id: String,
    pub signal: String,
    pub note: Option<String>,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictEntry {
    pub id: String,
    pub memory_id: String,
    pub remote_content: String,
    pub local_content: String,
    pub remote_updated_at: i64,
    pub local_updated_at: i64,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStartLogRow {
    pub id: String,
    pub project_name: String,
    pub project_path: String,
    pub created_at: i64,
    pub max_tokens: i64,
    pub used_tokens: i64,
    pub memories_recalled: i64,
    pub truncated: bool,
    pub loaded: Value,
    pub skipped: Value,
}

pub struct JournalRow {
    pub memory_id: String,
    pub content: String,
    pub updated_at: i64,
}

pub struct SqliteStore {
    pub(crate) conn: Connection,
}

pub const VALID_RELATIONSHIPS: &[&str] = &["parent", "child", "sibling"];

/// Default cap on a single memory's title+content token count (see
/// `count_entry_tokens`), leaving headroom under the default 2000-token
/// session-start budget so no single memory can dominate a recall alone.
/// Configurable per-instance via the `max_content_tokens` `_meta` key
/// (dashboard-editable, same pattern as `tag_namespaces`).
pub const DEFAULT_MAX_CONTENT_TOKENS: i64 = 1500;

static RELATIONSHIP_LINK_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"\[([^\]]+)\]\((?:(parent|child|sibling):)?(mem_[0-9a-f]{32})\)").unwrap()
});

/// Extract `[phrase](kind:mem_xxx)` relationship links from memory content
/// (bare `[phrase](mem_xxx)` defaults to "sibling"). Deduped by (kind, target)
/// pair — the same target under two different kinds both survive, first
/// phrase wins for an exact repeat. Links to `source_id` are dropped.
pub(crate) fn parse_relationship_links(
    source_id: &str,
    content: &str,
) -> Vec<(String, String, String)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for cap in RELATIONSHIP_LINK_RE.captures_iter(content) {
        let target = cap[3].to_string();
        if target == source_id {
            continue;
        }
        let kind = cap
            .get(2)
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "sibling".to_string());
        if !seen.insert((kind.clone(), target.clone())) {
            continue;
        }
        out.push((cap[1].to_string(), kind, target));
    }
    out
}

/// Maintain parent/child/sibling edges for `source_id` from its content's
/// `[phrase](kind:mem_xxx)` links (bare `[phrase](mem_xxx)` defaults to
/// "sibling"): upserts an active edge per surviving (kind, target) pair
/// (refreshing `link_text` and resetting `status` to 'active' on rephrase),
/// and deletes any (kind, target) pair no longer present in the content.
///
/// Deletion is scoped to `link_text IS NOT NULL` — i.e. edges this same
/// function previously created from content. Edges created out-of-band
/// (e.g. via the `memory_store_edge` MCP tool) are stored with
/// `link_text: None` and have no content link to begin with; without this
/// scope, saving a memory for any reason (even a title-only edit) would
/// wipe every such edge since none of them match a currently-parsed link.
async fn sync_relationship_edges(
    tx: &libsql::Transaction,
    source_id: &str,
    content: &str,
    now: i64,
) -> Result<()> {
    let links = parse_relationship_links(source_id, content);

    for (phrase, kind, target) in &links {
        tx.execute(
            "INSERT INTO edges (id, source_id, target_id, relationship, status, created_at, link_text)
             SELECT 'edge_' || lower(hex(randomblob(16))), ?1, ?2, ?3, 'active', ?4, ?5
             WHERE EXISTS (SELECT 1 FROM memories WHERE id = ?2)
             ON CONFLICT(source_id, target_id, relationship)
             DO UPDATE SET link_text = excluded.link_text, status = 'active'",
            params![source_id, target.as_str(), kind.as_str(), now, phrase.as_str()],
        )
        .await?;
    }

    if links.is_empty() {
        tx.execute(
            "DELETE FROM edges WHERE source_id = ?1 AND relationship IN ('parent', 'child', 'sibling') \
             AND link_text IS NOT NULL",
            params![source_id],
        )
        .await?;
    } else {
        let conditions: Vec<String> = (0..links.len())
            .map(|i| {
                format!(
                    "(relationship = ?{} AND target_id = ?{})",
                    i * 2 + 2,
                    i * 2 + 3
                )
            })
            .collect();
        let sql = format!(
            "DELETE FROM edges WHERE source_id = ?1 AND relationship IN ('parent', 'child', 'sibling') \
             AND link_text IS NOT NULL AND NOT ({})",
            conditions.join(" OR ")
        );
        let mut p: Vec<libsql::Value> = vec![libsql::Value::Text(source_id.to_string())];
        for (_, kind, target) in &links {
            p.push(libsql::Value::Text(kind.clone()));
            p.push(libsql::Value::Text(target.clone()));
        }
        tx.execute(&sql, p).await?;
    }
    Ok(())
}

/// Quote a raw user query for FTS5 MATCH: every whitespace-separated term is
/// wrapped in double quotes (FTS5 string syntax) with embedded quotes doubled,
/// so characters like / + ' - can never be parsed as FTS5 operators.
pub(crate) fn fts_quote(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Rejects tags that are empty/whitespace-only or unreasonably long.
const MAX_TAG_LEN: usize = 128;

fn validate_tag_format(tags: &[String]) -> Result<()> {
    for t in tags {
        if t.trim().is_empty() {
            return Err(anyhow!("tag must not be empty or whitespace-only"));
        }
        if t.len() > MAX_TAG_LEN {
            return Err(anyhow!(
                "tag exceeds max length of {MAX_TAG_LEN} chars: {t:?}"
            ));
        }
    }
    Ok(())
}

/// Seeded `tag_namespaces` registry, used both as the dashboard's initial
/// suggestion set and as the fallback when the stored registry is missing
/// or fails to parse. `description` is shown to human users in the
/// dashboard and to agents via the `tag_namespaces_list` MCP tool — it's
/// the answer to "what does this namespace mean, when do I use it".
pub(crate) fn default_tag_namespaces() -> Value {
    json!({
        "project": {
            "color": "#4a9eff", "values": [], "single_value": true,
            "description": "Which product/repo this memory is scoped to. At most one per memory."
        },
        "topic": {
            "color": "#5fb8b0", "values": [],
            "description": "What part of the system this concerns, or the subject discussed (e.g. sync, graph-canvas, obsidian, billing)."
        },
        "status": {
            "color": "#a875d1", "values": ["idea", "planned", "done", "deferred"],
            "description": "Lifecycle of this memory: idea (proposed, not built), planned, done (shipped), or deferred (paused)."
        },
        "lang": {
            "color": "#e0607e", "values": [],
            "description": "Programming language, for cross-project reference/pattern memories not tied to one project."
        },
        "kind": {
            "color": "#1d9e75", "values": ["pattern", "doc-mirror", "snippet", "decision"], "values_mode": "fixed",
            "description": "Nature of cross-project knowledge content: pattern (reusable architecture/code pattern), doc-mirror (external docs saved for local/LLM consumption), snippet (reusable code), decision (an architectural decision record)."
        },
        "scope": {
            "color": "#ba7517", "values": ["team", "individual"], "values_mode": "fixed",
            "description": "Who this memory is meant for: team (shared knowledge, relevant if sync is configured to a shared remote) or individual (personal only)."
        },
        "part": {
            "color": "#f0883e", "values": ["index", "fragment"], "values_mode": "fixed", "single_value": true,
            "description": "Marks a memory's role when content was too large for one memory and got split: index (the short entry point whose content links to each fragment via [phrase](child:mem_xxx)) or fragment (one piece of the split content). Absent on ordinary, unsplit memories."
        },
    })
}

/// Shallow object merge, keyed by top-level namespace name: every key from
/// `stored` wins outright (customized or merely re-saved as-is); any key
/// present only in `defaults` (added to code after `stored` was last saved)
/// gets filled in from `defaults`. Not a deep/field-level merge — a
/// namespace present in `stored` is used wholesale, not merged field by
/// field with its code-default counterpart, since a partial per-field merge
/// would make it unclear which value "wins" for a namespace the user has
/// deliberately customized.
fn merge_tag_namespaces(defaults: Value, stored: Value) -> Value {
    let mut merged = match stored {
        Value::Object(map) => map,
        _ => return defaults,
    };
    if let Value::Object(default_map) = defaults {
        for (name, entry) in default_map {
            merged.entry(name).or_insert(entry);
        }
    }
    Value::Object(merged)
}

/// Validates `tags` against the `tag_namespaces` registry: at most one tag
/// per namespace marked `single_value` (falls back to `["project"]` alone
/// when the registry has no singleton namespaces at all), and — for any
/// namespace marked `values_mode: "fixed"` with a non-empty `values` list —
/// every tag in that namespace must use one of the registered values.
/// A namespace with `values_mode: "fixed"` but an empty `values` list is
/// not yet restrictive (nothing has been registered to check against).
async fn validate_tags_against_registry(store: &SqliteStore, tags: &[String]) -> Result<()> {
    let registry = store.tag_namespace_registry().await;
    let Some(obj) = registry.as_object() else {
        return validate_project_singleton_fallback(tags);
    };

    let singletons: Vec<&String> = obj
        .iter()
        .filter(|(_, v)| v.get("single_value").and_then(|b| b.as_bool()) == Some(true))
        .map(|(k, _)| k)
        .collect();
    if singletons.is_empty() {
        validate_project_singleton_fallback(tags)?;
    } else {
        for ns in singletons {
            let prefix = format!("{}:", ns.to_lowercase());
            let count = tags
                .iter()
                .filter(|t| t.to_lowercase().starts_with(&prefix))
                .count();
            if count > 1 {
                return Err(anyhow!("a memory can have at most one {ns}:* tag"));
            }
        }
    }

    for tag in tags {
        let Some(idx) = tag.find(':') else { continue };
        let (ns, value) = (&tag[..idx], &tag[idx + 1..]);
        let Some(entry) = obj.get(ns) else { continue };
        if entry.get("values_mode").and_then(|v| v.as_str()) != Some("fixed") {
            continue;
        }
        let Some(values) = entry.get("values").and_then(|v| v.as_array()) else {
            continue;
        };
        if values.is_empty() {
            continue;
        }
        let allowed = values
            .iter()
            .filter_map(|v| v.as_str())
            .any(|v| v.eq_ignore_ascii_case(value));
        if !allowed {
            let list: Vec<&str> = values.iter().filter_map(|v| v.as_str()).collect();
            return Err(anyhow!(
                "{ns}:{value} is not a registered value — {ns} only allows: {}",
                list.join(", ")
            ));
        }
    }
    Ok(())
}

fn validate_project_singleton_fallback(tags: &[String]) -> Result<()> {
    let count = tags
        .iter()
        .filter(|t| t.to_lowercase().starts_with("project:"))
        .count();
    if count > 1 {
        return Err(anyhow!("a memory can have at most one project:* tag"));
    }
    Ok(())
}

impl SqliteStore {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }

    pub async fn store(&self, m: &NewMemoryRow<'_>) -> Result<()> {
        validate_tag_format(m.tags)?;
        validate_tags_against_registry(self, m.tags).await?;
        let now = chrono_now();
        let token_count = m
            .token_count
            .unwrap_or_else(|| crate::budget::count_entry_tokens(m.title, m.content) as i64);

        let tx = self.conn.transaction().await?;
        tx.execute(
            "INSERT INTO memories (id, title, content, created_at, updated_at, token_count, layer, memory_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
               title = excluded.title,
               content = excluded.content,
               updated_at = excluded.updated_at,
               token_count = excluded.token_count,
               layer = excluded.layer,
               memory_type = excluded.memory_type",
            params![m.id, m.title, m.content, now, now, token_count, m.layer, m.memory_type],
        )
        .await?;

        tx.execute(
            "DELETE FROM memory_tags WHERE memory_id = ?1",
            params![m.id],
        )
        .await?;
        for tag in m.tags {
            let tag_lower = tag.to_lowercase();
            tx.execute(
                "INSERT OR IGNORE INTO memory_tags (memory_id, tag) VALUES (?1, ?2)",
                params![m.id, tag_lower.as_str()],
            )
            .await?;
        }

        sync_relationship_edges(&tx, m.id, m.content, now).await?;

        // Journal the write
        tx.execute(
            "INSERT INTO sync_journal (memory_id, content, updated_at, recorded_at)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(memory_id) DO UPDATE SET
               content = excluded.content, updated_at = excluded.updated_at, recorded_at = excluded.recorded_at",
            params![m.id, m.content, now],
        )
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn recall_by_id(&self, id: &str) -> Result<Option<MemoryEntry>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, title, content, created_at, updated_at, token_count, layer, memory_type FROM memories WHERE id = ?1",
                params![id],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            let entry = self.row_to_entry(&row)?;
            let tags = self.fetch_tags(&entry.id).await?;
            Ok(Some(MemoryEntry { tags, ..entry }))
        } else {
            Ok(None)
        }
    }

    pub async fn recall_by_title(&self, title: &str) -> Result<Option<MemoryEntry>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, title, content, created_at, updated_at, token_count, layer, memory_type FROM memories WHERE title = ?1",
                params![title],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            let entry = self.row_to_entry(&row)?;
            let tags = self.fetch_tags(&entry.id).await?;
            Ok(Some(MemoryEntry { tags, ..entry }))
        } else {
            Ok(None)
        }
    }

    pub async fn search(&self, query: &str, limit: i64) -> Result<Vec<MemoryEntry>> {
        let quoted = fts_quote(query);
        if quoted.is_empty() {
            return Ok(Vec::new());
        }
        let mut rows = self
            .conn
            .query(
                "SELECT m.id, m.title, m.content, m.created_at, m.updated_at, m.token_count, m.layer, m.memory_type
                 FROM memories m
                 JOIN memories_fts f ON m.rowid = f.rowid
                 WHERE memories_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
                params![quoted, limit],
            )
            .await?;
        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            let entry = self.row_to_entry(&row)?;
            let tags = self.fetch_tags(&entry.id).await?;
            results.push(MemoryEntry { tags, ..entry });
        }
        Ok(results)
    }

    pub async fn update(
        &self,
        id: &str,
        title: &str,
        content: &str,
        tags: &[String],
    ) -> Result<bool> {
        validate_tag_format(tags)?;
        validate_tags_against_registry(self, tags).await?;
        let now = chrono_now();
        let token_count = crate::budget::count_entry_tokens(title, content) as i64;
        let tx = self.conn.transaction().await?;
        let changed = tx
            .execute(
                "UPDATE memories SET title = ?1, content = ?2, updated_at = ?3, token_count = ?4 WHERE id = ?5",
                params![title, content, now, token_count, id],
            )
            .await?;
        if changed == 0 {
            return Ok(false);
        }
        tx.execute("DELETE FROM memory_tags WHERE memory_id = ?1", params![id])
            .await?;
        for tag in tags {
            let tag_lower = tag.to_lowercase();
            tx.execute(
                "INSERT OR IGNORE INTO memory_tags (memory_id, tag) VALUES (?1, ?2)",
                params![id, tag_lower.as_str()],
            )
            .await?;
        }

        sync_relationship_edges(&tx, id, content, now).await?;

        // Journal the write
        tx.execute(
            "INSERT INTO sync_journal (memory_id, content, updated_at, recorded_at)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(memory_id) DO UPDATE SET
               content = excluded.content, updated_at = excluded.updated_at, recorded_at = excluded.recorded_at",
            params![id, content, now],
        )
        .await?;

        tx.commit().await?;
        Ok(true)
    }

    /// Merges `tags` into the memory's existing tag set (dedup via
    /// `INSERT OR IGNORE`, same as `store`). Returns `false` if the memory
    /// doesn't exist. Runs the same format/cardinality validation as
    /// `store`/`update`, checked against the *merged* set.
    pub async fn add_tags(&self, id: &str, tags: &[String]) -> Result<bool> {
        let Some(current) = self.recall_by_id(id).await? else {
            return Ok(false);
        };
        let mut merged = current.tags.clone();
        for t in tags {
            let lower = t.to_lowercase();
            if !merged.contains(&lower) {
                merged.push(lower);
            }
        }
        validate_tag_format(&merged)?;
        validate_tags_against_registry(self, &merged).await?;

        let now = chrono_now();
        let tx = self.conn.transaction().await?;
        for t in tags {
            tx.execute(
                "INSERT OR IGNORE INTO memory_tags (memory_id, tag) VALUES (?1, ?2)",
                params![id, t.to_lowercase()],
            )
            .await?;
        }
        tx.execute(
            "UPDATE memories SET updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )
        .await?;
        tx.execute(
            "INSERT INTO sync_journal (memory_id, content, updated_at, recorded_at)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(memory_id) DO UPDATE SET
               content = excluded.content, updated_at = excluded.updated_at, recorded_at = excluded.recorded_at",
            params![id, current.content, now],
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Drops `tags` from the memory's existing tag set, leaving the rest
    /// untouched. Returns `false` if the memory doesn't exist.
    pub async fn remove_tags(&self, id: &str, tags: &[String]) -> Result<bool> {
        let Some(current) = self.recall_by_id(id).await? else {
            return Ok(false);
        };
        let now = chrono_now();
        let tx = self.conn.transaction().await?;
        for t in tags {
            tx.execute(
                "DELETE FROM memory_tags WHERE memory_id = ?1 AND tag = ?2",
                params![id, t.to_lowercase()],
            )
            .await?;
        }
        tx.execute(
            "UPDATE memories SET updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )
        .await?;
        tx.execute(
            "INSERT INTO sync_journal (memory_id, content, updated_at, recorded_at)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(memory_id) DO UPDATE SET
               content = excluded.content, updated_at = excluded.updated_at, recorded_at = excluded.recorded_at",
            params![id, current.content, now],
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let changed = self
            .conn
            .execute("DELETE FROM memories WHERE id = ?1", params![id])
            .await?;
        Ok(changed > 0)
    }

    pub async fn resolve_recall(&self, query: &str) -> Result<Vec<MemoryEntry>> {
        // Try exact title match first
        if let Some(entry) = self.recall_by_title(query).await? {
            return Ok(vec![entry]);
        }
        // Fall back to FTS search
        let results = self.search(query, 5).await?;
        Ok(results)
    }

    pub async fn delete_all(&self) -> Result<i64> {
        let changed = self.conn.execute("DELETE FROM memories", ()).await?;
        Ok(changed as i64)
    }

    pub async fn count(&self) -> Result<i64> {
        let mut rows = self.conn.query("SELECT COUNT(*) FROM memories", ()).await?;
        let row = rows.next().await?.ok_or_else(|| anyhow!("no count row"))?;
        Ok(row.get(0)?)
    }

    pub async fn log_session_start(
        &self,
        project_path: &str,
        result: &crate::session::SessionStartResult,
    ) -> Result<()> {
        let id = format!("ssl_{}", uuid::Uuid::new_v4().simple());
        let now = chrono_now();
        let loaded_json = json!(
            result
                .loaded
                .iter()
                .map(|l| json!({
                    "id": l.entry.id,
                    "title": l.entry.title,
                    "tags": l.entry.tags,
                    "layer": l.entry.layer,
                    "tokens": l.tokens,
                }))
                .collect::<Vec<_>>()
        )
        .to_string();
        let skipped_json = json!(
            result
                .skipped
                .iter()
                .map(|s| json!({ "query": s.query, "reason": s.reason.as_str() }))
                .collect::<Vec<_>>()
        )
        .to_string();
        // Match `SessionStartResult::truncated()` semantics so the analytics
        // page reports the same truncation status the MCP tool already
        // returned for this identical session-start run.
        let truncated = result.truncated();

        self.conn
            .execute(
                "INSERT INTO session_start_log
                    (id, project_name, project_path, created_at, max_tokens, used_tokens,
                     memories_recalled, truncated, loaded_json, skipped_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    id,
                    result.project.clone(),
                    project_path,
                    now,
                    result.max_tokens as i64,
                    result.used_tokens as i64,
                    result.memories_recalled as i64,
                    truncated as i64,
                    loaded_json,
                    skipped_json,
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn list_session_logs(&self, limit: i64) -> Result<Vec<SessionStartLogRow>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, project_name, project_path, created_at, max_tokens, used_tokens,
                        memories_recalled, truncated, loaded_json, skipped_json
                 FROM session_start_log ORDER BY created_at DESC LIMIT ?1",
                params![limit],
            )
            .await?;
        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            let loaded_json: String = row.get(8)?;
            let skipped_json: String = row.get(9)?;
            results.push(SessionStartLogRow {
                id: row.get(0)?,
                project_name: row.get(1)?,
                project_path: row.get(2)?,
                created_at: row.get(3)?,
                max_tokens: row.get(4)?,
                used_tokens: row.get(5)?,
                memories_recalled: row.get(6)?,
                truncated: row.get::<i64>(7)? != 0,
                loaded: serde_json::from_str(&loaded_json)?,
                skipped: serde_json::from_str(&skipped_json)?,
            });
        }
        Ok(results)
    }

    pub async fn list_memories(&self, limit: i64, offset: i64) -> Result<Vec<MemoryEntry>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, title, content, created_at, updated_at, token_count, layer, memory_type
                 FROM memories ORDER BY updated_at DESC LIMIT ?1 OFFSET ?2",
                params![limit, offset],
            )
            .await?;
        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            let entry = self.row_to_entry(&row)?;
            let tags = self.fetch_tags(&entry.id).await?;
            results.push(MemoryEntry { tags, ..entry });
        }
        Ok(results)
    }

    /// Evaluates a tag boolean expression against every stored memory. Reuses
    /// `list_memories`'s per-row tag fetch (same N+1 pattern already used by
    /// `list_memories`/`search`) rather than a bulk-query optimization — fine
    /// at this tool's realistic memory counts (see `export()`'s identical
    /// `list_memories(100_000, 0)` full-table convention).
    pub async fn find_by_tag_expr(
        &self,
        expr: &crate::tag_query::TagExpr,
    ) -> Result<Vec<MemoryEntry>> {
        let all = self.list_memories(100_000, 0).await?;
        Ok(all.into_iter().filter(|e| expr.eval(&e.tags)).collect())
    }

    pub async fn list_edges(&self, memory_id: Option<&str>) -> Result<Vec<EdgeEntry>> {
        let mut rows = if let Some(mid) = memory_id {
            self.conn
                .query(
                    "SELECT id, source_id, target_id, relationship, status, created_at, link_text, reason
                     FROM edges WHERE source_id = ?1 OR target_id = ?1 ORDER BY created_at DESC",
                    params![mid],
                )
                .await?
        } else {
            self.conn
                .query(
                    "SELECT id, source_id, target_id, relationship, status, created_at, link_text, reason
                     FROM edges ORDER BY created_at DESC",
                    (),
                )
                .await?
        };
        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(EdgeEntry {
                id: row.get(0)?,
                source_id: row.get(1)?,
                target_id: row.get(2)?,
                relationship: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get(5)?,
                link_text: row.get(6)?,
                reason: row.get(7)?,
            });
        }
        Ok(results)
    }

    pub async fn get_edges_grouped(&self, memory_id: &str) -> Result<GroupedEdges> {
        async fn fetch(
            conn: &Connection,
            sql: &str,
            memory_id: &str,
        ) -> Result<Vec<RelatedMemory>> {
            let mut rows = conn.query(sql, params![memory_id]).await?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await? {
                out.push(RelatedMemory {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    link_text: row.get(2)?,
                });
            }
            Ok(out)
        }

        let parents = fetch(
            &self.conn,
            "SELECT id, title, MIN(link_text) AS link_text FROM ( \
                SELECT m.id, m.title, e.link_text FROM edges e JOIN memories m ON m.id = e.target_id \
                 WHERE e.source_id = ?1 AND e.relationship = 'parent' \
                 UNION ALL \
                SELECT m.id, m.title, e.link_text FROM edges e JOIN memories m ON m.id = e.source_id \
                 WHERE e.target_id = ?1 AND e.relationship = 'child' \
             ) GROUP BY id, title",
            memory_id,
        )
        .await?;

        let children = fetch(
            &self.conn,
            "SELECT id, title, MIN(link_text) AS link_text FROM ( \
                SELECT m.id, m.title, e.link_text FROM edges e JOIN memories m ON m.id = e.target_id \
                 WHERE e.source_id = ?1 AND e.relationship = 'child' \
                 UNION ALL \
                SELECT m.id, m.title, e.link_text FROM edges e JOIN memories m ON m.id = e.source_id \
                 WHERE e.target_id = ?1 AND e.relationship = 'parent' \
             ) GROUP BY id, title",
            memory_id,
        )
        .await?;

        let siblings = fetch(
            &self.conn,
            "SELECT id, title, MIN(link_text) AS link_text FROM ( \
                SELECT m.id, m.title, e.link_text FROM edges e JOIN memories m ON m.id = e.target_id \
                 WHERE e.source_id = ?1 AND e.relationship = 'sibling' \
                 UNION ALL \
                SELECT m.id, m.title, e.link_text FROM edges e JOIN memories m ON m.id = e.source_id \
                 WHERE e.target_id = ?1 AND e.relationship = 'sibling' \
             ) GROUP BY id, title",
            memory_id,
        )
        .await?;

        Ok(GroupedEdges {
            parents,
            children,
            siblings,
        })
    }

    pub async fn create_edge(
        &self,
        source_id: &str,
        target_id: &str,
        relationship: &str,
    ) -> Result<crate::model::EdgeCreate> {
        self.create_edge_with_status(source_id, target_id, relationship, "active", None, None)
            .await
    }

    /// Like `create_edge`, but lets the caller set `status`/`link_text`
    /// directly instead of always defaulting to a freshly-accepted edge —
    /// used by import so a restored backup preserves pending/rejected edges
    /// and mention anchor text instead of silently promoting everything to
    /// 'active' and dropping link_text.
    pub async fn create_edge_with_status(
        &self,
        source_id: &str,
        target_id: &str,
        relationship: &str,
        status: &str,
        link_text: Option<&str>,
        reason: Option<&str>,
    ) -> Result<crate::model::EdgeCreate> {
        use crate::model::EdgeCreate;
        if !VALID_RELATIONSHIPS.contains(&relationship) || source_id == target_id {
            return Ok(EdgeCreate::InvalidRelationship);
        }
        let endpoints: i64 = {
            let mut rows = self
                .conn
                .query(
                    "SELECT COUNT(*) FROM memories WHERE id IN (?1, ?2)",
                    params![source_id, target_id],
                )
                .await?;
            rows.next().await?.unwrap().get(0)?
        };
        if endpoints != 2 {
            return Ok(EdgeCreate::MissingEndpoint);
        }
        let dup: i64 = {
            let mut rows = self
                .conn
                .query(
                    "SELECT COUNT(*) FROM edges WHERE relationship = ?3
                     AND ((source_id = ?1 AND target_id = ?2) OR (source_id = ?2 AND target_id = ?1))",
                    params![source_id, target_id, relationship],
                )
                .await?;
            rows.next().await?.unwrap().get(0)?
        };
        if dup > 0 {
            return Ok(EdgeCreate::Duplicate);
        }
        let id = format!("edge_{}", uuid::Uuid::new_v4().simple());
        self.conn
            .execute(
                "INSERT INTO edges (id, source_id, target_id, relationship, status, created_at, link_text, reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id.as_str(),
                    source_id,
                    target_id,
                    relationship,
                    status,
                    chrono_now(),
                    link_text,
                    reason
                ],
            )
            .await?;
        Ok(EdgeCreate::Created(id))
    }

    pub async fn set_edge_status(&self, id: &str, status: &str) -> Result<bool> {
        let changed = self
            .conn
            .execute(
                "UPDATE edges SET status = ?1 WHERE id = ?2",
                params![status, id],
            )
            .await?;
        Ok(changed > 0)
    }

    pub async fn get_edge(&self, id: &str) -> Result<Option<EdgeEntry>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, source_id, target_id, relationship, status, created_at, link_text, reason
                 FROM edges WHERE id = ?1",
                params![id],
            )
            .await?;
        match rows.next().await? {
            None => Ok(None),
            Some(row) => Ok(Some(EdgeEntry {
                id: row.get(0)?,
                source_id: row.get(1)?,
                target_id: row.get(2)?,
                relationship: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get(5)?,
                link_text: row.get(6)?,
                reason: row.get(7)?,
            })),
        }
    }

    /// Patch a subset of edge fields. `None` leaves a field unchanged.
    /// Returns false when no edge has this id.
    pub async fn update_edge(
        &self,
        id: &str,
        relationship: Option<&str>,
        reason: Option<&str>,
        link_text: Option<&str>,
    ) -> Result<bool> {
        if let Some(r) = relationship
            && !VALID_RELATIONSHIPS.contains(&r)
        {
            anyhow::bail!(
                "invalid relationship; valid: {}",
                VALID_RELATIONSHIPS.join(", ")
            );
        }
        let changed = self
            .conn
            .execute(
                "UPDATE edges SET
                     relationship = COALESCE(?1, relationship),
                     reason = COALESCE(?2, reason),
                     link_text = COALESCE(?3, link_text)
                 WHERE id = ?4",
                params![relationship, reason, link_text, id],
            )
            .await?;
        Ok(changed > 0)
    }

    pub async fn create_feedback(
        &self,
        memory_id: &str,
        signal: &str,
        note: Option<&str>,
    ) -> Result<FeedbackEntry> {
        let id = format!("fb_{}", uuid::Uuid::new_v4().simple());
        let now = chrono_now();
        self.conn
            .execute(
                "INSERT INTO feedback (id, memory_id, signal, note, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'pending', ?5)",
                params![id.as_str(), memory_id, signal, note, now],
            )
            .await?;
        Ok(FeedbackEntry {
            id,
            memory_id: memory_id.to_string(),
            signal: signal.to_string(),
            note: note.map(|s| s.to_string()),
            status: "pending".to_string(),
            created_at: now,
        })
    }

    pub async fn list_feedback(
        &self,
        memory_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<FeedbackEntry>> {
        let mut rows = match (memory_id, status) {
            (Some(mid), Some(status)) => {
                self.conn
                    .query(
                        "SELECT id, memory_id, signal, note, status, created_at
                         FROM feedback WHERE memory_id = ?1 AND status = ?2 ORDER BY created_at DESC",
                        params![mid, status],
                    )
                    .await?
            }
            (Some(mid), None) => {
                self.conn
                    .query(
                        "SELECT id, memory_id, signal, note, status, created_at
                         FROM feedback WHERE memory_id = ?1 ORDER BY created_at DESC",
                        params![mid],
                    )
                    .await?
            }
            (None, Some(status)) => {
                self.conn
                    .query(
                        "SELECT id, memory_id, signal, note, status, created_at
                         FROM feedback WHERE status = ?1 ORDER BY created_at DESC",
                        params![status],
                    )
                    .await?
            }
            (None, None) => {
                self.conn
                    .query(
                        "SELECT id, memory_id, signal, note, status, created_at
                         FROM feedback ORDER BY created_at DESC",
                        (),
                    )
                    .await?
            }
        };
        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(FeedbackEntry {
                id: row.get(0)?,
                memory_id: row.get(1)?,
                signal: row.get(2)?,
                note: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get(5)?,
            });
        }
        Ok(results)
    }

    pub async fn set_feedback_status(&self, id: &str, status: &str) -> Result<bool> {
        let changed = self
            .conn
            .execute(
                "UPDATE feedback SET status = ?1 WHERE id = ?2",
                params![status, id],
            )
            .await?;
        Ok(changed > 0)
    }

    pub async fn resolve_conflict(&self, id: &str, resolution: &str) -> Result<bool> {
        let Some(conflict) = self.get_conflict_by_id(id).await? else {
            return Ok(false);
        };
        if conflict.status != "pending" {
            return Ok(false);
        }
        if resolution == "keep_local"
            && let Some(mem) = self.recall_by_id(&conflict.memory_id).await?
        {
            self.update(&mem.id, &mem.title, &conflict.local_content, &mem.tags)
                .await?;
        }
        let changed = self
            .conn
            .execute(
                "UPDATE conflicts SET status = ?1 WHERE id = ?2",
                params![resolution, id],
            )
            .await?;
        Ok(changed > 0)
    }

    pub async fn take_journal(&self) -> Result<Vec<JournalRow>> {
        let mut rows = self
            .conn
            .query(
                "SELECT memory_id, content, updated_at FROM sync_journal",
                (),
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(JournalRow {
                memory_id: row.get(0)?,
                content: row.get(1)?,
                updated_at: row.get(2)?,
            });
        }
        Ok(out)
    }

    pub async fn detect_conflicts(&self, journal: &[JournalRow]) -> Result<usize> {
        let mut found = 0usize;
        for j in journal {
            let current = self.recall_by_id(&j.memory_id).await?;
            if let Some(cur) = current
                && cur.content != j.content
            {
                self.write_conflict(
                    &j.memory_id,
                    &cur.content,
                    &j.content,
                    cur.updated_at,
                    j.updated_at,
                )
                .await?;
                found += 1;
            }
            self.conn
                .execute(
                    "DELETE FROM sync_journal WHERE memory_id = ?1",
                    params![j.memory_id.as_str()],
                )
                .await?;
        }
        Ok(found)
    }

    pub async fn pending_conflict_count(&self) -> Result<i64> {
        let mut rows = self
            .conn
            .query(
                "SELECT COUNT(*) FROM conflicts WHERE status = 'pending'",
                (),
            )
            .await?;
        Ok(rows.next().await?.unwrap().get(0)?)
    }

    pub async fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO _meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .await?;
        Ok(())
    }

    pub async fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let mut rows = self
            .conn
            .query("SELECT value FROM _meta WHERE key = ?1", params![key])
            .await?;
        Ok(match rows.next().await? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    /// The `tag_namespaces` registry (colors, values, descriptions): starts
    /// from `default_tag_namespaces()` and overlays whatever's stored, key by
    /// key. Falling back to bare defaults when nothing is stored yet or the
    /// stored value fails to parse. Single source of truth for the API
    /// settings endpoint, tag-cardinality validation, and the
    /// `tag_namespaces_list` MCP tool.
    ///
    /// The overlay (not a wholesale replace) matters: a namespace added to
    /// `default_tag_namespaces()` after a user's first save (e.g. `part`,
    /// added post-1.0) must still show up for anyone whose stored blob
    /// predates it, rather than being permanently shadowed by an old
    /// snapshot. Namespaces the stored blob does mention (customized or
    /// left untouched) still win over the code default.
    pub async fn tag_namespace_registry(&self) -> Value {
        let defaults = default_tag_namespaces();
        match self.get_meta("tag_namespaces").await {
            Ok(Some(s)) => match serde_json::from_str::<Value>(&s) {
                Ok(stored) => merge_tag_namespaces(defaults, stored),
                Err(_) => defaults,
            },
            _ => defaults,
        }
    }

    /// The max-content-tokens guardrail (see `DEFAULT_MAX_CONTENT_TOKENS`),
    /// falling back to the default when nothing is stored yet or the stored
    /// value fails to parse. Single source of truth for the API settings
    /// endpoint and the `memory_store`/`memory_update` size check.
    pub async fn max_content_tokens(&self) -> i64 {
        match self.get_meta("max_content_tokens").await {
            Ok(Some(s)) => s.parse().unwrap_or(DEFAULT_MAX_CONTENT_TOKENS),
            _ => DEFAULT_MAX_CONTENT_TOKENS,
        }
    }

    pub async fn list_conflicts(&self, status: Option<&str>) -> Result<Vec<ConflictEntry>> {
        let mut rows = if let Some(status) = status {
            self.conn
                .query(
                    "SELECT id, memory_id, remote_content, local_content, remote_updated_at, local_updated_at, status, created_at
                     FROM conflicts WHERE status = ?1 ORDER BY created_at DESC",
                    params![status],
                )
                .await?
        } else {
            self.conn
                .query(
                    "SELECT id, memory_id, remote_content, local_content, remote_updated_at, local_updated_at, status, created_at
                     FROM conflicts ORDER BY created_at DESC",
                    (),
                )
                .await?
        };
        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(ConflictEntry {
                id: row.get(0)?,
                memory_id: row.get(1)?,
                remote_content: row.get(2)?,
                local_content: row.get(3)?,
                remote_updated_at: row.get(4)?,
                local_updated_at: row.get(5)?,
                status: row.get(6)?,
                created_at: row.get(7)?,
            });
        }
        Ok(results)
    }

    pub async fn get_conflict_by_id(&self, id: &str) -> Result<Option<ConflictEntry>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, memory_id, remote_content, local_content, remote_updated_at, local_updated_at, status, created_at
                 FROM conflicts WHERE id = ?1",
                params![id],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            Ok(Some(ConflictEntry {
                id: row.get(0)?,
                memory_id: row.get(1)?,
                remote_content: row.get(2)?,
                local_content: row.get(3)?,
                remote_updated_at: row.get(4)?,
                local_updated_at: row.get(5)?,
                status: row.get(6)?,
                created_at: row.get(7)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn write_conflict(
        &self,
        memory_id: &str,
        remote_content: &str,
        local_content: &str,
        remote_updated_at: i64,
        local_updated_at: i64,
    ) -> Result<ConflictEntry> {
        let id = format!("conflict_{}", uuid::Uuid::new_v4().simple());
        let now = chrono_now();
        self.conn
            .execute(
                "INSERT INTO conflicts (id, memory_id, remote_content, local_content, remote_updated_at, local_updated_at, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7)",
                params![id.as_str(), memory_id, remote_content, local_content, remote_updated_at, local_updated_at, now],
            )
            .await?;
        Ok(ConflictEntry {
            id,
            memory_id: memory_id.to_string(),
            remote_content: remote_content.to_string(),
            local_content: local_content.to_string(),
            remote_updated_at,
            local_updated_at,
            status: "pending".to_string(),
            created_at: now,
        })
    }

    async fn fetch_tags(&self, memory_id: &str) -> Result<Vec<String>> {
        let mut rows = self
            .conn
            .query(
                "SELECT tag FROM memory_tags WHERE memory_id = ?1 ORDER BY tag",
                params![memory_id],
            )
            .await?;
        let mut tags = Vec::new();
        while let Some(row) = rows.next().await? {
            tags.push(row.get(0)?);
        }
        Ok(tags)
    }

    /// SQLite bumps this counter when a DIFFERENT connection commits a write,
    /// which is exactly the cross-process signal the dashboard poller needs.
    pub async fn data_version(&self) -> Result<i64> {
        let mut rows = self.conn.query("PRAGMA data_version", ()).await?;
        let row = rows
            .next()
            .await?
            .ok_or_else(|| anyhow::anyhow!("PRAGMA data_version returned no row"))?;
        Ok(row.get(0)?)
    }

    fn row_to_entry(&self, row: &libsql::Row) -> Result<MemoryEntry> {
        Ok(MemoryEntry {
            id: row.get(0)?,
            title: row.get(1)?,
            content: row.get(2)?,
            tags: Vec::new(),
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            token_count: row.get(5)?,
            layer: row.get(6)?,
            memory_type: row.get(7)?,
        })
    }
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
