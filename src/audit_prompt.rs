//! The background audit agent's prompt. The checks are read verbatim from
//! the /memory-audit skill, so the in-session and dashboard audits can never
//! drift apart; only the write path differs (memory_propose here).

use crate::prompt_data::{DATA_NOTICE, single_line};
use crate::store::SqliteStore;

const AUDIT_SKILL: &str = include_str!("../plugins/claude/skills/memory-audit.md");
const START: &str = "<!-- audit-checks:start -->";
const END: &str = "<!-- audit-checks:end -->";

/// The skill's checks section, between its two markers.
pub(crate) fn audit_checks() -> &'static str {
    let start = AUDIT_SKILL.find(START).map(|i| i + START.len());
    let end = AUDIT_SKILL.find(END);
    match (start, end) {
        (Some(s), Some(e)) if s < e => AUDIT_SKILL[s..e].trim(),
        _ => "",
    }
}

pub(crate) async fn build_audit_prompt(
    store: &SqliteStore,
    project: &str,
    layer: &str,
) -> anyhow::Result<String> {
    let tree = store.memory_tree(project).await?;
    let mut lines = Vec::new();
    for n in &tree.nodes {
        let mut line = format!(
            "{} | {} | role {} | {} tokens | homes [{}] | {} | updated {} | tags [{}]",
            n.id,
            single_line(&n.title),
            n.role.as_deref().unwrap_or("none"),
            n.tokens,
            n.homes.join(", "),
            if n.reachable {
                "reachable"
            } else {
                "UNREACHED"
            },
            n.updated_at,
            n.tags.join(", "),
        );
        if !n.broken_pointers.is_empty() {
            line.push_str(&format!(
                " | broken pointers [{}]",
                n.broken_pointers.join(", ")
            ));
        }
        if !n.parent_links.is_empty() {
            line.push_str(&format!(" | parent links [{}]", n.parent_links.join(", ")));
        }
        if !n.broken_cross_refs.is_empty() {
            line.push_str(&format!(
                " | broken cross-references [{}]",
                n.broken_cross_refs.join(", ")
            ));
        }
        lines.push(line);
        for p in &n.pointers {
            lines.push(format!("    -> {} : {}", p.target_id, single_line(&p.text)));
        }
        for c in &n.cross_refs {
            lines.push(format!("    ~> {} : {}", c.target_id, single_line(&c.text)));
        }
    }
    let tree_text = if lines.is_empty() {
        format!("  (no memories tagged project:{project})")
    } else {
        lines.join("\n")
    };
    let root = tree.root_id.as_deref().unwrap_or("none");
    let checks = audit_checks();
    Ok(format!(
        "Mynd memory audit: project {project}, {layer} store\n\
         {DATA_NOTICE}\n\n\
         TREE (from memory_tree, no content; root: {root}; `->` is a pointer, `~>` a cross-reference):\n\
         {tree_text}\n\n\
         {checks}\n\n\
         ## How to propose fixes\n\n\
         You cannot change memories. For every fix, call memory_propose once with project \"{project}\", \
         layer \"{layer}\", kind (no-root, orphan, several-homes, broken-pointer, stale-pointer, \
         oversized-index, bloated, legacy, cross-reference or duplicate), a one-line summary, a one or two \
         sentence reason, and ops.\n\
         - Ops run in the order given, in one transaction, and only if no memory they touch changed meanwhile.\n\
         - {{\"op\": \"create\", \"ref\": \"<short-name>\", \"title\", \"content\", \"tags\", \"parent\", \"pointer\"}} \
         stores a new memory. With parent (an existing id or an earlier create's ref) and pointer, the parent \
         gets the pointer line automatically.\n\
         - {{\"op\": \"update\", \"id\", \"content\", \"tags\"}} changes an existing memory: content is the whole \
         new content and tags the whole new list. Copy the current content from memory_recall and change only \
         the lines the fix needs.\n\
         - Inside content, link a memory created earlier in the same proposal as (child:ref:<short-name>).\n\
         - New memories and new lines come before removing old lines or content.\n\
         - Report a duplicate with kind duplicate and no ops.\n\n\
         Use memory_recall to read content and memory_tree to re-read structure. Propose each fix once, \
         then stop: the user reviews them in the dashboard."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_checks_are_the_skill_section_between_the_markers() {
        let checks = audit_checks();
        assert!(checks.starts_with("## Checks"), "{checks}");
        assert!(checks.contains("1. **No root.**"));
        assert!(checks.contains("10. **Duplicates.**"));
        assert!(
            !checks.contains("audit-checks:"),
            "markers are not included"
        );
    }

    #[tokio::test]
    async fn build_audit_prompt_lists_the_tree_and_the_propose_contract() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let database = crate::db::open_database(
            &crate::config::SyncSettings::default(),
            path.to_str().unwrap(),
        )
        .await
        .unwrap();
        let conn = database.connect().unwrap();
        crate::db::run_migrations(&conn).await.unwrap();
        let store = crate::store::SqliteStore::new(conn);
        store
            .store(&crate::store::NewMemoryRow {
                id: "mem_00000000000000000000000000000001",
                title: "orphan\nwith newline",
                content: "body text that must not appear",
                tags: &["project:mynd".to_string()],
                token_count: None,
                layer: "workspace",
                memory_type: "project",
            })
            .await
            .unwrap();
        store
            .store(&crate::store::NewMemoryRow {
                id: "mem_00000000000000000000000000000002",
                title: "leaf with links",
                content: "see [the orphan's setup notes](mem_00000000000000000000000000000001) and [gone](mem_00000000000000000000000000000009)",
                tags: &["project:mynd".to_string()],
                token_count: None,
                layer: "workspace",
                memory_type: "project",
            })
            .await
            .unwrap();
        let p = build_audit_prompt(&store, "mynd", "workspace")
            .await
            .unwrap();
        assert!(p.contains(crate::prompt_data::DATA_NOTICE));
        assert!(p.contains("orphan with newline"), "titles stay on one line");
        assert!(p.contains("UNREACHED"));
        assert!(
            p.contains("~> mem_00000000000000000000000000000001 : the orphan's setup notes"),
            "a leaf's cross-references are listed"
        );
        assert!(p.contains("broken cross-references [mem_00000000000000000000000000000009]"));
        assert!(
            !p.contains("body text that must not appear"),
            "no content in the prompt"
        );
        assert!(p.contains("memory_propose"));
        assert!(p.contains("1. **No root.**"));
    }
}
