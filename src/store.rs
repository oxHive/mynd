use anyhow::{Result, anyhow};
use libsql::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::hive::roster::{JoinRecord, RevocationRecord, RosterEntry, RosterStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub source_device_id: Option<String>,
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

pub struct HiveManifest {
    pub memories: std::collections::HashMap<String, (String, i64)>,
    pub tombstones: std::collections::HashMap<String, i64>,
    pub settings: (String, i64),
    pub tag_namespaces: (String, i64),
}

pub struct PeerStatusRow {
    pub online: bool,
    pub last_synced_at: Option<i64>,
    pub pending_conflict_count: i64,
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

pub(crate) fn compute_hive_content_hash(
    title: &str,
    content: &str,
    tags: &[String],
    layer: &str,
    memory_type: &str,
) -> String {
    use sha2::{Digest, Sha256};
    let mut sorted_tags: Vec<&str> = tags.iter().map(String::as_str).collect();
    sorted_tags.sort_unstable();
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    hasher.update(b"\0");
    hasher.update(content.as_bytes());
    hasher.update(b"\0");
    hasher.update(sorted_tags.join(",").as_bytes());
    hasher.update(b"\0");
    hasher.update(layer.as_bytes());
    hasher.update(b"\0");
    hasher.update(memory_type.as_bytes());
    hex::encode(hasher.finalize())
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
        self.store_with_timestamp(m, chrono_now()).await
    }

    /// Like `store`, but stamps `updated_at` (and `created_at` for a brand-new
    /// row) with `updated_at` instead of the wall clock. Used by the hive
    /// apply path so a memory pulled from a peer keeps the *peer's* timestamp:
    /// last-write-wins only converges across the hive if every device compares
    /// the same timestamp for the same version of a memory. Stamping `now`
    /// instead made the pulled copy look newer than the original, so an edit
    /// the origin made in between was permanently judged "older" and lost.
    pub(crate) async fn store_with_timestamp(
        &self,
        m: &NewMemoryRow<'_>,
        updated_at: i64,
    ) -> Result<()> {
        validate_tag_format(m.tags)?;
        validate_tags_against_registry(self, m.tags).await?;
        let now = updated_at;
        let token_count = m
            .token_count
            .unwrap_or_else(|| crate::budget::count_entry_tokens(m.title, m.content) as i64);

        let hive_hash =
            compute_hive_content_hash(m.title, m.content, m.tags, m.layer, m.memory_type);
        let tx = self.conn.transaction().await?;
        tx.execute(
            "INSERT INTO memories (id, title, content, created_at, updated_at, token_count, layer, memory_type, hive_content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
               title = excluded.title,
               content = excluded.content,
               updated_at = excluded.updated_at,
               token_count = excluded.token_count,
               layer = excluded.layer,
               memory_type = excluded.memory_type,
               hive_content_hash = excluded.hive_content_hash",
            params![m.id, m.title, m.content, now, now, token_count, m.layer, m.memory_type, hive_hash],
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

        // A (re)written memory is alive again: drop any tombstone for its id
        // so the manifest doesn't advertise both a live row and a deletion,
        // and so `apply_incoming_memory`'s no-resurrect check doesn't keep
        // blocking newer versions of it from peers.
        tx.execute(
            "DELETE FROM hive_tombstones WHERE memory_id = ?1",
            params![m.id],
        )
        .await?;

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
        let Some(current) = self.recall_by_id(id).await? else {
            return Ok(false);
        };
        let now = chrono_now();
        let token_count = crate::budget::count_entry_tokens(title, content) as i64;
        let hive_hash =
            compute_hive_content_hash(title, content, tags, &current.layer, &current.memory_type);
        let tx = self.conn.transaction().await?;
        let changed = tx
            .execute(
                "UPDATE memories SET title = ?1, content = ?2, updated_at = ?3, token_count = ?4, hive_content_hash = ?5 WHERE id = ?6",
                params![title, content, now, token_count, hive_hash, id],
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
        let hive_hash = compute_hive_content_hash(
            &current.title,
            &current.content,
            &merged,
            &current.layer,
            &current.memory_type,
        );
        let tx = self.conn.transaction().await?;
        for t in tags {
            tx.execute(
                "INSERT OR IGNORE INTO memory_tags (memory_id, tag) VALUES (?1, ?2)",
                params![id, t.to_lowercase()],
            )
            .await?;
        }
        tx.execute(
            "UPDATE memories SET updated_at = ?2, hive_content_hash = ?3 WHERE id = ?1",
            params![id, now, hive_hash],
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
        let removed_lower: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
        let remaining: Vec<String> = current
            .tags
            .iter()
            .filter(|t| !removed_lower.contains(t))
            .cloned()
            .collect();
        let now = chrono_now();
        let hive_hash = compute_hive_content_hash(
            &current.title,
            &current.content,
            &remaining,
            &current.layer,
            &current.memory_type,
        );
        let tx = self.conn.transaction().await?;
        for t in tags {
            tx.execute(
                "DELETE FROM memory_tags WHERE memory_id = ?1 AND tag = ?2",
                params![id, t.to_lowercase()],
            )
            .await?;
        }
        tx.execute(
            "UPDATE memories SET updated_at = ?2, hive_content_hash = ?3 WHERE id = ?1",
            params![id, now, hive_hash],
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
        let now = chrono_now();
        let tx = self.conn.transaction().await?;
        let changed = tx
            .execute("DELETE FROM memories WHERE id = ?1", params![id])
            .await?;
        if changed > 0 {
            tx.execute(
                "INSERT INTO hive_tombstones (memory_id, deleted_at) VALUES (?1, ?2)
                 ON CONFLICT(memory_id) DO UPDATE SET deleted_at = excluded.deleted_at",
                params![id, now],
            )
            .await?;
        }
        tx.commit().await?;
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
        let now = chrono_now();
        let tx = self.conn.transaction().await?;
        tx.execute(
            "INSERT OR REPLACE INTO hive_tombstones (memory_id, deleted_at) SELECT id, ? FROM memories",
            params![now],
        )
        .await?;
        let changed = tx.execute("DELETE FROM memories", ()).await?;
        tx.commit().await?;
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
        if changed > 0
            && let Some(device_id) = &conflict.source_device_id
        {
            self.conn
                .execute(
                    "UPDATE hive_peer_status SET pending_conflict_count = MAX(0, pending_conflict_count - 1) WHERE device_id = ?1",
                    params![device_id.as_str()],
                )
                .await?;
        }
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
                    None,
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
                    "SELECT id, memory_id, remote_content, local_content, remote_updated_at, local_updated_at, status, created_at, source_device_id
                     FROM conflicts WHERE status = ?1 ORDER BY created_at DESC",
                    params![status],
                )
                .await?
        } else {
            self.conn
                .query(
                    "SELECT id, memory_id, remote_content, local_content, remote_updated_at, local_updated_at, status, created_at, source_device_id
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
                source_device_id: row.get(8)?,
            });
        }
        Ok(results)
    }

    pub async fn get_conflict_by_id(&self, id: &str) -> Result<Option<ConflictEntry>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, memory_id, remote_content, local_content, remote_updated_at, local_updated_at, status, created_at, source_device_id
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
                source_device_id: row.get(8)?,
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
        source_device_id: Option<&str>,
    ) -> Result<ConflictEntry> {
        let id = format!("conflict_{}", uuid::Uuid::new_v4().simple());
        let now = chrono_now();
        self.conn
            .execute(
                "INSERT INTO conflicts (id, memory_id, remote_content, local_content, remote_updated_at, local_updated_at, status, created_at, source_device_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8)",
                params![id.as_str(), memory_id, remote_content, local_content, remote_updated_at, local_updated_at, now, source_device_id],
            )
            .await?;
        if let Some(device_id) = source_device_id {
            self.increment_peer_conflict_count(device_id).await?;
        }
        Ok(ConflictEntry {
            id,
            memory_id: memory_id.to_string(),
            remote_content: remote_content.to_string(),
            local_content: local_content.to_string(),
            remote_updated_at,
            local_updated_at,
            status: "pending".to_string(),
            created_at: now,
            source_device_id: source_device_id.map(String::from),
        })
    }

    /// Upserts `hive_peer_status` if the device has no row yet (e.g. a
    /// conflict from a peer before its first ping), otherwise increments.
    async fn increment_peer_conflict_count(&self, device_id: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO hive_peer_status (device_id, online, last_synced_at, pending_conflict_count)
                 VALUES (?1, 0, NULL, 1)
                 ON CONFLICT(device_id) DO UPDATE SET
                   pending_conflict_count = pending_conflict_count + 1",
                params![device_id],
            )
            .await?;
        Ok(())
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

    pub async fn hive_list_roster(&self) -> Result<Vec<RosterEntry>> {
        Self::list_roster_on(&self.conn).await
    }

    /// Merges a gossiped roster into the persisted one as a single
    /// read-merge-write transaction and returns the merged result. Every
    /// caller that used to do `hive_list_roster` → `merge_roster` → N ×
    /// `hive_upsert_roster_entry` had a window between the read and the
    /// writes in which a concurrent revoke (the dashboard's revoke handler
    /// racing the ping loop's gossip refresh) could land and then be
    /// overwritten by the stale Active snapshot -- silently un-revoking the
    /// device. Doing all three steps inside one transaction closes that.
    pub async fn hive_merge_roster(&self, incoming: Vec<RosterEntry>) -> Result<Vec<RosterEntry>> {
        let tx = self.conn.transaction().await?;
        let local = Self::list_roster_on(&tx).await?;
        let merged = crate::hive::gossip::merge_roster(local, incoming);
        for entry in &merged {
            Self::upsert_roster_entry_on(&tx, entry).await?;
        }
        tx.commit().await?;
        Ok(merged)
    }

    async fn list_roster_on(conn: &Connection) -> Result<Vec<RosterEntry>> {
        let mut rows = conn
            .query(
                "SELECT device_id, public_key, name, status, joined_at, revoked_at, revoked_by, \
                 join_record, revocation_record FROM hive_devices",
                (),
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            let status_str: String = row.get(3)?;
            let join_record_json: String = row.get(7)?;
            let revocation_record_json: Option<String> = row.get(8)?;
            out.push(RosterEntry {
                device_id: row.get(0)?,
                public_key: row.get(1)?,
                name: row.get(2)?,
                status: if status_str == "revoked" {
                    RosterStatus::Revoked
                } else {
                    RosterStatus::Active
                },
                joined_at: row.get(4)?,
                revoked_at: row.get(5)?,
                revoked_by: row.get(6)?,
                join_record: serde_json::from_str::<JoinRecord>(&join_record_json)?,
                revocation_record: revocation_record_json
                    .map(|s| serde_json::from_str::<RevocationRecord>(&s))
                    .transpose()?,
            });
        }
        Ok(out)
    }

    pub async fn hive_upsert_roster_entry(&self, entry: &RosterEntry) -> Result<()> {
        Self::upsert_roster_entry_on(&self.conn, entry).await
    }

    async fn upsert_roster_entry_on(conn: &Connection, entry: &RosterEntry) -> Result<()> {
        let status_str = match entry.status {
            RosterStatus::Active => "active",
            RosterStatus::Revoked => "revoked",
        };
        let join_record_json = serde_json::to_string(&entry.join_record)?;
        let revocation_record_json = entry
            .revocation_record
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        conn
            .execute(
                "INSERT INTO hive_devices \
                 (device_id, public_key, name, status, joined_at, revoked_at, revoked_by, join_record, revocation_record) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
                 ON CONFLICT(device_id) DO UPDATE SET \
                   public_key = excluded.public_key, name = excluded.name, status = excluded.status, \
                   revoked_at = excluded.revoked_at, revoked_by = excluded.revoked_by, \
                   join_record = excluded.join_record, revocation_record = excluded.revocation_record",
                params![
                    entry.device_id.as_str(),
                    entry.public_key.as_str(),
                    entry.name.as_str(),
                    status_str,
                    entry.joined_at,
                    entry.revoked_at,
                    entry.revoked_by.as_deref(),
                    join_record_json.as_str(),
                    revocation_record_json.as_deref(),
                ],
            )
            .await?;
        Ok(())
    }

    /// Backfills `hive_content_hash` for any memory written before that
    /// column existed (V9 migration added it as nullable; only writes going
    /// through store()/update()/add_tags()/remove_tags() after that point
    /// compute it). Without this, a memory created before hive sync was ever
    /// enabled would keep a NULL hash forever and silently never appear in
    /// the manifest -- never syncing to any peer. Idempotent and cheap once
    /// caught up (the WHERE clause finds nothing on subsequent calls).
    async fn backfill_hive_content_hashes(&self) -> Result<()> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, title, content, layer, memory_type FROM memories WHERE hive_content_hash IS NULL",
                (),
            )
            .await?;
        let mut to_backfill = Vec::new();
        while let Some(row) = rows.next().await? {
            let id: String = row.get(0)?;
            let title: String = row.get(1)?;
            let content: String = row.get(2)?;
            let layer: String = row.get(3)?;
            let memory_type: String = row.get(4)?;
            to_backfill.push((id, title, content, layer, memory_type));
        }
        if to_backfill.is_empty() {
            return Ok(());
        }
        // Tags are fetched before opening the transaction (fetch_tags reads
        // via self.conn, not a transaction handle), then every UPDATE runs
        // inside one transaction -- matching this file's convention
        // elsewhere (store/update/add_tags/remove_tags all batch their
        // writes in a single transaction) rather than one implicit
        // autocommit per row.
        let mut hashes = Vec::with_capacity(to_backfill.len());
        for (id, title, content, layer, memory_type) in &to_backfill {
            let tags = self.fetch_tags(id).await?;
            hashes.push(compute_hive_content_hash(
                title,
                content,
                &tags,
                layer,
                memory_type,
            ));
        }
        let tx = self.conn.transaction().await?;
        for ((id, ..), hash) in to_backfill.iter().zip(hashes.iter()) {
            tx.execute(
                "UPDATE memories SET hive_content_hash = ?2 WHERE id = ?1",
                params![id.as_str(), hash.as_str()],
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn hive_manifest(&self) -> Result<HiveManifest> {
        self.backfill_hive_content_hashes().await?;
        let mut memories = std::collections::HashMap::new();
        let mut rows = self
            .conn
            .query(
                "SELECT id, hive_content_hash, updated_at FROM memories WHERE hive_content_hash IS NOT NULL",
                (),
            )
            .await?;
        while let Some(row) = rows.next().await? {
            let id: String = row.get(0)?;
            let hash: String = row.get(1)?;
            let updated_at: i64 = row.get(2)?;
            memories.insert(id, (hash, updated_at));
        }

        let mut tombstones = std::collections::HashMap::new();
        let mut rows = self
            .conn
            .query("SELECT memory_id, deleted_at FROM hive_tombstones", ())
            .await?;
        while let Some(row) = rows.next().await? {
            let id: String = row.get(0)?;
            let deleted_at: i64 = row.get(1)?;
            tombstones.insert(id, deleted_at);
        }

        let settings_override = self.hive_settings_override().await?;
        let (sync_s, ping_s, settings_updated_at) = settings_override.unwrap_or((300, 60, 0));
        let settings_hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(sync_s.to_le_bytes());
            hasher.update(ping_s.to_le_bytes());
            hex::encode(hasher.finalize())
        };

        let tag_namespaces_updated_at: i64 = self
            .get_meta("tag_namespaces_updated_at")
            .await?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let tag_namespaces_json = self.tag_namespace_registry().await;
        let tag_namespaces_hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(tag_namespaces_json.to_string().as_bytes());
            hex::encode(hasher.finalize())
        };

        Ok(HiveManifest {
            memories,
            tombstones,
            settings: (settings_hash, settings_updated_at),
            tag_namespaces: (tag_namespaces_hash, tag_namespaces_updated_at),
        })
    }

    /// Returns the info a caller needs to push a just-changed memory to
    /// online peers -- call this right after a successful store()/update()/
    /// add_tags()/remove_tags(), not from inside them.
    pub async fn hive_push_payload_for(&self, id: &str) -> Result<Option<serde_json::Value>> {
        let Some(entry) = self.recall_by_id(id).await? else {
            return Ok(None);
        };
        let hash = compute_hive_content_hash(
            &entry.title,
            &entry.content,
            &entry.tags,
            &entry.layer,
            &entry.memory_type,
        );
        Ok(Some(serde_json::json!({
            "kind": "memory", "id": entry.id, "title": entry.title, "content": entry.content,
            "tags": entry.tags, "layer": entry.layer, "memory_type": entry.memory_type,
            "updated_at": entry.updated_at, "hive_content_hash": hash,
        })))
    }

    pub async fn hive_settings_override(&self) -> Result<Option<(u64, u64, i64)>> {
        let Some(raw) = self.get_meta("hive_settings_override").await? else {
            return Ok(None);
        };
        let parsed: (u64, u64, i64) = serde_json::from_str(&raw)?;
        Ok(Some(parsed))
    }

    pub async fn set_hive_settings_override(
        &self,
        sync_interval_seconds: u64,
        ping_interval_seconds: u64,
        updated_at: i64,
    ) -> Result<()> {
        let raw =
            serde_json::to_string(&(sync_interval_seconds, ping_interval_seconds, updated_at))?;
        self.set_meta("hive_settings_override", &raw).await
    }

    pub async fn hive_enabled_override(&self) -> Result<Option<bool>> {
        let Some(raw) = self.get_meta("hive_enabled_override").await? else {
            return Ok(None);
        };
        Ok(Some(raw == "true"))
    }

    pub async fn set_hive_enabled_override(&self, enabled: bool) -> Result<()> {
        self.set_meta(
            "hive_enabled_override",
            if enabled { "true" } else { "false" },
        )
        .await
    }

    pub async fn hive_trusted_networks(&self) -> Result<Vec<crate::hive::network::TrustedNetwork>> {
        let Some(raw) = self.get_meta("hive_trusted_networks").await? else {
            return Ok(Vec::new());
        };
        Ok(serde_json::from_str(&raw)?)
    }

    pub async fn add_hive_trusted_network(&self, id: &str, label: Option<String>) -> Result<()> {
        let mut current = self.hive_trusted_networks().await?;
        if !current.iter().any(|n| n.id == id) {
            current.push(crate::hive::network::TrustedNetwork {
                id: id.to_string(),
                label,
                added_at: chrono::Utc::now().timestamp(),
            });
            self.set_meta("hive_trusted_networks", &serde_json::to_string(&current)?)
                .await?;
        }
        Ok(())
    }

    pub async fn remove_hive_trusted_network(&self, id: &str) -> Result<()> {
        let mut current = self.hive_trusted_networks().await?;
        current.retain(|n| n.id != id);
        self.set_meta("hive_trusted_networks", &serde_json::to_string(&current)?)
            .await
    }

    pub async fn hive_upsert_peer_status(
        &self,
        device_id: &str,
        online: bool,
        last_synced_at: Option<i64>,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO hive_peer_status (device_id, online, last_synced_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(device_id) DO UPDATE SET
                   online = excluded.online,
                   last_synced_at = COALESCE(excluded.last_synced_at, hive_peer_status.last_synced_at)",
                params![device_id, online, last_synced_at],
            )
            .await?;
        Ok(())
    }

    pub async fn hive_get_peer_status(&self, device_id: &str) -> Result<Option<PeerStatusRow>> {
        let mut rows = self
            .conn
            .query(
                "SELECT online, last_synced_at, pending_conflict_count FROM hive_peer_status WHERE device_id = ?1",
                params![device_id],
            )
            .await?;
        Ok(match rows.next().await? {
            Some(row) => Some(PeerStatusRow {
                online: row.get::<i64>(0)? != 0,
                last_synced_at: row.get(1)?,
                pending_conflict_count: row.get(2)?,
            }),
            None => None,
        })
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

pub(crate) fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Debug, PartialEq)]
pub enum ApplyOutcome {
    Applied,
    KeptLocal,
    Conflicted,
}

pub(crate) const HIVE_CLOCK_SKEW_TOLERANCE_SECONDS: i64 = 300;

/// Whether a peer-supplied timestamp is plausible: not further in the future
/// than the clock-skew tolerance. A far-future `updated_at`/`deleted_at` on
/// anything last-write-wins (settings, tag namespaces, tombstones) would
/// otherwise win every comparison forever, wedging that record against all
/// legitimate later edits from every device in the hive.
pub(crate) fn hive_timestamp_is_plausible(ts: i64) -> bool {
    ts <= chrono_now() + HIVE_CLOCK_SKEW_TOLERANCE_SECONDS
}

impl SqliteStore {
    /// The recorded deletion time for `id`, if this device (or a peer whose
    /// tombstone was merged in) has deleted it.
    pub async fn hive_tombstone_for(&self, id: &str) -> Result<Option<i64>> {
        let mut rows = self
            .conn
            .query(
                "SELECT deleted_at FROM hive_tombstones WHERE memory_id = ?1",
                params![id],
            )
            .await?;
        Ok(match rows.next().await? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    /// Applies a peer's tombstone. The local copy is deleted only if it is
    /// strictly older than the deletion (an edit or re-creation made after
    /// the delete wins). Whether or not a row existed, the tombstone is
    /// recorded locally at the peer's `deleted_at` (keeping the later of the
    /// two if one already exists) -- so this device won't resurrect the
    /// memory from a third peer that hasn't caught up yet, and re-advertises
    /// the deletion with its real time rather than "when I heard about it".
    /// Returns whether a local row was actually deleted.
    pub async fn apply_incoming_tombstone(&self, id: &str, deleted_at: i64) -> Result<bool> {
        if let Some(local) = self.recall_by_id(id).await?
            && local.updated_at >= deleted_at
        {
            return Ok(false);
        }
        let tx = self.conn.transaction().await?;
        let changed = tx
            .execute("DELETE FROM memories WHERE id = ?1", params![id])
            .await?;
        tx.execute(
            "INSERT INTO hive_tombstones (memory_id, deleted_at) VALUES (?1, ?2)
             ON CONFLICT(memory_id) DO UPDATE SET
               deleted_at = MAX(hive_tombstones.deleted_at, excluded.deleted_at)",
            params![id, deleted_at],
        )
        .await?;
        tx.commit().await?;
        Ok(changed > 0)
    }

    /// Applies a memory received from a peer (push or pull) under
    /// last-write-wins with a conflict row for ties and implausible clocks.
    /// The content hash is recomputed here from the incoming fields rather
    /// than trusted from the peer's manifest/push body: a peer whose
    /// advertised hash didn't match its content would otherwise be
    /// re-fetched and re-applied on every single sync round.
    pub async fn apply_incoming_memory(
        &self,
        incoming: &MemoryEntry,
        source_device_id: Option<&str>,
    ) -> Result<ApplyOutcome> {
        // A memory already deleted here (or deleted on a peer whose
        // tombstone was merged in) must not come back just because some
        // other peer hasn't processed the deletion yet: the tombstone wins
        // unless the incoming version is strictly newer than the deletion.
        if let Some(deleted_at) = self.hive_tombstone_for(&incoming.id).await?
            && incoming.updated_at <= deleted_at
        {
            return Ok(ApplyOutcome::KeptLocal);
        }

        let now = chrono_now();
        let Some(local) = self.recall_by_id(&incoming.id).await? else {
            // Not present locally at all -- no conflict is possible, this is
            // a plain new-to-us memory. Apply directly, keeping the peer's
            // timestamp (clamped to "now" if the peer's clock is implausibly
            // ahead, so a bad clock can't mint a version nobody can ever
            // overwrite).
            let updated_at = if incoming.updated_at > now + HIVE_CLOCK_SKEW_TOLERANCE_SECONDS {
                now
            } else {
                incoming.updated_at
            };
            self.store_with_timestamp(
                &NewMemoryRow {
                    id: &incoming.id,
                    title: &incoming.title,
                    content: &incoming.content,
                    tags: &incoming.tags,
                    token_count: incoming.token_count,
                    layer: &incoming.layer,
                    memory_type: &incoming.memory_type,
                },
                updated_at,
            )
            .await?;
            return Ok(ApplyOutcome::Applied);
        };

        let local_hash = compute_hive_content_hash(
            &local.title,
            &local.content,
            &local.tags,
            &local.layer,
            &local.memory_type,
        );
        let incoming_hash = compute_hive_content_hash(
            &incoming.title,
            &incoming.content,
            &incoming.tags,
            &incoming.layer,
            &incoming.memory_type,
        );
        if local_hash == incoming_hash {
            return Ok(ApplyOutcome::KeptLocal); // identical, nothing to do
        }

        if incoming.updated_at > now + HIVE_CLOCK_SKEW_TOLERANCE_SECONDS {
            self.write_conflict(
                &incoming.id,
                &incoming.content,
                &local.content,
                incoming.updated_at,
                local.updated_at,
                source_device_id,
            )
            .await?;
            return Ok(ApplyOutcome::Conflicted);
        }

        if incoming.updated_at > local.updated_at {
            self.store_with_timestamp(
                &NewMemoryRow {
                    id: &incoming.id,
                    title: &incoming.title,
                    content: &incoming.content,
                    tags: &incoming.tags,
                    token_count: incoming.token_count,
                    layer: &incoming.layer,
                    memory_type: &incoming.memory_type,
                },
                incoming.updated_at,
            )
            .await?;
            Ok(ApplyOutcome::Applied)
        } else if incoming.updated_at < local.updated_at {
            Ok(ApplyOutcome::KeptLocal)
        } else {
            self.write_conflict(
                &incoming.id,
                &incoming.content,
                &local.content,
                incoming.updated_at,
                local.updated_at,
                source_device_id,
            )
            .await?;
            Ok(ApplyOutcome::Conflicted)
        }
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
