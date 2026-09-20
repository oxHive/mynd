use super::*;
use crate::{config::SyncSettings, db};
use tempfile::TempDir;

async fn make_store() -> (SqliteStore, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    (SqliteStore::new(conn), dir)
}

#[tokio::test]
async fn store_persists_row_and_tags() {
    let (s, _dir) = make_store().await;
    s.store(&test_row(
        "mem_1",
        "My Title",
        "content here",
        &["rust".into(), "test".into()],
    ))
    .await
    .unwrap();
    let entry = s.recall_by_id("mem_1").await.unwrap().unwrap();
    assert_eq!(entry.title, "My Title");
    assert_eq!(entry.content, "content here");
    assert!(entry.tags.contains(&"rust".to_string()));
    assert!(entry.tags.contains(&"test".to_string()));
}

#[tokio::test]
async fn store_deduplicates_tags() {
    let (s, _dir) = make_store().await;
    s.store(&test_row(
        "mem_2",
        "Title",
        "body",
        &["rust".into(), "rust".into()],
    ))
    .await
    .unwrap();
    let entry = s.recall_by_id("mem_2").await.unwrap().unwrap();
    assert_eq!(entry.tags.len(), 1);
}

#[tokio::test]
async fn store_lowercases_tags() {
    let (s, _dir) = make_store().await;
    s.store(&test_row(
        "mem_upper",
        "Title",
        "content",
        &["Lang:Rust".into(), "PROJECT:HiveMind".into()],
    ))
    .await
    .unwrap();
    let entry = s.recall_by_id("mem_upper").await.unwrap().unwrap();
    assert!(entry.tags.contains(&"lang:rust".to_string()));
    assert!(entry.tags.contains(&"project:hivemind".to_string()));
}

#[tokio::test]
async fn store_rejects_more_than_one_project_tag() {
    let (s, _dir) = make_store().await;
    let result = s
        .store(&test_row(
            "mem_multi_project",
            "Title",
            "content",
            &["project:hivemind".into(), "project:oxhive".into()],
        ))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn store_rejects_empty_tag() {
    let (s, _dir) = make_store().await;
    let result = s
        .store(&test_row(
            "mem_empty_tag",
            "Title",
            "content",
            &["  ".into()],
        ))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn store_rejects_overlong_tag() {
    let (s, _dir) = make_store().await;
    let long_tag = "x".repeat(129);
    let result = s
        .store(&test_row("mem_long_tag", "Title", "content", &[long_tag]))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn custom_singleton_namespace_is_enforced_from_registry() {
    let (s, _dir) = make_store().await;
    s.set_meta(
        "tag_namespaces",
        r##"{"owner": {"color": "#fff", "values": [], "single_value": true}}"##,
    )
    .await
    .unwrap();
    let result = s
        .store(&test_row(
            "mem_two_owners",
            "Title",
            "content",
            &["owner:alice".into(), "owner:bob".into()],
        ))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn tag_namespace_registry_backfills_namespace_added_to_code_after_last_save() {
    // Simulates a stored blob saved before `part` existed in
    // default_tag_namespaces() — the registry must still surface `part`
    // (and every other current default) rather than being permanently
    // shadowed by the stale snapshot.
    let (s, _dir) = make_store().await;
    s.set_meta(
        "tag_namespaces",
        r##"{"project": {"color": "#4a9eff", "values": [], "single_value": true}}"##,
    )
    .await
    .unwrap();
    let registry = s.tag_namespace_registry().await;
    assert_eq!(
        registry["part"]["values"],
        serde_json::json!(["index", "fragment"])
    );
    assert_eq!(registry["project"]["color"], "#4a9eff");
}

#[tokio::test]
async fn tag_namespace_registry_lets_stored_customization_win_over_default() {
    let (s, _dir) = make_store().await;
    s.set_meta(
        "tag_namespaces",
        r##"{"project": {"color": "#ff0000", "values": [], "single_value": true}}"##,
    )
    .await
    .unwrap();
    let registry = s.tag_namespace_registry().await;
    assert_eq!(registry["project"]["color"], "#ff0000");
}

#[tokio::test]
async fn fixed_values_namespace_rejects_unregistered_value() {
    let (s, _dir) = make_store().await;
    s.set_meta(
        "tag_namespaces",
        r##"{"status": {"color": "#fff", "values": ["idea", "done"], "values_mode": "fixed"}}"##,
    )
    .await
    .unwrap();
    let result = s
        .store(&test_row(
            "mem_bad_status",
            "Title",
            "content",
            &["status:archived".into()],
        ))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn fixed_values_namespace_accepts_registered_value_case_insensitively() {
    let (s, _dir) = make_store().await;
    s.set_meta(
        "tag_namespaces",
        r##"{"status": {"color": "#fff", "values": ["idea", "done"], "values_mode": "fixed"}}"##,
    )
    .await
    .unwrap();
    let result = s
        .store(&test_row(
            "mem_ok_status",
            "Title",
            "content",
            &["status:Done".into()],
        ))
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn fixed_values_namespace_with_empty_list_is_not_restrictive() {
    let (s, _dir) = make_store().await;
    s.set_meta(
        "tag_namespaces",
        r##"{"status": {"color": "#fff", "values": [], "values_mode": "fixed"}}"##,
    )
    .await
    .unwrap();
    let result = s
        .store(&test_row(
            "mem_empty_fixed",
            "Title",
            "content",
            &["status:anything".into()],
        ))
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn suggestion_mode_namespace_accepts_any_value() {
    let (s, _dir) = make_store().await;
    s.set_meta(
        "tag_namespaces",
        r##"{"status": {"color": "#fff", "values": ["idea", "done"], "values_mode": "suggestion"}}"##,
    )
    .await
    .unwrap();
    let result = s
        .store(&test_row(
            "mem_suggestion",
            "Title",
            "content",
            &["status:whatever".into()],
        ))
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn add_tags_merges_without_touching_existing() {
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_add", "Title", "content", &["a".into()]))
        .await
        .unwrap();
    let ok = s
        .add_tags("mem_add", &["b".into(), "a".into()])
        .await
        .unwrap();
    assert!(ok);
    let entry = s.recall_by_id("mem_add").await.unwrap().unwrap();
    let mut tags = entry.tags.clone();
    tags.sort();
    assert_eq!(tags, vec!["a".to_string(), "b".to_string()]);
}

#[tokio::test]
async fn remove_tags_drops_only_named_tags() {
    let (s, _dir) = make_store().await;
    s.store(&test_row(
        "mem_rm",
        "Title",
        "content",
        &["a".into(), "b".into()],
    ))
    .await
    .unwrap();
    let ok = s.remove_tags("mem_rm", &["a".into()]).await.unwrap();
    assert!(ok);
    let entry = s.recall_by_id("mem_rm").await.unwrap().unwrap();
    assert_eq!(entry.tags, vec!["b".to_string()]);
}

#[tokio::test]
async fn add_tags_returns_false_for_missing_memory() {
    let (s, _dir) = make_store().await;
    let ok = s.add_tags("mem_missing", &["a".into()]).await.unwrap();
    assert!(!ok);
}

#[tokio::test]
async fn update_rejects_more_than_one_project_tag() {
    let (s, _dir) = make_store().await;
    let tags: Vec<String> = vec![];
    s.store(&test_row("mem_up", "Title", "content", &tags))
        .await
        .unwrap();
    let result = s
        .update(
            "mem_up",
            "Title",
            "content",
            &["project:a".into(), "project:b".into()],
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn delete_removes_memory_tags_and_fts() {
    let (s, _dir) = make_store().await;
    s.store(&test_row(
        "mem_del",
        "Delete Me",
        "some content",
        &["tag1".into()],
    ))
    .await
    .unwrap();
    s.delete("mem_del").await.unwrap();
    assert!(s.recall_by_id("mem_del").await.unwrap().is_none());
    let results = s.search("some content", 10).await.unwrap();
    assert!(results.is_empty(), "FTS should not return deleted memory");
}

#[tokio::test]
async fn delete_removes_connected_edges() {
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_e1", "E1", "body", &["tag_e".into()]))
        .await
        .unwrap();
    s.store(&test_row("mem_e2", "E2", "body", &["tag_e".into()]))
        .await
        .unwrap();
    s.delete("mem_e1").await.unwrap();
    let edges = s.list_edges(None).await.unwrap();
    assert!(
        edges.is_empty(),
        "edges involving deleted memory should be gone"
    );
}

#[tokio::test]
async fn search_returns_results_for_matching_content() {
    let (s, _dir) = make_store().await;
    s.store(&test_row(
        "mem_s1",
        "Rust Tips",
        "use iterators not loops",
        &[],
    ))
    .await
    .unwrap();
    let results = s.search("iterators", 10).await.unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].id, "mem_s1");
}

#[tokio::test]
async fn conflict_round_trip() {
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_c1", "C1", "local content", &[]))
        .await
        .unwrap();
    let entry = s.recall_by_id("mem_c1").await.unwrap().unwrap();
    let conflict = s
        .write_conflict(
            "mem_c1",
            "remote content",
            "local content",
            entry.updated_at + 1,
            entry.updated_at,
        )
        .await
        .unwrap();
    let fetched = s.get_conflict_by_id(&conflict.id).await.unwrap().unwrap();
    assert_eq!(fetched.remote_content, "remote content");
    let resolved = s
        .resolve_conflict(&conflict.id, "keep_local")
        .await
        .unwrap();
    assert!(resolved);
    let after = s.get_conflict_by_id(&conflict.id).await.unwrap().unwrap();
    assert_eq!(after.status, "keep_local");
}

#[tokio::test]
async fn update_returns_false_for_missing_id() {
    let (s, _dir) = make_store().await;
    let updated = s
        .update("mem_nonexistent", "title", "new content", &[])
        .await
        .unwrap();
    assert!(!updated);
}

#[tokio::test]
async fn list_memories_returns_all_stored() {
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_a", "Alpha", "first", &["a".into()]))
        .await
        .unwrap();
    s.store(&test_row("mem_b", "Beta", "second", &["b".into()]))
        .await
        .unwrap();
    let list = s.list_memories(10, 0).await.unwrap();
    assert_eq!(list.len(), 2);
}

#[tokio::test]
async fn find_by_tag_expr_returns_matching_memories() {
    let (s, _dir) = make_store().await;
    s.store(&test_row(
        "mem_rust",
        "Rust notes",
        "content",
        &["lang:rust".into(), "project:hivemind".into()],
    ))
    .await
    .unwrap();
    s.store(&test_row(
        "mem_vue",
        "Vue notes",
        "content",
        &["lang:vue".into(), "project:hivemind".into()],
    ))
    .await
    .unwrap();
    s.store(&test_row(
        "mem_other",
        "Unrelated",
        "content",
        &["project:oxhive".into()],
    ))
    .await
    .unwrap();

    let expr = crate::tag_query::parse("tag:project:hivemind & tag:lang:rust").unwrap();
    let results = s.find_by_tag_expr(&expr).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Rust notes");

    let expr_or = crate::tag_query::parse("tag:lang:rust | tag:lang:vue").unwrap();
    let mut results_or = s.find_by_tag_expr(&expr_or).await.unwrap();
    results_or.sort_by(|a, b| a.title.cmp(&b.title));
    assert_eq!(results_or.len(), 2);
    assert_eq!(results_or[0].title, "Rust notes");
    assert_eq!(results_or[1].title, "Vue notes");
}

#[tokio::test]
async fn list_edges_filtered_by_memory_id() {
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_x", "X", "body", &["shared_tag".into()]))
        .await
        .unwrap();
    s.store(&test_row("mem_y", "Y", "body", &["shared_tag".into()]))
        .await
        .unwrap();
    s.store(&test_row("mem_z", "Z", "body", &["other_tag".into()]))
        .await
        .unwrap();
    s.create_edge("mem_x", "mem_z", "sibling").await.unwrap();

    let all = s.list_edges(None).await.unwrap();
    let filtered = s.list_edges(Some("mem_x")).await.unwrap();

    assert!(all.len() >= filtered.len());
    assert!(
        filtered
            .iter()
            .all(|e| e.source_id == "mem_x" || e.target_id == "mem_x")
    );
}

#[tokio::test]
async fn set_edge_status_updates() {
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_p", "P", "body", &[])).await.unwrap();
    s.store(&test_row("mem_q", "Q", "body", &[])).await.unwrap();
    let edge = s.create_edge("mem_p", "mem_q", "sibling").await.unwrap();
    let crate::model::EdgeCreate::Created(edge_id) = edge else {
        panic!("expected EdgeCreate::Created");
    };
    let ok = s.set_edge_status(&edge_id, "inactive").await.unwrap();
    assert!(ok);
    let edges = s.list_edges(None).await.unwrap();
    let updated = edges.iter().find(|e| e.id == edge_id).unwrap();
    assert_eq!(updated.status, "inactive");
}

#[tokio::test]
async fn create_edge_with_reason_roundtrips() {
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_a", "A", "a", &[])).await.unwrap();
    s.store(&test_row("mem_b", "B", "b", &[])).await.unwrap();
    let created = s
        .create_edge_with_status(
            "mem_a",
            "mem_b",
            "sibling",
            "pending",
            None,
            Some("both cover auth"),
        )
        .await
        .unwrap();
    assert!(matches!(created, crate::model::EdgeCreate::Created(_)));
    let edges = s.list_edges(None).await.unwrap();
    assert_eq!(edges[0].reason.as_deref(), Some("both cover auth"));
    assert_eq!(edges[0].status, "pending");
}

#[tokio::test]
async fn set_edge_status_returns_false_for_missing() {
    let (s, _dir) = make_store().await;
    let ok = s
        .set_edge_status("edge_nonexistent", "inactive")
        .await
        .unwrap();
    assert!(!ok);
}

#[tokio::test]
async fn update_edge_patches_fields_in_place() {
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_a", "A", "a", &[])).await.unwrap();
    s.store(&test_row("mem_b", "B", "b", &[])).await.unwrap();
    let created = s
        .create_edge_with_status(
            "mem_a",
            "mem_b",
            "sibling",
            "pending",
            None,
            Some("first take"),
        )
        .await
        .unwrap();
    let crate::model::EdgeCreate::Created(id) = created else {
        panic!("expected EdgeCreate::Created");
    };
    let ok = s
        .update_edge(&id, Some("parent"), Some("refined reason"), None)
        .await
        .unwrap();
    assert!(ok);
    let e = s.get_edge(&id).await.unwrap().unwrap();
    assert_eq!(e.relationship, "parent");
    assert_eq!(e.reason.as_deref(), Some("refined reason"));
    assert_eq!(e.status, "pending", "status untouched");
}

#[tokio::test]
async fn update_edge_rejects_invalid_relationship_and_missing_id() {
    let (s, _dir) = make_store().await;
    assert!(
        !s.update_edge("edge_missing", None, Some("x"), None)
            .await
            .unwrap()
    );

    s.store(&test_row("mem_a", "A", "a", &[])).await.unwrap();
    s.store(&test_row("mem_b", "B", "b", &[])).await.unwrap();
    let crate::model::EdgeCreate::Created(id) =
        s.create_edge("mem_a", "mem_b", "sibling").await.unwrap()
    else {
        panic!("expected EdgeCreate::Created");
    };
    assert!(
        s.update_edge(&id, Some("related_to"), None, None)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn create_edge_reports_duplicate_and_missing_endpoint() {
    use crate::model::EdgeCreate;
    let (s, _dir) = make_store().await;
    let tags: Vec<String> = vec![];
    s.store(&test_row("mem_1", "A", "a", &tags)).await.unwrap();
    s.store(&test_row("mem_2", "B", "b", &tags)).await.unwrap();

    let first = s.create_edge("mem_1", "mem_2", "sibling").await.unwrap();
    assert!(matches!(first, EdgeCreate::Created(_)));
    // duplicate, even reversed
    assert_eq!(
        s.create_edge("mem_2", "mem_1", "sibling").await.unwrap(),
        EdgeCreate::Duplicate
    );
    assert_eq!(
        s.create_edge("mem_1", "mem_ghost", "sibling")
            .await
            .unwrap(),
        EdgeCreate::MissingEndpoint
    );
    assert_eq!(
        s.create_edge("mem_1", "mem_2", "banana").await.unwrap(),
        EdgeCreate::InvalidRelationship
    );
}

#[tokio::test]
async fn list_feedback_filtered_by_memory_id() {
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_f1", "F1", "body", &[]))
        .await
        .unwrap();
    s.store(&test_row("mem_f2", "F2", "body", &[]))
        .await
        .unwrap();
    s.create_feedback("mem_f1", "positive", None).await.unwrap();
    s.create_feedback("mem_f2", "negative", Some("outdated"))
        .await
        .unwrap();

    let all = s.list_feedback(None, None).await.unwrap();
    let filtered = s.list_feedback(Some("mem_f1"), None).await.unwrap();

    assert_eq!(all.len(), 2);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].memory_id, "mem_f1");
}

#[tokio::test]
async fn set_feedback_status_updates() {
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_g", "G", "body", &[])).await.unwrap();
    let fb = s
        .create_feedback("mem_g", "negative", None)
        .await
        .unwrap()
        .unwrap();
    let ok = s.set_feedback_status(&fb.id, "resolved").await.unwrap();
    assert!(ok);
    let items = s.list_feedback(Some("mem_g"), None).await.unwrap();
    assert_eq!(items[0].status, "resolved");
}

#[tokio::test]
async fn set_feedback_status_returns_false_for_missing() {
    let (s, _dir) = make_store().await;
    let ok = s
        .set_feedback_status("fb_nonexistent", "resolved")
        .await
        .unwrap();
    assert!(!ok);
}

#[tokio::test]
async fn list_conflicts_returns_entries() {
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_h", "H", "local", &[]))
        .await
        .unwrap();
    let entry = s.recall_by_id("mem_h").await.unwrap().unwrap();
    s.write_conflict(
        "mem_h",
        "remote",
        "local",
        entry.updated_at + 1,
        entry.updated_at,
    )
    .await
    .unwrap();
    let conflicts = s.list_conflicts(None).await.unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].memory_id, "mem_h");
}

#[tokio::test]
async fn get_conflict_by_id_returns_none_for_missing() {
    let (s, _dir) = make_store().await;
    let result = s.get_conflict_by_id("conflict_nonexistent").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn get_edges_grouped_reads_parent_and_child_from_both_directions() {
    const PARENT: &str = "mem_11111111111111111111111111111111";
    const CHILD: &str = "mem_22222222222222222222222222222222";
    let (s, _dir) = make_store().await;
    s.store(&test_row(CHILD, "Child", "child body", &[]))
        .await
        .unwrap();
    s.store(&test_row(PARENT, "Parent", "parent body", &[]))
        .await
        .unwrap();
    // CHILD asserts PARENT is its parent.
    s.update(CHILD, "Child", &format!("[the rule](parent:{PARENT})"), &[])
        .await
        .unwrap();

    let from_child = s.get_edges_grouped(CHILD).await.unwrap();
    assert_eq!(from_child.parents.len(), 1);
    assert_eq!(from_child.parents[0].id, PARENT);
    assert_eq!(from_child.parents[0].link_text.as_deref(), Some("the rule"));
    assert!(from_child.children.is_empty());

    // From the parent's side, the same edge should surface CHILD as a child,
    // even though PARENT never authored a `child:` link itself.
    let from_parent = s.get_edges_grouped(PARENT).await.unwrap();
    assert_eq!(from_parent.children.len(), 1);
    assert_eq!(from_parent.children[0].id, CHILD);
    assert!(from_parent.parents.is_empty());
}

#[tokio::test]
async fn get_edges_grouped_siblings_symmetric() {
    const A: &str = "mem_33333333333333333333333333333333";
    const B: &str = "mem_44444444444444444444444444444444";
    let (s, _dir) = make_store().await;
    s.store(&test_row(A, "A", "a", &[])).await.unwrap();
    s.store(&test_row(B, "B", &format!("[peer](sibling:{A})"), &[]))
        .await
        .unwrap();

    let from_a = s.get_edges_grouped(A).await.unwrap();
    assert_eq!(from_a.siblings.len(), 1);
    assert_eq!(from_a.siblings[0].id, B);

    let from_b = s.get_edges_grouped(B).await.unwrap();
    assert_eq!(from_b.siblings.len(), 1);
    assert_eq!(from_b.siblings[0].id, A);
}

#[tokio::test]
async fn get_edges_grouped_dedupes_mutual_reciprocal_links() {
    const A: &str = "mem_55555555555555555555555555555555";
    const B: &str = "mem_66666666666666666666666666666666";
    let (s, _dir) = make_store().await;
    s.store(&test_row(A, "A", "a", &[])).await.unwrap();
    s.store(&test_row(B, "B", "b", &[])).await.unwrap();
    // Each side independently authors a reciprocal sibling link, producing
    // two distinct edge rows: (A -> B, sibling) and (B -> A, sibling).
    s.update(A, "A", &format!("[my sibling](sibling:{B})"), &[])
        .await
        .unwrap();
    s.update(B, "B", &format!("[my sibling too](sibling:{A})"), &[])
        .await
        .unwrap();

    let from_a = s.get_edges_grouped(A).await.unwrap();
    assert_eq!(
        from_a.siblings.len(),
        1,
        "expected exactly one sibling entry, got {:?}",
        from_a.siblings
    );
    assert_eq!(from_a.siblings[0].id, B);

    let from_b = s.get_edges_grouped(B).await.unwrap();
    assert_eq!(
        from_b.siblings.len(),
        1,
        "expected exactly one sibling entry, got {:?}",
        from_b.siblings
    );
    assert_eq!(from_b.siblings[0].id, A);
}

#[tokio::test]
async fn get_edges_grouped_dedupes_mutual_parent_child_links() {
    const A: &str = "mem_77777777777777777777777777777777";
    const B: &str = "mem_88888888888888888888888888888888";
    let (s, _dir) = make_store().await;
    s.store(&test_row(A, "A", "a", &[])).await.unwrap();
    s.store(&test_row(B, "B", "b", &[])).await.unwrap();
    // A declares B as its parent, and B independently declares A as its child,
    // producing two distinct edge rows: (A -> B, parent) and (B -> A, child).
    s.update(A, "A", &format!("[my parent](parent:{B})"), &[])
        .await
        .unwrap();
    s.update(B, "B", &format!("[my child](child:{A})"), &[])
        .await
        .unwrap();

    let from_a = s.get_edges_grouped(A).await.unwrap();
    assert_eq!(
        from_a.parents.len(),
        1,
        "expected exactly one parent entry, got {:?}",
        from_a.parents
    );
    assert_eq!(from_a.parents[0].id, B);

    let from_b = s.get_edges_grouped(B).await.unwrap();
    assert_eq!(
        from_b.children.len(),
        1,
        "expected exactly one child entry, got {:?}",
        from_b.children
    );
    assert_eq!(from_b.children[0].id, A);
}

fn test_row<'a>(
    id: &'a str,
    title: &'a str,
    content: &'a str,
    tags: &'a [String],
) -> NewMemoryRow<'a> {
    NewMemoryRow {
        id,
        title,
        content,
        tags,
        token_count: None,
        layer: "workspace",
        memory_type: "project",
    }
}

#[tokio::test]
async fn store_persists_layer_and_memory_type() {
    let (s, _dir) = make_store().await;
    s.store(&NewMemoryRow {
        id: "mem_l1",
        title: "pref",
        content: "body",
        tags: &[],
        token_count: None,
        layer: "personal",
        memory_type: "preference",
    })
    .await
    .unwrap();
    let e = s.recall_by_id("mem_l1").await.unwrap().unwrap();
    assert_eq!(e.layer, "personal");
    assert_eq!(e.memory_type, "preference");
}

#[tokio::test]
async fn store_computes_token_count_when_missing() {
    let (s, _dir) = make_store().await;
    let tags: Vec<String> = vec![];
    s.store(&test_row(
        "mem_tc",
        "title here",
        "some content words",
        &tags,
    ))
    .await
    .unwrap();
    let e = s.recall_by_id("mem_tc").await.unwrap().unwrap();
    assert!(e.token_count.unwrap() > 0);
}

#[tokio::test]
async fn update_changes_title_and_recounts_tokens() {
    let (s, _dir) = make_store().await;
    let tags: Vec<String> = vec![];
    s.store(&test_row("mem_t", "old title", "short", &tags))
        .await
        .unwrap();
    let before = s.recall_by_id("mem_t").await.unwrap().unwrap();
    let long = "much longer content ".repeat(50);
    let ok = s.update("mem_t", "new title", &long, &tags).await.unwrap();
    assert!(ok);
    let after = s.recall_by_id("mem_t").await.unwrap().unwrap();
    assert_eq!(after.title, "new title");
    assert!(after.token_count.unwrap() > before.token_count.unwrap());
}

#[test]
fn fts_quote_wraps_terms_and_escapes_quotes() {
    assert_eq!(fts_quote("project/myapp"), "\"project/myapp\"");
    assert_eq!(fts_quote("c++ tips"), "\"c++\" \"tips\"");
    assert_eq!(fts_quote("say \"hi\""), "\"say\" \"\"\"hi\"\"\"");
    assert_eq!(fts_quote("   "), "");
}

#[test]
fn parse_relationship_links_defaults_bare_link_to_sibling() {
    let id = "mem_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let content = "see [plain link](mem_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb)";
    let got = parse_relationship_links(id, content);
    assert_eq!(
        got,
        vec![(
            "plain link".to_string(),
            "sibling".to_string(),
            "mem_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()
        )]
    );
}

#[test]
fn parse_relationship_links_reads_explicit_kind_prefix() {
    let id = "mem_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let content = "\
        [the rule](parent:mem_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb) \
        [an instance](child:mem_cccccccccccccccccccccccccccccccc) \
        [a peer](sibling:mem_dddddddddddddddddddddddddddddddd)";
    let got = parse_relationship_links(id, content);
    assert_eq!(
        got,
        vec![
            (
                "the rule".to_string(),
                "parent".to_string(),
                "mem_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()
            ),
            (
                "an instance".to_string(),
                "child".to_string(),
                "mem_cccccccccccccccccccccccccccccccc".to_string()
            ),
            (
                "a peer".to_string(),
                "sibling".to_string(),
                "mem_dddddddddddddddddddddddddddddddd".to_string()
            ),
        ]
    );
}

#[test]
fn parse_relationship_links_same_target_different_kinds_both_survive() {
    let id = "mem_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let content = "\
        [as parent](parent:mem_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb) \
        [as sibling too](sibling:mem_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb)";
    let got = parse_relationship_links(id, content);
    assert_eq!(got.len(), 2);
    assert!(got.iter().any(|(_, k, _)| k == "parent"));
    assert!(got.iter().any(|(_, k, _)| k == "sibling"));
}

#[test]
fn parse_relationship_links_drops_self_link_and_unknown_kind_word() {
    let id = "mem_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let content = "\
        [self](parent:mem_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa) \
        [bogus kind](notakind:mem_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb) \
        [real one](sibling:mem_ccccccccccccccccccccccccccccccc)";
    let got = parse_relationship_links(id, content);
    // "bogus kind" fails the fixed parent|child|sibling alternation entirely,
    // so it doesn't match at all (not even as a bare/default link) — this is
    // the same "malformed mention falls through as inert text" behavior the
    // prior feature already had for e.g. a too-short mem_ id.
    assert_eq!(got.len(), 0);
}

const T_B: &str = "mem_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const T_C: &str = "mem_cccccccccccccccccccccccccccccccc";

#[tokio::test]
async fn store_creates_edges_with_parsed_kind_and_link_text() {
    let (s, _dir) = make_store().await;
    s.store(&test_row(T_B, "Target", "target body", &[]))
        .await
        .unwrap();
    let content = format!("[the rule]({T_B})");
    s.store(&test_row("mem_src", "Src", &content, &[]))
        .await
        .unwrap();

    let edges = s.list_edges(Some("mem_src")).await.unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].relationship, "sibling");
    assert_eq!(edges[0].status, "active");
    assert_eq!(edges[0].link_text.as_deref(), Some("the rule"));
}

#[tokio::test]
async fn store_creates_explicit_kind_edge() {
    let (s, _dir) = make_store().await;
    s.store(&test_row(T_B, "Target", "target body", &[]))
        .await
        .unwrap();
    let content = format!("[the rule](parent:{T_B})");
    s.store(&test_row("mem_src", "Src", &content, &[]))
        .await
        .unwrap();

    let edges = s.list_edges(Some("mem_src")).await.unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].relationship, "parent");
}

#[tokio::test]
async fn update_switching_kind_deletes_old_kind_edge_and_creates_new() {
    let (s, _dir) = make_store().await;
    s.store(&test_row(T_B, "B", "b", &[])).await.unwrap();
    s.store(&test_row(
        "mem_src",
        "Src",
        &format!("[x](parent:{T_B})"),
        &[],
    ))
    .await
    .unwrap();

    let first = s.list_edges(Some("mem_src")).await.unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].relationship, "parent");

    s.update("mem_src", "Src", &format!("[x](child:{T_B})"), &[])
        .await
        .unwrap();
    let second = s.list_edges(Some("mem_src")).await.unwrap();
    assert_eq!(
        second.len(),
        1,
        "old parent-kind edge should be gone, replaced by a child-kind one"
    );
    assert_eq!(second[0].relationship, "child");
}

#[tokio::test]
async fn update_same_target_two_kinds_keeps_both_and_only_removes_dropped_one() {
    let (s, _dir) = make_store().await;
    s.store(&test_row(T_B, "B", "b", &[])).await.unwrap();
    s.store(&test_row(
        "mem_src",
        "Src",
        &format!("[as parent](parent:{T_B}) [as sibling](sibling:{T_B})"),
        &[],
    ))
    .await
    .unwrap();
    let first = s.list_edges(Some("mem_src")).await.unwrap();
    assert_eq!(first.len(), 2);

    // Drop the sibling-kind link, keep the parent-kind one.
    s.update("mem_src", "Src", &format!("[as parent](parent:{T_B})"), &[])
        .await
        .unwrap();
    let second = s.list_edges(Some("mem_src")).await.unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].relationship, "parent");
}

#[tokio::test]
async fn update_dropping_one_of_two_targets_deletes_only_that_edge() {
    let (s, _dir) = make_store().await;
    s.store(&test_row(T_B, "B", "b", &[])).await.unwrap();
    s.store(&test_row(T_C, "C", "c", &[])).await.unwrap();
    s.store(&test_row(
        "mem_src",
        "Src",
        &format!("[a]({T_B}) [b]({T_C})"),
        &[],
    ))
    .await
    .unwrap();
    let first = s.list_edges(Some("mem_src")).await.unwrap();
    assert_eq!(first.len(), 2);

    // Rephrase to drop the link to T_C, keeping only the link to T_B.
    s.update("mem_src", "Src", &format!("[a]({T_B})"), &[])
        .await
        .unwrap();
    let second = s.list_edges(Some("mem_src")).await.unwrap();
    assert_eq!(
        second.len(),
        1,
        "the T_C edge should have been stale-deleted"
    );
    assert_eq!(second[0].target_id, T_B);
}

#[tokio::test]
async fn update_does_not_delete_manually_created_edge_with_no_content_link() {
    // Edges created via create_edge_with_status (the memory_store_edge MCP
    // tool's path) have link_text: None and no corresponding [phrase](kind:id)
    // markup in either memory's content. A later content-driven sync must not
    // treat "no content link" as "this edge should be deleted" for edges it
    // never created in the first place.
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_src", "Src", "plain body, no links", &[]))
        .await
        .unwrap();
    s.store(&test_row(T_B, "Target", "plain body, no links", &[]))
        .await
        .unwrap();
    s.create_edge("mem_src", T_B, "parent").await.unwrap();

    let before = s.list_edges(Some("mem_src")).await.unwrap();
    assert_eq!(before.len(), 1);

    // Title-only edit: content is unchanged, still has no relationship links.
    s.update("mem_src", "Src (renamed)", "plain body, no links", &[])
        .await
        .unwrap();

    let after = s.list_edges(Some("mem_src")).await.unwrap();
    assert_eq!(
        after.len(),
        1,
        "manually-created edge should survive an unrelated content-sync"
    );
}

#[tokio::test]
async fn relationship_link_to_nonexistent_target_creates_no_edge() {
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_src", "Src", &format!("[ghost]({T_B})"), &[]))
        .await
        .unwrap();
    let edges = s.list_edges(Some("mem_src")).await.unwrap();
    assert!(edges.is_empty());
}

#[tokio::test]
async fn stale_pending_mention_edge_self_heals_to_active_on_save() {
    let (s, _dir) = make_store().await;
    s.store(&test_row(T_B, "Target", "target body", &[]))
        .await
        .unwrap();
    s.store(&test_row("mem_src", "Src", "no links yet", &[]))
        .await
        .unwrap();

    // Simulate an import-created `sibling` edge stuck in 'pending' (e.g. from a
    // path that inserts relationship edges without activating them).
    s.conn
        .execute(
            "INSERT INTO edges (id, source_id, target_id, relationship, status, created_at, link_text)
             VALUES ('edge_pending_test', 'mem_src', ?1, 'sibling', 'pending', ?2, 'old text')",
            params![T_B, chrono_now()],
        )
        .await
        .unwrap();

    // Saving the source memory with a matching `[phrase](target_id)` link should
    // upsert the existing edge and flip it back to 'active'.
    let content = format!("relates to [the target]({T_B})");
    s.update("mem_src", "Src", &content, &[]).await.unwrap();

    let edges = s.list_edges(Some("mem_src")).await.unwrap();
    let m: Vec<_> = edges
        .iter()
        .filter(|e| e.relationship == "sibling")
        .collect();
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].id, "edge_pending_test");
    assert_eq!(m[0].status, "active");
    assert_eq!(m[0].link_text.as_deref(), Some("the target"));
}

#[tokio::test]
async fn search_tolerates_fts_special_characters() {
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_sp", "project/myapp", "slash content", &[]))
        .await
        .unwrap();
    // must not error, and exact-ish term still matches via quoted FTS
    let results = s.search("project/myapp", 10).await.unwrap();
    assert_eq!(results.len(), 1);
    // pure syntax garbage must not error either
    assert!(s.search("c++ ((", 10).await.unwrap().len() <= 1);
}

#[tokio::test]
async fn resolve_recall_returns_empty_for_unmatched_special_title() {
    let (s, _dir) = make_store().await;
    let r = s.resolve_recall("does/not/exist").await.unwrap();
    assert!(r.is_empty());
}

#[tokio::test]
async fn detect_conflicts_records_overwritten_local_write() {
    let (s, _dir) = make_store().await;
    let tags: Vec<String> = vec![];
    s.store(&test_row("mem_c", "C", "local version", &tags))
        .await
        .unwrap();
    let journal = s.take_journal().await.unwrap();
    assert_eq!(journal.len(), 1);
    // simulate remote frames landing during sync: content replaced out of band
    s.conn.execute(
        "UPDATE memories SET content = 'remote version', updated_at = updated_at + 10 WHERE id = 'mem_c'", (),
    ).await.unwrap();
    let found = s.detect_conflicts(&journal).await.unwrap();
    assert_eq!(found, 1);
    let conflicts = s.list_conflicts(Some("pending")).await.unwrap();
    assert_eq!(conflicts[0].remote_content, "remote version");
    assert_eq!(conflicts[0].local_content, "local version");
    assert!(
        s.take_journal().await.unwrap().is_empty(),
        "journal cleared after detection"
    );
}

#[tokio::test]
async fn detect_conflicts_is_silent_when_local_write_survived() {
    let (s, _dir) = make_store().await;
    let tags: Vec<String> = vec![];
    s.store(&test_row("mem_ok", "OK", "content", &tags))
        .await
        .unwrap();
    let journal = s.take_journal().await.unwrap();
    let found = s.detect_conflicts(&journal).await.unwrap();
    assert_eq!(found, 0);
    assert!(s.list_conflicts(Some("pending")).await.unwrap().is_empty());
}

#[tokio::test]
async fn resolve_keep_local_restores_content() {
    let (s, _dir) = make_store().await;
    let tags: Vec<String> = vec![];
    s.store(&test_row("mem_r", "R", "remote won", &tags))
        .await
        .unwrap();
    let c = s
        .write_conflict("mem_r", "remote won", "my local text", 20, 10)
        .await
        .unwrap();
    assert!(s.resolve_conflict(&c.id, "keep_local").await.unwrap());
    let mem = s.recall_by_id("mem_r").await.unwrap().unwrap();
    assert_eq!(mem.content, "my local text");
    let after = s.get_conflict_by_id(&c.id).await.unwrap().unwrap();
    assert_eq!(after.status, "keep_local");
}

#[tokio::test]
async fn meta_roundtrip() {
    let (s, _dir) = make_store().await;
    assert!(s.get_meta("last_synced_at").await.unwrap().is_none());
    s.set_meta("last_synced_at", "1234").await.unwrap();
    assert_eq!(s.get_meta("last_synced_at").await.unwrap().unwrap(), "1234");
}

#[tokio::test]
async fn max_content_tokens_defaults_when_unset() {
    let (s, _dir) = make_store().await;
    assert_eq!(s.max_content_tokens().await, DEFAULT_MAX_CONTENT_TOKENS);
}

#[tokio::test]
async fn max_content_tokens_reads_back_stored_value() {
    let (s, _dir) = make_store().await;
    s.set_meta("max_content_tokens", "500").await.unwrap();
    assert_eq!(s.max_content_tokens().await, 500);
}

#[tokio::test]
async fn max_content_tokens_falls_back_on_unparsable_value() {
    let (s, _dir) = make_store().await;
    s.set_meta("max_content_tokens", "not-a-number")
        .await
        .unwrap();
    assert_eq!(s.max_content_tokens().await, DEFAULT_MAX_CONTENT_TOKENS);
}

#[tokio::test]
async fn log_and_list_session_start_round_trip() {
    let (store, _dir) = make_store().await;
    let result = crate::session::SessionStartResult {
        project: "demo".to_string(),
        loaded: vec![],
        skipped: vec![crate::session::SkippedEntry {
            query: "missing pref".to_string(),
            reason: crate::session::SkipReason::NotFound,
        }],
        used_tokens: 120,
        max_tokens: 2000,
        memories_recalled: 0,
    };
    store
        .log_session_start("/tmp/demo-project", &result)
        .await
        .unwrap();

    let logs = store.list_session_logs(10).await.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].project_name, "demo");
    assert_eq!(logs[0].project_path, "/tmp/demo-project");
    assert_eq!(logs[0].used_tokens, 120);
    assert_eq!(logs[0].max_tokens, 2000);
    assert_eq!(logs[0].memories_recalled, 0);
    assert!(logs[0].truncated);
    assert_eq!(logs[0].skipped[0]["query"], "missing pref");
    assert_eq!(logs[0].skipped[0]["reason"], "not_found");
}

#[tokio::test]
async fn list_session_logs_orders_newest_first() {
    let (store, _dir) = make_store().await;
    for (project, used) in [("first", 10usize), ("second", 20usize)] {
        let result = crate::session::SessionStartResult {
            project: project.to_string(),
            loaded: vec![],
            skipped: vec![],
            used_tokens: used,
            max_tokens: 2000,
            memories_recalled: 0,
        };
        store.log_session_start("/tmp/p", &result).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    }
    let logs = store.list_session_logs(10).await.unwrap();
    assert_eq!(logs.len(), 2);
    assert_eq!(logs[0].project_name, "second", "newest first");
    assert_eq!(logs[1].project_name, "first");
}

/// All handlers share one libsql connection, and a libsql transaction is
/// just `BEGIN` on that connection. Without the store's write lock, two
/// tasks that both reach `BEGIN` fail with "cannot start a transaction
/// within a transaction", and a bystander's statement can be swallowed by
/// (and rolled back with) another task's transaction. Multi-threaded on
/// purpose so the writers genuinely interleave.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_writers_never_collide_on_the_shared_connection() {
    let (s, _dir) = make_store().await;
    let store = std::sync::Arc::new(s);
    let mut handles = Vec::new();
    for i in 0..40u32 {
        let store = store.clone();
        handles.push(tokio::spawn(async move {
            let id = format!("mem_{i:032x}");
            store
                .store(&test_row(&id, "t", "c", &["topic:x".into()]))
                .await?;
            store.update(&id, "t2", "c2", &[]).await?;
            store.set_meta("last_writer", &i.to_string()).await?;
            anyhow::Ok(())
        }));
    }
    for h in handles {
        h.await.unwrap().expect("no writer may fail");
    }
    assert_eq!(store.count().await.unwrap(), 40);
    for i in 0..40u32 {
        let e = store
            .recall_by_id(&format!("mem_{i:032x}"))
            .await
            .unwrap()
            .expect("every write must have landed");
        assert_eq!(e.content, "c2");
    }
}

/// `list_memories`/`search` batch their tag lookup 500 ids at a time; cross
/// the batch boundary and check the result matches the per-id path exactly.
#[tokio::test]
async fn batched_tag_loading_matches_per_memory_recall_across_batches() {
    let (s, _dir) = make_store().await;
    for i in 0..503u32 {
        let tags: Vec<String> = match i % 3 {
            0 => vec![],
            1 => vec![format!("topic:t{i}")],
            _ => vec![
                format!("topic:t{i}"),
                "lang:rust".to_string(),
                "kind:pattern".to_string(),
            ],
        };
        s.store(&test_row(&format!("mem_{i:032x}"), "t", "c", &tags))
            .await
            .unwrap();
    }
    let listed = s.list_memories(1000, 0).await.unwrap();
    assert_eq!(listed.len(), 503);
    for e in &listed {
        let single = s.recall_by_id(&e.id).await.unwrap().unwrap();
        assert_eq!(e.tags, single.tags, "tags for {}", e.id);
    }
    let hits = s.search("c", 50).await.unwrap();
    assert!(!hits.is_empty());
    for e in &hits {
        let single = s.recall_by_id(&e.id).await.unwrap().unwrap();
        assert_eq!(e.tags, single.tags);
    }
}

#[tokio::test]
async fn create_feedback_returns_none_for_unknown_memory() {
    let (s, _dir) = make_store().await;
    let result = s
        .create_feedback("mem_nope", "incorrect", None)
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn create_feedback_returns_entry_for_known_memory() {
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_1", "t", "c", &[])).await.unwrap();
    let entry = s
        .create_feedback("mem_1", "outdated", Some("stale"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entry.memory_id, "mem_1");
    assert_eq!(entry.signal, "outdated");
}

#[tokio::test]
async fn delete_all_also_clears_sync_journal_and_session_start_log() {
    let (s, _dir) = make_store().await;
    s.store(&test_row("mem_1", "t", "c", &[])).await.unwrap();
    assert!(
        !s.take_journal().await.unwrap().is_empty(),
        "store() journals the write"
    );
    s.log_session_start(
        "/proj",
        &crate::session::SessionStartResult {
            project: "p".to_string(),
            loaded: vec![],
            skipped: vec![],
            used_tokens: 0,
            max_tokens: 100,
            memories_recalled: 0,
        },
    )
    .await
    .unwrap();
    assert_eq!(s.list_session_logs(10).await.unwrap().len(), 1);

    let deleted = s.delete_all().await.unwrap();
    assert_eq!(deleted, 1);
    assert!(
        s.take_journal().await.unwrap().is_empty(),
        "journal must be cleared too"
    );
    assert!(
        s.list_session_logs(10).await.unwrap().is_empty(),
        "session_start_log must be cleared too"
    );
}
