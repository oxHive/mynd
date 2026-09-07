use crate::budget::count_entry_tokens;
use crate::config::{MyndConfig, RecallSource};
use crate::store::{MemoryEntry, SqliteStore};
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    NotFound,
    BudgetExceeded,
}

impl SkipReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            SkipReason::NotFound => "not_found",
            SkipReason::BudgetExceeded => "budget_exceeded",
        }
    }
}

#[derive(Debug)]
pub struct LoadedEntry {
    pub entry: MemoryEntry,
    pub tokens: usize,
    pub source: RecallSource,
}

#[derive(Debug)]
pub struct SkippedEntry {
    pub query: String,
    pub reason: SkipReason,
}

#[derive(Debug)]
pub struct SessionStartResult {
    pub project: String,
    pub loaded: Vec<LoadedEntry>,
    pub skipped: Vec<SkippedEntry>,
    pub used_tokens: usize,
    pub max_tokens: usize,
    pub memories_recalled: usize,
}

impl SessionStartResult {
    pub fn truncated(&self) -> bool {
        !self.skipped.is_empty()
    }
    pub fn remaining(&self) -> usize {
        self.max_tokens.saturating_sub(self.used_tokens)
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "project": self.project,
            "context_loaded": self.loaded.iter().map(|l| serde_json::json!({
                "id": l.entry.id,
                "title": l.entry.title,
                "content": l.entry.content,
                "tags": l.entry.tags,
                "layer": l.entry.layer,
            })).collect::<Vec<_>>(),
            "budget": {
                "used_tokens": self.used_tokens,
                "max_tokens": self.max_tokens,
                "remaining": self.remaining(),
                "truncated": self.truncated(),
            },
            "skipped": self.skipped.iter().map(|s| serde_json::json!({
                "query": s.query,
                "reason": s.reason.as_str(),
            })).collect::<Vec<_>>(),
            "hint": "Session context loaded. Incorporate it silently and proceed with the user's request.",
        })
    }
}

/// Run the configured session-start recalls under the token budget.
/// On over-budget, the entry is skipped and the loop CONTINUES — a later,
/// smaller entry may still fit. Recalls are resolved title -> FTS.
pub async fn execute_session_start(
    config: &MyndConfig,
    store: &SqliteStore,
) -> Result<SessionStartResult> {
    let max_tokens = config.max_tokens;
    let mut used_tokens = 0usize;
    let mut loaded = Vec::new();
    let mut skipped = Vec::new();

    for recall in &config.recalls {
        let is_tag_expr = crate::tag_query::looks_like_tag_expr(&recall.query);

        let entries_result = if is_tag_expr {
            match crate::tag_query::parse(&recall.query) {
                Ok(expr) => store.find_by_tag_expr(&expr).await,
                Err(e) => Err(e),
            }
        } else {
            store.resolve_recall(&recall.query).await
        };

        let entries = match entries_result {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!(
                    "recall \"{}\" failed: {e:#}; treating as not found",
                    recall.query
                );
                skipped.push(SkippedEntry {
                    query: recall.query.clone(),
                    reason: SkipReason::NotFound,
                });
                continue;
            }
        };

        if entries.is_empty() {
            skipped.push(SkippedEntry {
                query: recall.query.clone(),
                reason: SkipReason::NotFound,
            });
            continue;
        }

        // A tag expression can match many memories; a plain title/FTS recall
        // still resolves to at most its single best match.
        let candidates: Vec<_> = if is_tag_expr {
            entries
        } else {
            entries.into_iter().take(1).collect()
        };

        let mut any_loaded = false;
        for entry in candidates {
            let tokens = count_entry_tokens(&entry.title, &entry.content);
            if used_tokens + tokens > max_tokens {
                continue;
            }
            used_tokens += tokens;
            loaded.push(LoadedEntry {
                entry,
                tokens,
                source: recall.source,
            });
            any_loaded = true;
        }
        if !any_loaded {
            skipped.push(SkippedEntry {
                query: recall.query.clone(),
                reason: SkipReason::BudgetExceeded,
            });
        }
    }

    let memories_recalled = loaded.len();
    Ok(SessionStartResult {
        project: config.project_name.clone(),
        loaded,
        skipped,
        used_tokens,
        max_tokens,
        memories_recalled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MyndConfig, Recall, RecallSource};
    use crate::{db, store::SqliteStore};
    use tempfile::TempDir;

    async fn store_with(memories: &[(&str, &str, &str, Vec<String>)]) -> (SqliteStore, TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let sync = crate::config::SyncSettings::default();
        let database = db::open_database(&sync, path.to_str().unwrap())
            .await
            .unwrap();
        let conn = database.connect().unwrap();
        db::run_migrations(&conn).await.unwrap();
        let store = SqliteStore::new(conn);
        for (id, title, content, tags) in memories {
            store
                .store(&crate::store::NewMemoryRow {
                    id,
                    title,
                    content,
                    tags,
                    token_count: None,
                    layer: "workspace",
                    memory_type: "project",
                })
                .await
                .unwrap();
        }
        (store, dir)
    }

    fn config(max: usize, recalls: Vec<&str>) -> MyndConfig {
        MyndConfig {
            project_name: "test-proj".to_string(),
            max_tokens: max,
            recalls: recalls
                .into_iter()
                .map(|q| Recall {
                    query: q.to_string(),
                    source: RecallSource::Project,
                })
                .collect(),
            file_open_rule_count: 0,
            mention_trigger_count: 0,
        }
    }

    #[test]
    fn skip_reason_as_str_values() {
        assert_eq!(SkipReason::NotFound.as_str(), "not_found");
        assert_eq!(SkipReason::BudgetExceeded.as_str(), "budget_exceeded");
    }

    #[test]
    fn session_result_remaining_and_truncated() {
        let result = SessionStartResult {
            project: "p".to_string(),
            loaded: vec![],
            skipped: vec![],
            used_tokens: 300,
            max_tokens: 2000,
            memories_recalled: 0,
        };
        assert_eq!(result.remaining(), 1700);
        assert!(!result.truncated());

        let result_with_skip = SessionStartResult {
            project: "p".to_string(),
            loaded: vec![],
            skipped: vec![SkippedEntry {
                query: "q".to_string(),
                reason: SkipReason::NotFound,
            }],
            used_tokens: 2001,
            max_tokens: 2000,
            memories_recalled: 0,
        };
        assert_eq!(
            result_with_skip.remaining(),
            0,
            "saturating_sub prevents underflow"
        );
        assert!(result_with_skip.truncated());
    }

    #[tokio::test]
    async fn loads_all_recalls_within_budget() {
        let (s, _dir) = store_with(&[
            ("id_a", "pref a", "short content a", vec![]),
            ("id_b", "pref b", "short content b", vec![]),
        ])
        .await;
        let r = execute_session_start(&config(2000, vec!["pref a", "pref b"]), &s)
            .await
            .unwrap();
        assert_eq!(r.loaded.len(), 2);
        assert!(r.skipped.is_empty());
        assert!(!r.truncated());
        assert_eq!(r.project, "test-proj");
        assert!(r.used_tokens > 0);
    }

    #[tokio::test]
    async fn records_not_found_recalls() {
        let (s, _dir) = store_with(&[("id_a", "pref a", "content a", vec![])]).await;
        let r = execute_session_start(&config(2000, vec!["pref a", "does not exist"]), &s)
            .await
            .unwrap();
        assert_eq!(r.loaded.len(), 1);
        assert_eq!(r.skipped.len(), 1);
        assert_eq!(r.skipped[0].reason, SkipReason::NotFound);
        assert!(r.truncated());
    }

    #[tokio::test]
    async fn to_json_matches_mcp_shape() {
        let (s, _dir) = store_with(&[("id_a", "pref a", "short content a", vec![])]).await;
        let r = execute_session_start(&config(2000, vec!["pref a"]), &s)
            .await
            .unwrap();
        let v = r.to_json();
        assert_eq!(v["project"], "test-proj");
        assert_eq!(v["context_loaded"][0]["title"], "pref a");
        assert_eq!(v["budget"]["max_tokens"], 2000);
        assert!(v["skipped"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn skips_over_budget_but_continues_to_smaller_entries() {
        let big = "word ".repeat(400);
        let (s, _dir) = store_with(&[
            ("id_big", "big", &big, vec![]),
            ("id_small", "small", "tiny", vec![]),
        ])
        .await;
        let small_cost = crate::budget::count_entry_tokens("small", "tiny");
        let r = execute_session_start(&config(small_cost + 5, vec!["big", "small"]), &s)
            .await
            .unwrap();
        assert_eq!(r.loaded.len(), 1, "only the small entry fits");
        assert_eq!(r.loaded[0].entry.title, "small");
        assert_eq!(r.skipped.len(), 1);
        assert_eq!(r.skipped[0].reason, SkipReason::BudgetExceeded);
        assert_eq!(r.skipped[0].query, "big");
    }

    #[tokio::test]
    async fn tag_expr_recall_loads_all_matching_memories() {
        let (s, _dir) = store_with(&[
            (
                "id_rust",
                "rust notes",
                "short",
                vec!["lang:rust".to_string(), "project:hivemind".to_string()],
            ),
            (
                "id_vue",
                "vue notes",
                "short",
                vec!["lang:vue".to_string(), "project:hivemind".to_string()],
            ),
            (
                "id_other",
                "other project",
                "short",
                vec!["project:oxhive".to_string()],
            ),
        ])
        .await;
        let r = execute_session_start(&config(2000, vec!["tag:project:hivemind"]), &s)
            .await
            .unwrap();
        assert_eq!(
            r.loaded.len(),
            2,
            "both hivemind-tagged memories should load"
        );
        assert!(r.skipped.is_empty());
        let mut titles: Vec<_> = r.loaded.iter().map(|l| l.entry.title.clone()).collect();
        titles.sort();
        assert_eq!(titles, vec!["rust notes", "vue notes"]);
    }

    #[tokio::test]
    async fn malformed_tag_expr_recall_is_skipped_not_found_not_fts_searched() {
        let (s, _dir) = store_with(&[(
            "id_a",
            "tag:project:hivemind",
            "a memory whose title happens to look like a tag expression",
            vec![],
        )])
        .await;
        // "tag:" with no value is a parse error, not a valid expression —
        // must NOT fall back to treating it as a literal FTS/title query,
        // even though a memory with that literal title exists.
        let r = execute_session_start(&config(2000, vec!["tag:"]), &s)
            .await
            .unwrap();
        assert_eq!(r.loaded.len(), 0);
        assert_eq!(r.skipped.len(), 1);
        assert_eq!(r.skipped[0].reason, SkipReason::NotFound);
    }
}
