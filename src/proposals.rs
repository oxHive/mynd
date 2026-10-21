//! Audit proposals: fixes the background audit agent suggests, held for the
//! user to approve in the dashboard. A proposal is a list of memory writes
//! (create or update; there is no delete) applied in one transaction, and only
//! if none of the memories it touches changed since it was proposed.

use crate::store::{
    SqliteStore, chrono_now, validate_pointer, validate_tag_format, validate_tags_against_registry,
};
use anyhow::{Result, anyhow, bail};
use libsql::params;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// The audit check a proposal comes from. `duplicate` is report-only and is
/// the only kind allowed to carry no ops.
pub const PROPOSAL_KINDS: &[&str] = &[
    "no-root",
    "orphan",
    "several-homes",
    "broken-pointer",
    "stale-pointer",
    "oversized-index",
    "bloated",
    "legacy",
    "cross-reference",
    "duplicate",
];

/// One write in a proposal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum ProposalOp {
    /// A new memory. `ref` names it inside this proposal: a later op can use it
    /// as `parent`, and later content can link it as `(child:ref:<ref>)`.
    Create {
        r#ref: String,
        title: String,
        content: String,
        #[serde(default)]
        tags: Vec<String>,
        /// Existing memory id, or the `ref` of an earlier create. The parent
        /// gets `- [pointer](child:<new id>)` appended.
        #[serde(default)]
        parent: Option<String>,
        /// Pointer text for the parent's new line; required with `parent`.
        #[serde(default)]
        pointer: Option<String>,
    },
    /// A change to an existing memory: `content` replaces the whole content,
    /// `tags` the whole tag list.
    Update {
        id: String,
        #[serde(default)]
        content: Option<String>,
        #[serde(default)]
        tags: Option<Vec<String>>,
    },
}

pub struct NewProposal<'a> {
    pub project: &'a str,
    pub layer: &'a str,
    pub kind: &'a str,
    pub summary: &'a str,
    pub reason: &'a str,
    pub ops: &'a [ProposalOp],
}

#[derive(Debug, Clone, Serialize)]
pub struct ProposalEntry {
    pub id: String,
    pub project: String,
    pub layer: String,
    pub kind: String,
    pub summary: String,
    pub reason: String,
    pub ops: Vec<ProposalOp>,
    /// `updated_at` of every existing memory the ops touch, at proposal time.
    pub base: HashMap<String, i64>,
    pub status: String,
    pub created_at: i64,
}

static REF_NAME_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"^[a-z0-9][a-z0-9-]*$").unwrap());
static REF_LINK_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"\((?:[a-z]+:)?ref:([a-z0-9][a-z0-9-]*)\)").unwrap()
});

/// Every `ref:<name>` a content links must name a create that comes earlier,
/// so its row exists when the link's edge is made.
fn check_refs_defined(text: &str, earlier: &HashSet<String>) -> Result<()> {
    for cap in REF_LINK_RE.captures_iter(text) {
        if !earlier.contains(&cap[1]) {
            bail!(
                "content links ref:{} but no earlier create op defines it",
                &cap[1]
            );
        }
    }
    Ok(())
}

/// Replaces each `ref:<name>)` link target with the id that create got.
pub(crate) fn substitute_refs(text: &str, ids: &HashMap<String, String>) -> String {
    let mut out = text.to_string();
    for (r, id) in ids {
        out = out.replace(&format!("ref:{r})"), &format!("{id})"));
    }
    out
}

fn row_to_proposal(row: &libsql::Row) -> Result<ProposalEntry> {
    let ops: String = row.get(6)?;
    let base: String = row.get(7)?;
    Ok(ProposalEntry {
        id: row.get(0)?,
        project: row.get(1)?,
        layer: row.get(2)?,
        kind: row.get(3)?,
        summary: row.get(4)?,
        reason: row.get(5)?,
        ops: serde_json::from_str(&ops)?,
        base: serde_json::from_str(&base)?,
        status: row.get(8)?,
        created_at: row.get(9)?,
    })
}

const PROPOSAL_COLS: &str =
    "id, project, layer, kind, summary, reason, ops, base, status, created_at";

impl SqliteStore {
    /// Checks `ops` against this store (the one they would apply to) and
    /// returns their `base`: the current `updated_at` of every existing memory
    /// they touch (updated memories and existing parents).
    pub async fn validate_proposal_ops(&self, ops: &[ProposalOp]) -> Result<HashMap<String, i64>> {
        let mut base = HashMap::new();
        let mut earlier: HashSet<String> = HashSet::new();
        for op in ops {
            match op {
                ProposalOp::Create {
                    r#ref,
                    title,
                    content,
                    tags,
                    parent,
                    pointer,
                } => {
                    if !REF_NAME_RE.is_match(r#ref) {
                        bail!(
                            "create ref {:?} must be lowercase letters, digits and dashes",
                            r#ref
                        );
                    }
                    if earlier.contains(r#ref) {
                        bail!("create ref {} is defined twice", r#ref);
                    }
                    validate_tag_format(tags)?;
                    validate_tags_against_registry(self, tags).await?;
                    self.check_content_size(title, content).await?;
                    self.check_root_unique(&format!("ref:{}", r#ref), tags)
                        .await?;
                    check_refs_defined(content, &earlier)?;
                    match (parent, pointer) {
                        (Some(p), Some(ptr)) => {
                            validate_pointer(ptr)?;
                            if !earlier.contains(p) {
                                let m = self.recall_by_id(p).await?.ok_or_else(|| {
                                    anyhow!(
                                        "create {}: parent {p} not found in this store and not an earlier create",
                                        r#ref
                                    )
                                })?;
                                base.insert(p.clone(), m.updated_at);
                            }
                        }
                        (None, None) => {}
                        _ => bail!("create {}: parent and pointer go together", r#ref),
                    }
                    earlier.insert(r#ref.clone());
                }
                ProposalOp::Update { id, content, tags } => {
                    if content.is_none() && tags.is_none() {
                        bail!("update {id} changes nothing");
                    }
                    let m = self
                        .recall_by_id(id)
                        .await?
                        .ok_or_else(|| anyhow!("update: memory {id} not found in this store"))?;
                    let new_tags = tags.clone().unwrap_or_else(|| m.tags.clone());
                    validate_tag_format(&new_tags)?;
                    validate_tags_against_registry(self, &new_tags).await?;
                    self.check_root_unique(id, &new_tags).await?;
                    let new_content = content.as_deref().unwrap_or(&m.content);
                    self.check_content_size(&m.title, new_content).await?;
                    check_refs_defined(new_content, &earlier)?;
                    base.insert(id.clone(), m.updated_at);
                }
            }
        }
        Ok(base)
    }

    pub async fn create_proposal(
        &self,
        p: &NewProposal<'_>,
        base: &HashMap<String, i64>,
    ) -> Result<String> {
        let id = format!("prop_{}", uuid::Uuid::new_v4().simple());
        let _w = self.write().await;
        self.conn
            .execute(
                "INSERT INTO audit_proposals (id, project, layer, kind, summary, reason, ops, base, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending', ?9)",
                params![
                    id.as_str(),
                    p.project,
                    p.layer,
                    p.kind,
                    p.summary,
                    p.reason,
                    serde_json::to_string(p.ops)?,
                    serde_json::to_string(base)?,
                    chrono_now()
                ],
            )
            .await?;
        Ok(id)
    }

    /// Rewrites a pending proposal in place (a revision). `false` when the id
    /// is unknown or no longer pending.
    pub async fn replace_proposal(
        &self,
        id: &str,
        p: &NewProposal<'_>,
        base: &HashMap<String, i64>,
    ) -> Result<bool> {
        let _w = self.write().await;
        let changed = self
            .conn
            .execute(
                "UPDATE audit_proposals SET project = ?2, layer = ?3, kind = ?4, summary = ?5,
                   reason = ?6, ops = ?7, base = ?8
                 WHERE id = ?1 AND status = 'pending'",
                params![
                    id,
                    p.project,
                    p.layer,
                    p.kind,
                    p.summary,
                    p.reason,
                    serde_json::to_string(p.ops)?,
                    serde_json::to_string(base)?
                ],
            )
            .await?;
        Ok(changed > 0)
    }

    pub async fn list_proposals(
        &self,
        project: Option<&str>,
        status: &str,
    ) -> Result<Vec<ProposalEntry>> {
        let mut rows = match project {
            Some(p) => {
                self.conn
                    .query(
                        &format!(
                            "SELECT {PROPOSAL_COLS} FROM audit_proposals WHERE status = ?1 AND project = ?2 ORDER BY created_at, id"
                        ),
                        params![status, p],
                    )
                    .await?
            }
            None => {
                self.conn
                    .query(
                        &format!(
                            "SELECT {PROPOSAL_COLS} FROM audit_proposals WHERE status = ?1 ORDER BY created_at, id"
                        ),
                        params![status],
                    )
                    .await?
            }
        };
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_proposal(&row)?);
        }
        Ok(out)
    }

    pub async fn get_proposal(&self, id: &str) -> Result<Option<ProposalEntry>> {
        let mut rows = self
            .conn
            .query(
                &format!("SELECT {PROPOSAL_COLS} FROM audit_proposals WHERE id = ?1"),
                params![id],
            )
            .await?;
        match rows.next().await? {
            Some(row) => Ok(Some(row_to_proposal(&row)?)),
            None => Ok(None),
        }
    }

    /// Moves a proposal from status `from` to `to`; `false` when it was not in
    /// `from` (already applied, rejected, or unknown).
    pub async fn set_proposal_status(&self, id: &str, from: &str, to: &str) -> Result<bool> {
        let _w = self.write().await;
        let changed = self
            .conn
            .execute(
                "UPDATE audit_proposals SET status = ?3 WHERE id = ?1 AND status = ?2",
                params![id, from, to],
            )
            .await?;
        Ok(changed > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{NewMemoryRow, SqliteStore};
    use crate::{config::SyncSettings, db};
    use tempfile::TempDir;

    fn hid(n: u128) -> String {
        format!("mem_{n:032x}")
    }

    async fn make_store() -> (SqliteStore, TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let database = db::open_database(&SyncSettings::default(), path.to_str().unwrap())
            .await
            .unwrap();
        let conn = database.connect().unwrap();
        db::run_migrations(&conn).await.unwrap();
        (SqliteStore::new(conn), dir)
    }

    async fn put(s: &SqliteStore, id: &str, title: &str, content: &str, tags: &[&str]) {
        let tags: Vec<String> = tags.iter().map(|t| t.to_string()).collect();
        s.store(&NewMemoryRow {
            id,
            title,
            content,
            tags: &tags,
            token_count: None,
            layer: "workspace",
            memory_type: "project",
        })
        .await
        .unwrap();
    }

    fn create(r: &str, content: &str, parent: Option<&str>) -> ProposalOp {
        ProposalOp::Create {
            r#ref: r.to_string(),
            title: format!("new {r}"),
            content: content.to_string(),
            tags: vec!["project:mynd".to_string()],
            parent: parent.map(str::to_string),
            pointer: parent.map(|_| format!("{r}: what it holds; open when needed")),
        }
    }

    #[test]
    fn ops_round_trip_through_json_with_op_tag_and_ref_field() {
        let json = serde_json::json!([
            { "op": "create", "ref": "a", "title": "t", "content": "c" },
            { "op": "update", "id": "mem_x", "tags": ["project:mynd"] }
        ]);
        let ops: Vec<ProposalOp> = serde_json::from_value(json.clone()).unwrap();
        assert!(matches!(&ops[0], ProposalOp::Create { r#ref, .. } if r#ref == "a"));
        assert_eq!(serde_json::to_value(&ops).unwrap()[0]["ref"], "a");
    }

    #[tokio::test]
    async fn validate_returns_base_for_every_existing_memory_touched() {
        let (s, _d) = make_store().await;
        put(&s, &hid(1), "root", "# r", &["role:root", "project:mynd"]).await;
        put(&s, &hid(2), "leaf", "l", &["project:mynd"]).await;
        let ops = vec![
            create("idx", "# idx", Some(&hid(1))),
            ProposalOp::Update {
                id: hid(2),
                content: Some("l2".into()),
                tags: None,
            },
        ];
        let base = s.validate_proposal_ops(&ops).await.unwrap();
        let mut keys: Vec<_> = base.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, vec![hid(1), hid(2)]);
    }

    #[tokio::test]
    async fn validate_rejects_bad_ops() {
        let (s, _d) = make_store().await;
        put(&s, &hid(1), "root", "# r", &["role:root", "project:mynd"]).await;
        let cases: Vec<(Vec<ProposalOp>, &str)> = vec![
            (vec![create("a", "c", Some(&hid(9)))], "not found"),
            (
                vec![ProposalOp::Update {
                    id: hid(9),
                    content: Some("x".into()),
                    tags: None,
                }],
                "not found",
            ),
            (
                vec![ProposalOp::Update {
                    id: hid(1),
                    content: None,
                    tags: None,
                }],
                "changes nothing",
            ),
            (vec![create("Bad Ref", "c", None)], "ref"),
            (
                vec![create("a", "c", None), create("a", "c", None)],
                "twice",
            ),
            (
                vec![
                    create("a", "- [b](child:ref:b)", None),
                    create("b", "c", None),
                ],
                "ref:b",
            ),
            (
                vec![create("b", "c", Some("later")), create("later", "c", None)],
                "parent",
            ),
            (
                vec![ProposalOp::Create {
                    r#ref: "r2".into(),
                    title: "second root".into(),
                    content: "#".into(),
                    tags: vec!["role:root".into(), "project:mynd".into()],
                    parent: None,
                    pointer: None,
                }],
                "already has a root",
            ),
        ];
        for (ops, needle) in cases {
            let err = s.validate_proposal_ops(&ops).await.unwrap_err();
            assert!(err.to_string().contains(needle), "{needle}: {err}");
        }
    }

    #[tokio::test]
    async fn proposals_crud_round_trip_and_replace_only_while_pending() {
        let (s, _d) = make_store().await;
        put(&s, &hid(1), "leaf", "l", &["project:mynd"]).await;
        let ops = vec![ProposalOp::Update {
            id: hid(1),
            content: Some("l2".into()),
            tags: None,
        }];
        let base = s.validate_proposal_ops(&ops).await.unwrap();
        let p = NewProposal {
            project: "mynd",
            layer: "workspace",
            kind: "stale-pointer",
            summary: "s",
            reason: "r",
            ops: &ops,
        };
        let id = s.create_proposal(&p, &base).await.unwrap();
        let got = s.get_proposal(&id).await.unwrap().unwrap();
        assert_eq!(got.status, "pending");
        assert_eq!(got.ops, ops);
        assert_eq!(got.base.get(&hid(1)).copied(), base.get(&hid(1)).copied());
        assert_eq!(
            s.list_proposals(Some("mynd"), "pending")
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(
            s.list_proposals(Some("other"), "pending")
                .await
                .unwrap()
                .is_empty()
        );

        let revised = NewProposal { summary: "s2", ..p };
        assert!(s.replace_proposal(&id, &revised, &base).await.unwrap());
        assert_eq!(s.get_proposal(&id).await.unwrap().unwrap().summary, "s2");

        assert!(
            s.set_proposal_status(&id, "pending", "rejected")
                .await
                .unwrap()
        );
        assert!(
            !s.set_proposal_status(&id, "pending", "applied")
                .await
                .unwrap()
        );
        assert!(!s.replace_proposal(&id, &revised, &base).await.unwrap());
    }
}
