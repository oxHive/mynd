use super::*;
use crate::{config::SyncSettings, db, store::SqliteStore};
use rmcp::model::ContentBlock;
use tempfile::TempDir;

async fn test_hivemind() -> (Mynd, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    (Mynd::new(SqliteStore::new(conn)), dir)
}

async fn test_hivemind_with_org() -> (Mynd, TempDir, TempDir) {
    let (hm, primary_dir) = test_hivemind().await;
    let org_dir = tempfile::tempdir().unwrap();
    let org_path = org_dir.path().join("org.db");
    let sync = SyncSettings::default();
    let org_database = db::open_database(&sync, org_path.to_str().unwrap())
        .await
        .unwrap();
    let org_conn = org_database.connect().unwrap();
    db::run_migrations(&org_conn).await.unwrap();
    let hm = hm.with_org_store(std::sync::Arc::new(SqliteStore::new(org_conn)));
    (hm, primary_dir, org_dir)
}

async fn seed_two(hm: &Mynd) -> (String, String) {
    for (t, c) in [("alpha", "a"), ("beta", "b")] {
        hm.do_memory_store(MemoryStoreInput {
            title: t.to_string(),
            content: c.to_string(),
            tags: vec![],
            token_count: None,
            layer: None,
            memory_type: None,
        })
        .await
        .unwrap();
    }
    let mems = hm.store.list_memories(10, 0).await.unwrap();
    let a = mems.iter().find(|m| m.title == "alpha").unwrap().id.clone();
    let b = mems.iter().find(|m| m.title == "beta").unwrap().id.clone();
    (a, b)
}

#[tokio::test]
async fn memory_store_rejects_content_over_max_content_tokens() {
    let (hm, _dir) = test_hivemind().await;
    hm.store.set_meta("max_content_tokens", "5").await.unwrap();
    let err = hm
        .do_memory_store(MemoryStoreInput {
            title: "a title with several words in it".to_string(),
            content: "this content also has plenty of words in it to push past five tokens"
                .to_string(),
            tags: vec![],
            token_count: None,
            layer: None,
            memory_type: None,
        })
        .await;
    assert!(
        err.is_err(),
        "should reject content over the configured limit"
    );
    let msg = err.unwrap_err().to_string();
    assert!(
        msg.contains("max_content_tokens"),
        "error should name the guardrail: {msg}"
    );
    assert!(
        msg.contains("child:mem_xxx"),
        "error should explain how to chunk: {msg}"
    );
}

#[tokio::test]
async fn memory_store_accepts_content_under_max_content_tokens() {
    let (hm, _dir) = test_hivemind().await;
    hm.store
        .set_meta("max_content_tokens", "500")
        .await
        .unwrap();
    let res = hm
        .do_memory_store(MemoryStoreInput {
            title: "short".to_string(),
            content: "short content".to_string(),
            tags: vec![],
            token_count: None,
            layer: None,
            memory_type: None,
        })
        .await;
    assert!(
        res.is_ok(),
        "should accept content under the configured limit"
    );
}

#[tokio::test]
async fn memory_store_with_org_layer_writes_to_org_store_not_primary() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "org secret".to_string(),
        content: "org content".to_string(),
        tags: vec![],
        token_count: None,
        layer: Some("org".to_string()),
        memory_type: None,
    })
    .await
    .unwrap();

    assert_eq!(hm.store.list_memories(10, 0).await.unwrap().len(), 0);
    let org_store = hm.org_store.as_ref().unwrap();
    let org_memories = org_store.list_memories(10, 0).await.unwrap();
    assert_eq!(org_memories.len(), 1);
    assert_eq!(org_memories[0].title, "org secret");
    assert_eq!(org_memories[0].layer, "org");
}

#[tokio::test]
async fn memory_store_with_org_layer_errors_when_no_org_store_configured() {
    let (hm, _dir) = test_hivemind().await;
    let err = hm
        .do_memory_store(MemoryStoreInput {
            title: "org secret".to_string(),
            content: "org content".to_string(),
            tags: vec![],
            token_count: None,
            layer: Some("org".to_string()),
            memory_type: None,
        })
        .await;
    assert!(err.is_err());
    assert!(
        err.unwrap_err()
            .to_string()
            .contains("org layer not configured")
    );
}

#[tokio::test]
async fn memory_recall_by_id_finds_org_memory_when_not_in_primary() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "org only".to_string(),
        content: "lives in org".to_string(),
        tags: vec![],
        token_count: None,
        layer: Some("org".to_string()),
        memory_type: None,
    })
    .await
    .unwrap();
    let org_id = hm
        .org_store
        .as_ref()
        .unwrap()
        .list_memories(10, 0)
        .await
        .unwrap()[0]
        .id
        .clone();

    let result = hm
        .do_memory_recall(MemoryRecallInput {
            id: Some(org_id),
            title: None,
        })
        .await
        .unwrap();
    let content = result.structured_content.unwrap();
    assert_eq!(content["found"], true);
    assert_eq!(content["title"], "org only");
}

#[tokio::test]
async fn memory_update_finds_and_updates_org_memory() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "org only".to_string(),
        content: "old content".to_string(),
        tags: vec![],
        token_count: None,
        layer: Some("org".to_string()),
        memory_type: None,
    })
    .await
    .unwrap();
    let org_id = hm
        .org_store
        .as_ref()
        .unwrap()
        .list_memories(10, 0)
        .await
        .unwrap()[0]
        .id
        .clone();

    let result = hm
        .do_memory_update(MemoryUpdateInput {
            id: org_id.clone(),
            title: None,
            content: Some("new content".to_string()),
            tags: None,
        })
        .await
        .unwrap();
    assert_eq!(result.structured_content.unwrap()["updated"], true);

    let updated = hm
        .org_store
        .as_ref()
        .unwrap()
        .recall_by_id(&org_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.content, "new content");
    assert_eq!(hm.store.recall_by_id(&org_id).await.unwrap(), None);
}

#[tokio::test]
async fn memory_delete_finds_and_deletes_org_memory() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "org only".to_string(),
        content: "content".to_string(),
        tags: vec![],
        token_count: None,
        layer: Some("org".to_string()),
        memory_type: None,
    })
    .await
    .unwrap();
    let org_id = hm
        .org_store
        .as_ref()
        .unwrap()
        .list_memories(10, 0)
        .await
        .unwrap()[0]
        .id
        .clone();

    let result = hm
        .do_memory_delete(MemoryDeleteInput {
            id: org_id.clone(),
            confirm: true,
        })
        .await
        .unwrap();
    assert_eq!(result.structured_content.unwrap()["deleted"], true);
    assert_eq!(
        hm.org_store
            .as_ref()
            .unwrap()
            .recall_by_id(&org_id)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn memory_recall_by_id_not_found_when_absent_from_both_stores() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    let result = hm
        .do_memory_recall(MemoryRecallInput {
            id: Some("mem_does_not_exist".to_string()),
            title: None,
        })
        .await
        .unwrap();
    assert_eq!(result.structured_content.unwrap()["found"], false);
}

#[tokio::test]
async fn memory_update_rejects_content_over_max_content_tokens() {
    let (hm, _dir) = test_hivemind().await;
    let (a, _b) = seed_two(&hm).await;
    hm.store.set_meta("max_content_tokens", "5").await.unwrap();
    let err = hm
        .do_memory_update(MemoryUpdateInput {
            id: a,
            title: None,
            content: Some(
                "this content also has plenty of words in it to push past five tokens".to_string(),
            ),
            tags: None,
        })
        .await;
    assert!(
        err.is_err(),
        "should reject an edit that pushes content over the limit"
    );
}

#[tokio::test]
async fn memory_store_edge_accepts_pending_status_and_reason() {
    let (hm, _dir) = test_hivemind().await;
    let (a, b) = seed_two(&hm).await;
    hm.do_memory_store_edge(MemoryStoreEdgeInput {
        source_id: a,
        target_id: b,
        relationship: "sibling".into(),
        status: Some("pending".into()),
        reason: Some("both about testing".into()),
    })
    .await
    .unwrap();
    let edges = hm.store.list_edges(None).await.unwrap();
    assert_eq!(edges[0].status, "pending");
    assert_eq!(edges[0].reason.as_deref(), Some("both about testing"));
}

#[tokio::test]
async fn memory_store_edge_rejects_bogus_status() {
    let (hm, _dir) = test_hivemind().await;
    let (a, b) = seed_two(&hm).await;
    let err = hm
        .do_memory_store_edge(MemoryStoreEdgeInput {
            source_id: a,
            target_id: b,
            relationship: "sibling".into(),
            status: Some("rejected".into()),
            reason: None,
        })
        .await;
    assert!(err.is_err(), "storing directly as rejected makes no sense");
}

#[tokio::test]
async fn suggest_prompt_instructs_pending_status() {
    let (hm, _dir) = test_hivemind().await;
    let (_a, _b) = seed_two(&hm).await;
    let msgs = hm.do_suggest_connections_prompt().await.unwrap();
    let text = prompt_text(&msgs[0]);
    assert!(text.contains("status: \"pending\""));
    assert!(text.contains("reason"));
}

#[tokio::test]
async fn get_info_advertises_name_and_tools_capability() {
    use rmcp::ServerHandler;
    let (hm, _dir) = test_hivemind().await;
    let info = hm.get_info();
    assert_eq!(info.server_info.name, "mynd");
    assert!(
        info.capabilities.tools.is_some(),
        "tools capability must be advertised"
    );
}

#[tokio::test]
async fn get_info_advertises_prompts_capability() {
    use rmcp::ServerHandler;
    let (hm, _dir) = test_hivemind().await;
    let info = hm.get_info();
    assert!(
        info.capabilities.prompts.is_some(),
        "prompts capability must be advertised"
    );
}

#[test]
fn list_prompts_returns_memory_list() {
    let prompts = Mynd::prompt_router().list_all();
    let names: Vec<&str> = prompts.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"memory-list"),
        "memory-list prompt must be listed"
    );
}

#[tokio::test]
async fn memory_store_tool_returns_mem_id() {
    let (hm, _dir) = test_hivemind().await;
    let result = hm
        .do_memory_store(MemoryStoreInput {
            title: "my preference".to_string(),
            content: "prefer tabs over spaces".to_string(),
            tags: vec!["style".to_string()],
            token_count: None,
            layer: None,
            memory_type: None,
        })
        .await
        .unwrap();
    let val = result.structured_content.unwrap();
    assert!(val["id"].as_str().unwrap().starts_with("mem_"));
}

#[tokio::test]
async fn memory_recall_by_id_returns_content() {
    let (hm, _dir) = test_hivemind().await;
    let stored = hm
        .do_memory_store(MemoryStoreInput {
            title: "rust style".to_string(),
            content: "use clippy, rustfmt, and deny warnings".to_string(),
            tags: vec!["rust".to_string()],
            token_count: None,
            layer: None,
            memory_type: None,
        })
        .await
        .unwrap();
    let id = stored.structured_content.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let result = hm
        .do_memory_recall(MemoryRecallInput {
            id: Some(id),
            title: None,
        })
        .await
        .unwrap();
    let val = result.structured_content.unwrap();
    assert_eq!(val["found"], true);
    assert_eq!(val["title"], "rust style");
    assert!(val["content"].as_str().unwrap().contains("clippy"));
}

#[tokio::test]
async fn memory_recall_by_title_returns_content() {
    let (hm, _dir) = test_hivemind().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "clean arch".to_string(),
        content: "domain at center, infra at edge".to_string(),
        tags: vec!["architecture".to_string()],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();

    let result = hm
        .do_memory_recall(MemoryRecallInput {
            id: None,
            title: Some("clean arch".to_string()),
        })
        .await
        .unwrap();
    let val = result.structured_content.unwrap();
    assert_eq!(val["found"], true);
    assert_eq!(val["content"], "domain at center, infra at edge");
}

#[tokio::test]
async fn memory_recall_returns_not_found_for_missing_id() {
    let (hm, _dir) = test_hivemind().await;
    let result = hm
        .do_memory_recall(MemoryRecallInput {
            id: Some("mem_doesnotexist".to_string()),
            title: None,
        })
        .await
        .unwrap();
    assert_eq!(result.structured_content.unwrap()["found"], false);
}

#[tokio::test]
async fn memory_recall_errors_without_id_or_title() {
    let (hm, _dir) = test_hivemind().await;
    let err = hm
        .do_memory_recall(MemoryRecallInput {
            id: None,
            title: None,
        })
        .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn memory_search_returns_snippets() {
    let (hm, _dir) = test_hivemind().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "db driver choice".to_string(),
        content: "we standardized on pgx v5 for postgres".to_string(),
        tags: vec!["golang".to_string(), "database".to_string()],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();

    let result = hm
        .do_memory_search(MemorySearchInput {
            query: Some("pgx".to_string()),
            tags: None,
            limit: None,
        })
        .await
        .unwrap();
    let val = result.structured_content.unwrap();
    assert_eq!(val["count"], 1);
    assert_eq!(val["results"][0]["title"], "db driver choice");
    assert!(
        val["results"][0]["snippet"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("pgx")
    );
    assert!(
        val["results"][0].get("content").is_none(),
        "search returns snippets, not full content"
    );
}

#[tokio::test]
async fn memory_search_empty_query_returns_zero() {
    let (hm, _dir) = test_hivemind().await;
    let result = hm
        .do_memory_search(MemorySearchInput {
            query: Some("  ".to_string()),
            tags: None,
            limit: None,
        })
        .await
        .unwrap();
    assert_eq!(result.structured_content.unwrap()["count"], 0);
}

#[tokio::test]
async fn memory_search_by_tags_only() {
    let (hm, _dir) = test_hivemind().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "rust preferences".to_string(),
        content: "use anyhow for errors".to_string(),
        tags: vec!["lang:rust".to_string(), "project:hivemind".to_string()],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();
    hm.do_memory_store(MemoryStoreInput {
        title: "vue preferences".to_string(),
        content: "use pinia for state".to_string(),
        tags: vec!["lang:vue".to_string(), "project:hivemind".to_string()],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();

    let result = hm
        .do_memory_search(MemorySearchInput {
            query: None,
            tags: Some(vec!["lang:rust".to_string()]),
            limit: None,
        })
        .await
        .unwrap();
    let val = result.structured_content.unwrap();
    assert_eq!(val["count"], 1);
    assert_eq!(val["results"][0]["title"], "rust preferences");
}

#[tokio::test]
async fn memory_search_query_and_tags_combined() {
    let (hm, _dir) = test_hivemind().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "rust error handling".to_string(),
        content: "use anyhow for errors".to_string(),
        tags: vec!["lang:rust".to_string()],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();
    hm.do_memory_store(MemoryStoreInput {
        title: "vue error handling".to_string(),
        content: "use error boundaries".to_string(),
        tags: vec!["lang:vue".to_string()],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();

    // "error handling" FTS-matches both, but only the rust one carries the tag.
    let result = hm
        .do_memory_search(MemorySearchInput {
            query: Some("error handling".to_string()),
            tags: Some(vec!["lang:rust".to_string()]),
            limit: None,
        })
        .await
        .unwrap();
    let val = result.structured_content.unwrap();
    assert_eq!(val["count"], 1);
    assert_eq!(val["results"][0]["title"], "rust error handling");
}

#[tokio::test]
async fn memory_update_changes_content() {
    let (hm, _dir) = test_hivemind().await;
    let stored = hm
        .do_memory_store(MemoryStoreInput {
            title: "deploy notes".to_string(),
            content: "uses docker swarm".to_string(),
            tags: vec!["devops".to_string()],
            token_count: None,
            layer: None,
            memory_type: None,
        })
        .await
        .unwrap();
    let id = stored.structured_content.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let result = hm
        .do_memory_update(MemoryUpdateInput {
            id: id.clone(),
            title: None,
            content: Some("migrated to kubernetes".to_string()),
            tags: None,
        })
        .await
        .unwrap();
    assert_eq!(result.structured_content.unwrap()["updated"], true);

    let recalled = hm
        .do_memory_recall(MemoryRecallInput {
            id: Some(id),
            title: None,
        })
        .await
        .unwrap();
    assert_eq!(
        recalled.structured_content.unwrap()["content"],
        "migrated to kubernetes"
    );
}

#[tokio::test]
async fn memory_update_returns_updated_false_for_missing() {
    let (hm, _dir) = test_hivemind().await;
    let result = hm
        .do_memory_update(MemoryUpdateInput {
            id: "mem_nope".to_string(),
            title: None,
            content: None,
            tags: None,
        })
        .await
        .unwrap();
    assert_eq!(result.structured_content.unwrap()["updated"], false);
}

/// Regression test for a TOCTOU panic: `do_memory_update` used to look up a
/// memory once via `find_owning_store` to confirm it exists, then a second
/// time to fetch it, `.expect()`-ing the second lookup to succeed. If a
/// concurrent `memory_delete` removed the row between the two lookups, the
/// `.expect()` would panic instead of returning `updated: false`. This is
/// plausible in production because `Mynd` instances share a cloned
/// `Arc<SqliteStore>` across connections (see http.rs) plus a background
/// sync loop that mutates the store concurrently.
///
/// There's no seam to pause `do_memory_update` mid-function, so the exact
/// interleaving (delete lands strictly between the two lookups) can't be
/// forced deterministically from a test without adding test-only
/// instrumentation to production code. Instead this races many concurrent
/// update/delete pairs against the same id on a real multi-threaded
/// runtime: whichever interleaving occurs, a reintroduced `.expect()`
/// panic would surface as a failed `JoinHandle::await`, so asserting both
/// tasks join cleanly is a meaningful, non-flaky check that the panic
/// surface is gone (the assertion holds regardless of whether any given
/// iteration actually lands in the narrow race window).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn memory_update_concurrent_with_delete_never_panics() {
    let (hm, _dir) = test_hivemind().await;

    for _ in 0..25 {
        let stored = hm
            .do_memory_store(MemoryStoreInput {
                title: "race".to_string(),
                content: "before".to_string(),
                tags: vec![],
                token_count: None,
                layer: None,
                memory_type: None,
            })
            .await
            .unwrap();
        let id = stored.structured_content.unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        let hm_update = hm.clone();
        let id_update = id.clone();
        let update_task = tokio::spawn(async move {
            hm_update
                .do_memory_update(MemoryUpdateInput {
                    id: id_update,
                    title: None,
                    content: Some("after".to_string()),
                    tags: None,
                })
                .await
        });

        let hm_delete = hm.clone();
        let id_delete = id.clone();
        let delete_task = tokio::spawn(async move {
            hm_delete
                .do_memory_delete(MemoryDeleteInput {
                    id: id_delete,
                    confirm: true,
                })
                .await
        });

        let (update_res, delete_res) = tokio::join!(update_task, delete_task);
        assert!(
            update_res.is_ok(),
            "do_memory_update panicked instead of returning gracefully: {:?}",
            update_res.err()
        );
        assert!(
            delete_res.is_ok(),
            "do_memory_delete panicked: {:?}",
            delete_res.err()
        );

        // Whichever order they actually landed in, both must still be
        // well-formed tool results, never a panic unwound through the task.
        update_res.unwrap().unwrap();
        delete_res.unwrap().unwrap();
    }
}

#[tokio::test]
async fn session_start_loads_configured_recalls() {
    let (hm, _dir) = test_hivemind().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "golang preferences".to_string(),
        content: "use uber/zap, sqlc, pgx v5".to_string(),
        tags: vec!["golang".to_string()],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();

    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".mynd.toml"),
        "[project]\nname=\"demo\"\n[hooks.on_session_start]\nmax_tokens=2000\nrecalls=[\"golang preferences\"]\n",
    ).unwrap();

    let result = hm
        .do_session_start(SessionStartInput {
            project_path: tmp.path().to_string_lossy().into_owned(),
        })
        .await
        .unwrap();
    let val = result.structured_content.unwrap();
    assert_eq!(val["project"], "demo");
    assert_eq!(val["context_loaded"].as_array().unwrap().len(), 1);
    assert_eq!(val["context_loaded"][0]["title"], "golang preferences");
    assert_eq!(val["budget"]["truncated"], false);
    assert!(val["budget"]["used_tokens"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn session_start_writes_a_log_entry() {
    let (hm, _dir) = test_hivemind().await;
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".mynd.toml"),
        "[project]\nname=\"demo\"\n[hooks.on_session_start]\nmax_tokens=2000\nrecalls=[]\n",
    )
    .unwrap();

    hm.do_session_start(SessionStartInput {
        project_path: tmp.path().to_string_lossy().into_owned(),
    })
    .await
    .unwrap();

    let logs = hm.store.list_session_logs(10).await.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].project_name, "demo");
    assert_eq!(logs[0].project_path, tmp.path().to_string_lossy());
}

#[tokio::test]
async fn session_start_rejects_nonexistent_path() {
    let (hm, _dir) = test_hivemind().await;
    let err = hm
        .do_session_start(SessionStartInput {
            project_path: "/no/such/dir/anywhere".to_string(),
        })
        .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn session_start_errors_without_config() {
    let (hm, _dir) = test_hivemind().await;
    let tmp = tempfile::tempdir().unwrap();
    let err = hm
        .do_session_start(SessionStartInput {
            project_path: tmp.path().to_string_lossy().into_owned(),
        })
        .await;
    assert!(err.is_err());
}

fn prompt_text(msg: &PromptMessage) -> &str {
    match &msg.content {
        ContentBlock::Text(t) => t.text.as_str(),
        _ => panic!("expected text content"),
    }
}

#[tokio::test]
async fn memory_list_prompt_returns_no_memories_message() {
    let (hm, _dir) = test_hivemind().await;
    let result = hm.do_memory_list_prompt().await.unwrap();
    assert_eq!(result.len(), 1);
    assert!(prompt_text(&result[0]).contains("No memories"));
}

#[tokio::test]
async fn memory_status_prompt_includes_count() {
    let (hm, _dir) = test_hivemind().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "test".to_string(),
        content: "c".to_string(),
        tags: vec![],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();
    let result = hm.do_memory_status_prompt().await.unwrap();
    let text = prompt_text(&result[0]);
    assert!(text.contains("1"), "status should show count of 1");
}

#[tokio::test]
async fn memory_search_prompt_returns_results() {
    let (hm, _dir) = test_hivemind().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "golang preferences".to_string(),
        content: "use uber/zap and chi router".to_string(),
        tags: vec!["golang".to_string()],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();
    let result = hm
        .do_memory_search_prompt(MemorySearchPromptInput {
            query: "uber".to_string(),
        })
        .await
        .unwrap();
    let text = prompt_text(&result[0]);
    assert!(text.contains("uber") || text.contains("golang"));
}

#[tokio::test]
async fn memory_edit_prompt_returns_formatted_content() {
    let (hm, _dir) = test_hivemind().await;
    let stored = hm
        .do_memory_store(MemoryStoreInput {
            title: "rust style".to_string(),
            content: "use clippy and rustfmt".to_string(),
            tags: vec!["rust".to_string()],
            token_count: None,
            layer: None,
            memory_type: None,
        })
        .await
        .unwrap();
    let id = stored.structured_content.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let result = hm
        .do_memory_edit_prompt(MemoryIdInput { id: id.clone() })
        .await
        .unwrap();
    let text = prompt_text(&result[0]);
    assert!(text.contains("rust style"), "should include memory title");
    assert!(text.contains("clippy"), "should include memory content");
    assert!(text.contains(&id), "should include the ID");
}

#[tokio::test]
async fn memory_edit_prompt_returns_error_for_missing_id() {
    let (hm, _dir) = test_hivemind().await;
    let result = hm
        .do_memory_edit_prompt(MemoryIdInput {
            id: "mem_nonexistent".to_string(),
        })
        .await;
    assert!(result.is_err(), "should error when memory not found");
}

#[tokio::test]
async fn memory_flag_prompt_creates_feedback_record() {
    let (hm, _dir) = test_hivemind().await;
    let stored = hm
        .do_memory_store(MemoryStoreInput {
            title: "test".to_string(),
            content: "c".to_string(),
            tags: vec![],
            token_count: None,
            layer: None,
            memory_type: None,
        })
        .await
        .unwrap();
    let id = stored.structured_content.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let result = hm
        .do_memory_flag_prompt(MemoryFlagInput {
            id: id.clone(),
            reason: "outdated".to_string(),
            note: None,
        })
        .await
        .unwrap();
    let text = prompt_text(&result[0]);
    assert!(
        text.to_lowercase().contains("flagged"),
        "should confirm the flag"
    );

    let feedback = hm.store.list_feedback(None, None).await.unwrap();
    assert_eq!(feedback.len(), 1, "feedback record should be created");
}

#[tokio::test]
async fn suggest_connections_prompt_lists_memories_and_edges() {
    let (hm, _dir) = test_hivemind().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "golang preferences".to_string(),
        content: "use uber/zap and chi router".to_string(),
        tags: vec!["golang".to_string()],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();
    hm.do_memory_store(MemoryStoreInput {
        title: "observability stack".to_string(),
        content: "prometheus, grafana, loki".to_string(),
        tags: vec!["observability".to_string()],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();
    let result = hm.do_suggest_connections_prompt().await.unwrap();
    let text = prompt_text(&result[0]);
    assert!(
        text.contains("golang preferences"),
        "should include memory titles"
    );
    assert!(
        text.contains("memory_store_edge"),
        "should instruct Claude to use the memory_store_edge tool to create edges"
    );
}

#[tokio::test]
async fn memory_store_accepts_layer_and_rejects_invalid() {
    let (hm, _dir) = test_hivemind().await;
    let ok = hm
        .do_memory_store(MemoryStoreInput {
            title: "t".into(),
            content: "c".into(),
            tags: vec![],
            token_count: None,
            layer: Some("personal".into()),
            memory_type: Some("preference".into()),
        })
        .await
        .unwrap();
    let id = ok.structured_content.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let recalled = hm
        .do_memory_recall(MemoryRecallInput {
            id: Some(id),
            title: None,
        })
        .await
        .unwrap();
    let val = recalled.structured_content.unwrap();
    assert_eq!(val["layer"], "personal");
    assert_eq!(val["memory_type"], "preference");

    let bad = hm
        .do_memory_store(MemoryStoreInput {
            title: "t".into(),
            content: "c".into(),
            tags: vec![],
            token_count: None,
            layer: Some("cosmic".into()),
            memory_type: None,
        })
        .await;
    assert!(bad.is_err());
}

#[tokio::test]
async fn review_feedback_prompt_shows_open_items() {
    let (hm, _dir) = test_hivemind().await;
    let stored = hm
        .do_memory_store(MemoryStoreInput {
            title: "old pref".to_string(),
            content: "stale content".to_string(),
            tags: vec![],
            token_count: None,
            layer: None,
            memory_type: None,
        })
        .await
        .unwrap();
    let mem_id = stored.structured_content.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    hm.store
        .create_feedback(&mem_id, "outdated", Some("This is outdated"))
        .await
        .unwrap();

    let result = hm.do_review_feedback_prompt().await.unwrap();
    let text = prompt_text(&result[0]);
    assert!(
        text.contains("outdated") || text.contains("old pref"),
        "should show feedback items"
    );
}

#[tokio::test]
async fn review_feedback_prompt_empty_when_no_open_items() {
    let (hm, _dir) = test_hivemind().await;
    let result = hm.do_review_feedback_prompt().await.unwrap();
    let text = prompt_text(&result[0]);
    assert!(
        text.to_lowercase().contains("no open"),
        "should indicate no items"
    );
}

#[tokio::test]
async fn with_store_constructor() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    let store = Arc::new(SqliteStore::new(conn));
    let hm = Mynd::with_store(Arc::clone(&store));
    assert!(hm.sync_trigger.is_none());
}

#[tokio::test]
async fn with_sync_constructor_stores_trigger() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    let store = Arc::new(SqliteStore::new(conn));
    let trigger = Arc::new(tokio::sync::Notify::new());
    let hm = Mynd::with_sync(store, trigger);
    assert!(hm.sync_trigger.is_some());
}

#[tokio::test]
async fn memory_store_notifies_sync_trigger() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    let store = Arc::new(SqliteStore::new(conn));
    let trigger = Arc::new(tokio::sync::Notify::new());
    let hm = Mynd::with_sync(store, Arc::clone(&trigger));

    let notified = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let notified2 = Arc::clone(&notified);
    let trigger2 = Arc::clone(&trigger);
    tokio::spawn(async move {
        trigger2.notified().await;
        notified2.store(true, std::sync::atomic::Ordering::Relaxed);
    });

    hm.do_memory_store(MemoryStoreInput {
        title: "t".to_string(),
        content: "c".to_string(),
        tags: vec![],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    assert!(notified.load(std::sync::atomic::Ordering::Relaxed));
}

#[tokio::test]
async fn suggest_connections_empty_store_returns_message() {
    let (hm, _dir) = test_hivemind().await;
    let result = hm.do_suggest_connections_prompt().await.unwrap();
    assert_eq!(result.len(), 1);
    let text = prompt_text(&result[0]);
    assert!(
        text.contains("No memories"),
        "empty store should say no memories"
    );
}

#[tokio::test]
async fn session_start_rejects_file_path() {
    let (hm, _dir) = test_hivemind().await;
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("somefile.txt");
    std::fs::write(&file_path, "hello").unwrap();
    let err = hm
        .do_session_start(SessionStartInput {
            project_path: file_path.to_string_lossy().into_owned(),
        })
        .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn memory_list_prompt_with_memories_shows_titles() {
    let (hm, _dir) = test_hivemind().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "my preference".to_string(),
        content: "use tabs".to_string(),
        tags: vec!["style".to_string()],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();
    let result = hm.do_memory_list_prompt().await.unwrap();
    let text = prompt_text(&result[0]);
    assert!(text.contains("my preference"));
    assert!(text.contains("style"));
}

#[tokio::test]
async fn memory_update_preserves_tags_when_not_specified() {
    let (hm, _dir) = test_hivemind().await;
    let stored = hm
        .do_memory_store(MemoryStoreInput {
            title: "tagged".to_string(),
            content: "original".to_string(),
            tags: vec!["keep".to_string()],
            token_count: None,
            layer: None,
            memory_type: None,
        })
        .await
        .unwrap();
    let id = stored.structured_content.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    hm.do_memory_update(MemoryUpdateInput {
        id: id.clone(),
        title: None,
        content: Some("updated".to_string()),
        tags: None,
    })
    .await
    .unwrap();

    let recalled = hm
        .do_memory_recall(MemoryRecallInput {
            id: Some(id),
            title: None,
        })
        .await
        .unwrap();
    let val = recalled.structured_content.unwrap();
    assert_eq!(val["content"], "updated");
    let tags: Vec<_> = val["tags"].as_array().unwrap().iter().collect();
    assert!(tags.iter().any(|t| t.as_str() == Some("keep")));
}

#[test]
fn all_seven_prompts_are_registered() {
    let prompts = Mynd::prompt_router().list_all();
    let names: Vec<&str> = prompts.iter().map(|p| p.name.as_str()).collect();
    let expected = [
        "memory-list",
        "memory-status",
        "memory-search",
        "memory-edit",
        "memory-flag",
        "suggest-connections",
        "review-feedback",
    ];
    for name in &expected {
        assert!(
            names.contains(name),
            "prompt {name} must be registered; got: {names:?}"
        );
    }
    assert_eq!(prompts.len(), 7, "exactly 7 prompts expected");
}

#[tokio::test]
async fn memory_get_edges_returns_grouped_connections() {
    let (hm, _dir) = test_hivemind().await;

    let parent = hm
        .do_memory_store(MemoryStoreInput {
            title: "Parent".to_string(),
            content: "parent body".to_string(),
            tags: vec![],
            token_count: None,
            layer: None,
            memory_type: None,
        })
        .await
        .unwrap();
    let parent_id = parent.structured_content.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let child = hm
        .do_memory_store(MemoryStoreInput {
            title: "Child".to_string(),
            content: "child body".to_string(),
            tags: vec![],
            token_count: None,
            layer: None,
            memory_type: None,
        })
        .await
        .unwrap();
    let child_id = child.structured_content.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Child asserts Parent is its parent.
    hm.do_memory_update(MemoryUpdateInput {
        id: child_id.clone(),
        title: None,
        content: Some(format!("[the rule](parent:{parent_id})")),
        tags: None,
    })
    .await
    .unwrap();

    let from_child = hm
        .memory_get_edges(Parameters(MemoryGetEdgesInput {
            memory_id: child_id.clone(),
        }))
        .await
        .unwrap();
    let from_child = from_child.structured_content.unwrap();
    assert_eq!(from_child["parents"][0]["id"], parent_id);
    assert_eq!(from_child["parents"][0]["link_text"], "the rule");
    assert!(from_child["children"].as_array().unwrap().is_empty());

    let from_parent = hm
        .memory_get_edges(Parameters(MemoryGetEdgesInput {
            memory_id: parent_id.clone(),
        }))
        .await
        .unwrap();
    let from_parent = from_parent.structured_content.unwrap();
    assert_eq!(from_parent["children"][0]["id"], child_id);
    assert!(from_parent["parents"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn memory_delete_requires_confirm() {
    let (hm, _dir) = test_hivemind().await;
    let stored = hm
        .do_memory_store(MemoryStoreInput {
            title: "temp".to_string(),
            content: "delete me".to_string(),
            tags: vec!["tmp".to_string()],
            token_count: None,
            layer: None,
            memory_type: None,
        })
        .await
        .unwrap();
    let id = stored.structured_content.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let err = hm
        .do_memory_delete(MemoryDeleteInput {
            id: id.clone(),
            confirm: false,
        })
        .await;
    assert!(err.is_err());
    assert!(
        hm.do_memory_recall(MemoryRecallInput {
            id: Some(id.clone()),
            title: None
        })
        .await
        .unwrap()
        .structured_content
        .unwrap()["found"]
            == true
    );

    let ok = hm
        .do_memory_delete(MemoryDeleteInput {
            id: id.clone(),
            confirm: true,
        })
        .await
        .unwrap();
    assert_eq!(ok.structured_content.unwrap()["deleted"], true);
    assert_eq!(
        hm.do_memory_recall(MemoryRecallInput {
            id: Some(id),
            title: None
        })
        .await
        .unwrap()
        .structured_content
        .unwrap()["found"],
        false
    );
}

#[tokio::test]
async fn memory_update_edge_patches_pending_edge() {
    let (hm, _dir) = test_hivemind().await;
    let (a, b) = seed_two(&hm).await;
    hm.do_memory_store_edge(MemoryStoreEdgeInput {
        source_id: a,
        target_id: b,
        relationship: "sibling".into(),
        status: Some("pending".into()),
        reason: Some("first take".into()),
    })
    .await
    .unwrap();
    let edge_id = hm.store.list_edges(None).await.unwrap()[0].id.clone();

    hm.do_memory_update_edge(MemoryUpdateEdgeInput {
        id: edge_id.clone(),
        relationship: Some("parent".into()),
        reason: Some("a is the general rule".into()),
        link_text: None,
    })
    .await
    .unwrap();

    let e = hm.store.get_edge(&edge_id).await.unwrap().unwrap();
    assert_eq!(e.relationship, "parent");
    assert_eq!(e.reason.as_deref(), Some("a is the general rule"));
    assert_eq!(e.status, "pending");
}

#[tokio::test]
async fn memory_update_edge_missing_id_reports_updated_false() {
    let (hm, _dir) = test_hivemind().await;
    let res = hm
        .do_memory_update_edge(MemoryUpdateEdgeInput {
            id: "edge_missing".into(),
            relationship: None,
            reason: Some("x".into()),
            link_text: None,
        })
        .await
        .unwrap();
    let v = res.structured_content.unwrap();
    assert_eq!(v["updated"], false);
}

#[tokio::test]
async fn memory_update_edge_invalid_relationship_errors() {
    let (hm, _dir) = test_hivemind().await;
    let (a, b) = seed_two(&hm).await;
    hm.do_memory_store_edge(MemoryStoreEdgeInput {
        source_id: a,
        target_id: b,
        relationship: "sibling".into(),
        status: Some("pending".into()),
        reason: None,
    })
    .await
    .unwrap();
    let edge_id = hm.store.list_edges(None).await.unwrap()[0].id.clone();
    let res = hm
        .do_memory_update_edge(MemoryUpdateEdgeInput {
            id: edge_id,
            relationship: Some("related_to".into()),
            reason: None,
            link_text: None,
        })
        .await;
    assert!(res.is_err());
}

#[tokio::test]
async fn store_edge_between_two_org_memories_succeeds() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    for (t, c) in [("org a", "a"), ("org b", "b")] {
        hm.do_memory_store(MemoryStoreInput {
            title: t.to_string(),
            content: c.to_string(),
            tags: vec![],
            token_count: None,
            layer: Some("org".to_string()),
            memory_type: None,
        })
        .await
        .unwrap();
    }
    let org_store = hm.org_store.as_ref().unwrap();
    let mems = org_store.list_memories(10, 0).await.unwrap();
    let a = mems.iter().find(|m| m.title == "org a").unwrap().id.clone();
    let b = mems.iter().find(|m| m.title == "org b").unwrap().id.clone();

    let result = hm
        .do_memory_store_edge(MemoryStoreEdgeInput {
            source_id: a,
            target_id: b,
            relationship: "sibling".to_string(),
            status: None,
            reason: None,
        })
        .await
        .unwrap();
    assert_eq!(result.structured_content.unwrap()["created"], true);
}

#[tokio::test]
async fn store_edge_across_primary_and_org_fails_with_missing_endpoint() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "workspace mem".to_string(),
        content: "a".to_string(),
        tags: vec![],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();
    hm.do_memory_store(MemoryStoreInput {
        title: "org mem".to_string(),
        content: "b".to_string(),
        tags: vec![],
        token_count: None,
        layer: Some("org".to_string()),
        memory_type: None,
    })
    .await
    .unwrap();
    let primary_id = hm.store.list_memories(10, 0).await.unwrap()[0].id.clone();
    let org_id = hm
        .org_store
        .as_ref()
        .unwrap()
        .list_memories(10, 0)
        .await
        .unwrap()[0]
        .id
        .clone();

    let result = hm
        .do_memory_store_edge(MemoryStoreEdgeInput {
            source_id: primary_id,
            target_id: org_id,
            relationship: "sibling".to_string(),
            status: None,
            reason: None,
        })
        .await;
    assert!(result.is_err(), "cross-layer edges must be rejected");
}

// ── Final-review fix regression tests ────────────────────────────────────
//
// Fix 2: org-store errors during ID-addressed lookups (find_owning_store)
// and edge retries must degrade gracefully, exactly as if org weren't
// configured — but a broken PRIMARY store must still surface as a real
// error. These tests simulate a genuinely broken org connection by
// dropping the `memories` table it depends on (mirrors the existing
// `s.conn.execute(...)` pattern in store_tests.rs), rather than merely
// inspecting the code path.

#[tokio::test]
async fn find_owning_store_degrades_when_org_connection_is_broken() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    hm.org_store
        .as_ref()
        .unwrap()
        .conn
        .execute("DROP TABLE memories", ())
        .await
        .unwrap();

    // Id isn't in primary; org lookup now errors internally on every query
    // — must degrade to "not found" rather than propagating the org's SQL
    // error as an internal_error.
    let result = hm
        .do_memory_recall(MemoryRecallInput {
            id: Some("mem_does_not_exist".to_string()),
            title: None,
        })
        .await;
    assert!(
        result.is_ok(),
        "a broken org connection must degrade to not-found, not error: {result:?}"
    );
    assert_eq!(result.unwrap().structured_content.unwrap()["found"], false);
}

#[tokio::test]
async fn find_owning_store_still_propagates_primary_errors() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    hm.store
        .conn
        .execute("DROP TABLE memories", ())
        .await
        .unwrap();

    let result = hm
        .do_memory_recall(MemoryRecallInput {
            id: Some("mem_does_not_exist".to_string()),
            title: None,
        })
        .await;
    assert!(
        result.is_err(),
        "a broken PRIMARY connection must still surface as a real error, not degrade"
    );
}

#[tokio::test]
async fn store_edge_falls_back_to_primary_missing_endpoint_when_org_connection_broken() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    hm.org_store
        .as_ref()
        .unwrap()
        .conn
        .execute("DROP TABLE memories", ())
        .await
        .unwrap();

    // Neither id exists anywhere, so primary returns MissingEndpoint and the
    // org retry fires (broken connection) — must fall back to primary's
    // ordinary MissingEndpoint error, not surface the org's raw SQL error.
    let result = hm
        .do_memory_store_edge(MemoryStoreEdgeInput {
            source_id: "mem_nope_a".to_string(),
            target_id: "mem_nope_b".to_string(),
            relationship: "sibling".to_string(),
            status: None,
            reason: None,
        })
        .await;
    assert!(result.is_err());
    let msg = result.unwrap_err().message.to_string();
    assert!(
        msg.contains("must both be existing"),
        "should be the ordinary MissingEndpoint message, not a raw org SQL error: {msg}"
    );
}

#[tokio::test]
async fn update_edge_falls_back_to_primary_result_when_org_connection_broken() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    hm.org_store
        .as_ref()
        .unwrap()
        .conn
        .execute("DROP TABLE memories", ())
        .await
        .unwrap();

    let result = hm
        .do_memory_update_edge(MemoryUpdateEdgeInput {
            id: "edge_missing".to_string(),
            relationship: None,
            reason: Some("x".to_string()),
            link_text: None,
        })
        .await
        .unwrap();
    assert_eq!(
        result.structured_content.unwrap()["updated"],
        false,
        "should degrade to primary's 'not found' result rather than surfacing the org SQL error"
    );
}

// Fix 3: org memories reachable by title, search top-up, and prompt merges.

#[tokio::test]
async fn memory_recall_by_title_finds_org_memory_when_not_in_primary() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "org title only".to_string(),
        content: "lives in org".to_string(),
        tags: vec![],
        token_count: None,
        layer: Some("org".to_string()),
        memory_type: None,
    })
    .await
    .unwrap();

    let result = hm
        .do_memory_recall(MemoryRecallInput {
            id: None,
            title: Some("org title only".to_string()),
        })
        .await
        .unwrap();
    let val = result.structured_content.unwrap();
    assert_eq!(val["found"], true);
    assert_eq!(val["content"], "lives in org");
}

#[tokio::test]
async fn memory_search_fills_remaining_limit_from_org_store() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "widget primary".to_string(),
        content: "primary widget notes".to_string(),
        tags: vec![],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();
    hm.do_memory_store(MemoryStoreInput {
        title: "widget org".to_string(),
        content: "org widget notes".to_string(),
        tags: vec![],
        token_count: None,
        layer: Some("org".to_string()),
        memory_type: None,
    })
    .await
    .unwrap();

    let result = hm
        .do_memory_search(MemorySearchInput {
            query: Some("widget".to_string()),
            tags: None,
            limit: Some(5),
        })
        .await
        .unwrap();
    let val = result.structured_content.unwrap();
    assert_eq!(val["count"], 2, "primary result plus org top-up");
    let titles: Vec<_> = val["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["title"].as_str().unwrap())
        .collect();
    assert!(titles.contains(&"widget primary"));
    assert!(titles.contains(&"widget org"));
}

#[tokio::test]
async fn memory_search_does_not_need_org_when_primary_already_fills_limit() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    for i in 0..3 {
        hm.do_memory_store(MemoryStoreInput {
            title: format!("gadget {i}"),
            content: "gadget content".to_string(),
            tags: vec![],
            token_count: None,
            layer: None,
            memory_type: None,
        })
        .await
        .unwrap();
    }
    hm.do_memory_store(MemoryStoreInput {
        title: "gadget org".to_string(),
        content: "gadget content org".to_string(),
        tags: vec![],
        token_count: None,
        layer: Some("org".to_string()),
        memory_type: None,
    })
    .await
    .unwrap();

    let result = hm
        .do_memory_search(MemorySearchInput {
            query: Some("gadget".to_string()),
            tags: None,
            limit: Some(3),
        })
        .await
        .unwrap();
    let val = result.structured_content.unwrap();
    assert_eq!(
        val["count"], 3,
        "primary alone fills the limit; org results must not be appended"
    );
    let titles: Vec<_> = val["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["title"].as_str().unwrap().to_string())
        .collect();
    assert!(!titles.contains(&"gadget org".to_string()));
}

#[tokio::test]
async fn memory_search_prompt_fills_remaining_from_org_store() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "sprocket primary".to_string(),
        content: "primary sprocket notes".to_string(),
        tags: vec![],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();
    hm.do_memory_store(MemoryStoreInput {
        title: "sprocket org".to_string(),
        content: "org sprocket notes".to_string(),
        tags: vec![],
        token_count: None,
        layer: Some("org".to_string()),
        memory_type: None,
    })
    .await
    .unwrap();

    let result = hm
        .do_memory_search_prompt(MemorySearchPromptInput {
            query: "sprocket".to_string(),
        })
        .await
        .unwrap();
    let text = prompt_text(&result[0]);
    assert!(text.contains("sprocket primary"));
    assert!(text.contains("sprocket org"));
}

#[tokio::test]
async fn memory_list_prompt_reflects_combined_counts_and_marks_org_entries() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "primary mem".to_string(),
        content: "c".to_string(),
        tags: vec![],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();
    hm.do_memory_store(MemoryStoreInput {
        title: "org mem".to_string(),
        content: "c".to_string(),
        tags: vec![],
        token_count: None,
        layer: Some("org".to_string()),
        memory_type: None,
    })
    .await
    .unwrap();

    let result = hm.do_memory_list_prompt().await.unwrap();
    let text = prompt_text(&result[0]);
    assert!(
        text.contains("(2 memories):"),
        "count should reflect both stores: {text}"
    );
    assert!(text.contains("primary mem"));
    assert!(
        text.contains("org mem [org]"),
        "org-sourced entries should be marked [org]: {text}"
    );
}

#[tokio::test]
async fn memory_status_prompt_reflects_combined_counts_and_marks_org_entries() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    hm.do_memory_store(MemoryStoreInput {
        title: "primary mem".to_string(),
        content: "c".to_string(),
        tags: vec![],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();
    hm.do_memory_store(MemoryStoreInput {
        title: "org mem".to_string(),
        content: "c".to_string(),
        tags: vec![],
        token_count: None,
        layer: Some("org".to_string()),
        memory_type: None,
    })
    .await
    .unwrap();

    let result = hm.do_memory_status_prompt().await.unwrap();
    let text = prompt_text(&result[0]);
    assert!(
        text.contains("Total memories: 2"),
        "should combine primary + org counts: {text}"
    );
    assert!(
        text.contains("org mem [org]"),
        "org-sourced recent entries should be marked: {text}"
    );
}

// Fix 4: org-layer writes wake org_sync_trigger, not sync_trigger, and
// vice versa.

#[tokio::test]
async fn memory_store_layer_routes_trigger_notifications() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    let store = Arc::new(SqliteStore::new(conn));
    let trigger = Arc::new(tokio::sync::Notify::new());

    let org_dir = tempfile::tempdir().unwrap();
    let org_path = org_dir.path().join("org.db");
    let org_database = db::open_database(&sync, org_path.to_str().unwrap())
        .await
        .unwrap();
    let org_conn = org_database.connect().unwrap();
    db::run_migrations(&org_conn).await.unwrap();
    let org_store = Arc::new(SqliteStore::new(org_conn));
    let org_trigger = Arc::new(tokio::sync::Notify::new());

    let hm = Mynd::with_sync(store, Arc::clone(&trigger))
        .with_org_store(org_store)
        .with_org_sync_trigger(Arc::clone(&org_trigger));

    // Checks whether `n` has a pending notification, without leaving a
    // dangling waiter behind on a negative result: `tokio::time::timeout`
    // drops the `notified()` future on elapse, which cleanly deregisters it
    // from `Notify`'s waiter list. A dangling waiter would otherwise steal
    // the *next* notification meant for a later check in this test.
    async fn was_notified(n: &Arc<tokio::sync::Notify>) -> bool {
        tokio::time::timeout(std::time::Duration::from_millis(20), n.notified())
            .await
            .is_ok()
    }

    hm.do_memory_store(MemoryStoreInput {
        title: "org write".to_string(),
        content: "c".to_string(),
        tags: vec![],
        token_count: None,
        layer: Some("org".to_string()),
        memory_type: None,
    })
    .await
    .unwrap();
    assert!(
        was_notified(&org_trigger).await,
        "org write should notify org_sync_trigger"
    );
    assert!(
        !was_notified(&trigger).await,
        "org write should NOT notify the primary sync_trigger"
    );

    hm.do_memory_store(MemoryStoreInput {
        title: "workspace write".to_string(),
        content: "c".to_string(),
        tags: vec![],
        token_count: None,
        layer: None,
        memory_type: None,
    })
    .await
    .unwrap();
    assert!(
        was_notified(&trigger).await,
        "workspace write should notify the primary sync_trigger"
    );
    assert!(
        !was_notified(&org_trigger).await,
        "workspace write should NOT notify org_sync_trigger"
    );
}

// Fix 6: org writes/updates are size-checked against the ORG store's own
// max_content_tokens, not the primary store's.

#[tokio::test]
async fn org_write_checked_against_orgs_own_max_content_tokens_not_primary() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    // Primary has a tiny limit; org keeps the (much larger) default.
    hm.store.set_meta("max_content_tokens", "1").await.unwrap();
    let content = "this content has plenty of words in it to push well past one token";
    let res = hm
        .do_memory_store(MemoryStoreInput {
            title: "org content".to_string(),
            content: content.to_string(),
            tags: vec![],
            token_count: None,
            layer: Some("org".to_string()),
            memory_type: None,
        })
        .await;
    assert!(
        res.is_ok(),
        "org write should be checked against the org store's own limit, not the tiny primary limit: {res:?}"
    );
}

#[tokio::test]
async fn org_write_rejected_using_orgs_own_max_content_tokens() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    hm.org_store
        .as_ref()
        .unwrap()
        .set_meta("max_content_tokens", "5")
        .await
        .unwrap();
    let err = hm
        .do_memory_store(MemoryStoreInput {
            title: "a title with several words in it".to_string(),
            content: "this content also has plenty of words in it to push past five tokens"
                .to_string(),
            tags: vec![],
            token_count: None,
            layer: Some("org".to_string()),
            memory_type: None,
        })
        .await;
    assert!(
        err.is_err(),
        "org write should be rejected using org's own configured limit"
    );
}

#[tokio::test]
async fn org_update_checked_against_orgs_own_max_content_tokens() {
    let (hm, _primary_dir, _org_dir) = test_hivemind_with_org().await;
    let stored = hm
        .do_memory_store(MemoryStoreInput {
            title: "org mem".to_string(),
            content: "short".to_string(),
            tags: vec![],
            token_count: None,
            layer: Some("org".to_string()),
            memory_type: None,
        })
        .await
        .unwrap();
    let id = stored.structured_content.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    hm.org_store
        .as_ref()
        .unwrap()
        .set_meta("max_content_tokens", "5")
        .await
        .unwrap();
    let err = hm
        .do_memory_update(MemoryUpdateInput {
            id,
            title: None,
            content: Some(
                "this content also has plenty of words in it to push past five tokens".to_string(),
            ),
            tags: None,
        })
        .await;
    assert!(
        err.is_err(),
        "org update should reject content exceeding org's own configured limit"
    );
}
