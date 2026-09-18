use super::*;
use crate::{config::SyncSettings, db, store::SqliteStore};
use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;

async fn test_store() -> (Arc<SqliteStore>, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    (Arc::new(SqliteStore::new(conn)), dir)
}

/// Writes a stub agent script that mirrors suggest_session's test stub:
/// it echoes a fake `claude -p --output-format json` result so a real
/// process gets spawned but nothing calls out to a real agent.
fn write_stub_agent(dir: &std::path::Path) -> String {
    let script = dir.join("stub-agent.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\necho '{\"type\":\"result\",\"session_id\":\"stub-1\",\"result\":\"done\",\"is_error\":false}'\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script.to_string_lossy().into_owned()
}

fn test_suggest_manager(
    store: Arc<SqliteStore>,
    dir: &std::path::Path,
    events: Events,
) -> Arc<crate::suggest_session::SuggestSessionManager> {
    let script = write_stub_agent(dir);
    let agent = crate::config::AgentSettings {
        command: script,
        args: vec![],
        kind: crate::config::AgentKind::Claude,
    };
    crate::suggest_session::SuggestSessionManager::new(
        store,
        events,
        agent,
        "http://127.0.0.1:3456/mcp".into(),
    )
}

fn test_update_state() -> SharedUpdateState {
    Arc::new(tokio::sync::RwLock::new(
        crate::update::UpdateState::new_idle(),
    ))
}

fn test_agent_settings() -> crate::config::AgentSettings {
    crate::config::AgentSettings::default()
}

async fn test_router() -> (Router, TempDir) {
    test_router_with_guard(true).await
}

async fn test_router_with_guard(guard_predefined_namespaces: bool) -> (Router, TempDir) {
    let (store, dir) = test_store().await;
    let (events, _) = broadcast::channel(16);
    let suggest = test_suggest_manager(Arc::clone(&store), dir.path(), events.clone());
    let r = router(
        store,
        None,
        SyncSettings::default(),
        None,
        "http://127.0.0.1:3457",
        events,
        suggest,
        test_update_state(),
        test_agent_settings(),
        guard_predefined_namespaces,
    );
    (r, dir)
}

async fn test_router_with_events() -> (Router, broadcast::Receiver<Value>, TempDir) {
    let (store, dir) = test_store().await;
    let (events, rx) = broadcast::channel(16);
    let suggest = test_suggest_manager(Arc::clone(&store), dir.path(), events.clone());
    let r = router(
        store,
        None,
        SyncSettings::default(),
        None,
        "http://127.0.0.1:3457",
        events,
        suggest,
        test_update_state(),
        test_agent_settings(),
        true,
    );
    (r, rx, dir)
}

async fn test_router_with_store() -> (Router, Arc<SqliteStore>, TempDir) {
    let (store, dir) = test_store().await;
    let (events, _) = broadcast::channel(16);
    let suggest = test_suggest_manager(Arc::clone(&store), dir.path(), events.clone());
    let r = router(
        Arc::clone(&store),
        None,
        SyncSettings::default(),
        None,
        "http://127.0.0.1:3457",
        events,
        suggest,
        test_update_state(),
        test_agent_settings(),
        true,
    );
    (r, store, dir)
}

async fn test_router_with_org() -> (Router, TempDir, TempDir) {
    let (store, dir) = test_store().await;
    let (org_store, org_dir) = test_store().await;
    let (events, _) = broadcast::channel(16);
    let suggest = test_suggest_manager(Arc::clone(&store), dir.path(), events.clone());
    let r = router(
        store,
        Some(org_store),
        SyncSettings::default(),
        Some(SyncSettings {
            enabled: true,
            remote_url: "https://gateway.example/org".into(),
            api_key: "hm_org_x".into(),
            interval_seconds: 60,
            sync_on_store: true,
            sync_on_startup: true,
        }),
        "http://127.0.0.1:3457",
        events,
        suggest,
        test_update_state(),
        test_agent_settings(),
        true,
    );
    (r, dir, org_dir)
}

async fn req(app: Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(v) => builder
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let resp = app.oneshot(request).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let val = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, val)
}

fn memory_body(title: &str, content: &str, tags: &[&str]) -> Value {
    json!({ "title": title, "content": content, "tags": tags })
}

#[tokio::test]
async fn memories_crud_roundtrip() {
    let (app, _dir) = test_router().await;
    let (st, created) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body(
            "golang preferences",
            "uber/zap, sqlc, pgx v5",
            &["golang"],
        )),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("mem_"));

    let (st, list) = req(app.clone(), "GET", "/api/v1/memories", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(list["count"], 1);
    assert_eq!(list["memories"][0]["title"], "golang preferences");

    let (st, one) = req(app.clone(), "GET", &format!("/api/v1/memories/{id}"), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(one["content"], "uber/zap, sqlc, pgx v5");

    let (st, patched) = req(
        app.clone(),
        "PATCH",
        &format!("/api/v1/memories/{id}"),
        Some(json!({ "content": "now pgx v6" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(patched["content"], "now pgx v6");

    let (st, _) = req(
        app.clone(),
        "DELETE",
        &format!("/api/v1/memories/{id}"),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = req(app.clone(), "GET", &format!("/api/v1/memories/{id}"), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn patch_memory_updates_title_and_returns_full_entry() {
    let (app, _dir) = test_router().await;
    let (_, created) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("old", "content", &[])),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    let (st, body) = req(
        app.clone(),
        "PATCH",
        &format!("/api/v1/memories/{id}"),
        Some(json!({ "title": "renamed" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["title"], "renamed");
    assert_eq!(body["content"], "content");
    assert!(body["updated_at"].is_i64());
}

#[tokio::test]
async fn add_memory_tags_merges_into_existing() {
    let (app, _dir) = test_router().await;
    let (_, created) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("T", "C", &["a"])),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    let (status, body) = req(
        app.clone(),
        "POST",
        &format!("/api/v1/memories/{id}/tags/add"),
        Some(json!({ "tags": ["b"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut tags: Vec<String> = body["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    tags.sort();
    assert_eq!(tags, vec!["a".to_string(), "b".to_string()]);
}

#[tokio::test]
async fn remove_memory_tags_drops_named_tags() {
    let (app, _dir) = test_router().await;
    let (_, created) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("T", "C", &["a", "b"])),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    let (status, body) = req(
        app.clone(),
        "POST",
        &format!("/api/v1/memories/{id}/tags/remove"),
        Some(json!({ "tags": ["a"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["tags"], json!(["b"]));
}

#[tokio::test]
async fn add_memory_tags_404s_for_missing_memory() {
    let (app, _dir) = test_router().await;
    let (status, _) = req(
        app,
        "POST",
        "/api/v1/memories/mem_missing/tags/add",
        Some(json!({ "tags": ["a"] })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn search_dispatches_tag_expression() {
    let (app, _dir) = test_router().await;
    req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("Findable", "content", &["lang:rust"])),
    )
    .await;
    let (status, body) = req(app, "GET", "/api/v1/search?q=tag%3Alang%3Arust", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 1);
    assert_eq!(body["results"][0]["title"], "Findable");
}

#[tokio::test]
async fn status_reports_version_and_count() {
    let (app, _dir) = test_router().await;
    req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("m", "x", &[])),
    )
    .await;
    let (st, status) = req(app.clone(), "GET", "/api/v1/status", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(status["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(status["memory_count"], 1);
    assert_eq!(status["sync"]["enabled"], false);
    assert_eq!(status["agent"]["kind"], "claude");
    assert_eq!(status["agent"]["command"], "claude");
}

#[tokio::test]
async fn status_reports_org_not_configured_when_absent() {
    let (app, _dir) = test_router().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["org"]["configured"], false);
    assert!(json["org"].get("last_synced_at").is_none() || json["org"]["last_synced_at"].is_null());
}

#[tokio::test]
async fn status_reports_org_configured_and_counts_when_present() {
    let (app, _dir, _org_dir) = test_router_with_org().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["org"]["configured"], true);
    assert_eq!(json["org"]["enabled"], true);
    assert_eq!(json["org"]["conflict_count"], 0);
    assert_eq!(json["org"]["count"], 0);
}

#[tokio::test]
async fn status_degrades_gracefully_when_org_store_is_broken() {
    let (app, _dir, org_dir) = test_router_with_org().await;
    // Break the org db out from under the router's already-open connection
    // by dropping tables that server_status's org block queries depend on.
    let org_path = org_dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, org_path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    conn.execute_batch("DROP TABLE _meta; DROP TABLE conflicts; DROP TABLE memories;")
        .await
        .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["org"]["configured"], true);
    assert_eq!(json["org"]["conflict_count"], 0);
    assert_eq!(json["org"]["count"], 0);
}

#[tokio::test]
async fn settings_sync_returns_defaults() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(app, "GET", "/api/v1/settings/sync", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], false);
    assert_eq!(body["interval_seconds"], 300);
}

#[tokio::test]
async fn sync_settings_reports_org_sync_null_when_absent() {
    let (app, _dir) = test_router().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/settings/sync")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert!(json["org_sync"].is_null());
}

#[tokio::test]
async fn sync_settings_reports_org_sync_fields_when_present() {
    let (app, _dir, _org_dir) = test_router_with_org().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/settings/sync")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["org_sync"]["enabled"], true);
    assert_eq!(
        json["org_sync"]["remote_url"],
        "https://gateway.example/org"
    );
}

#[tokio::test]
async fn delete_memory_returns_404_when_not_found() {
    let (app, _dir) = test_router().await;
    let (status, _) = req(app, "DELETE", "/api/v1/memories/mem_nonexistent", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_memory_falls_back_to_org_store() {
    let (app, _dir, org_dir) = test_router_with_org().await;
    // Reopen the same org.db this router was built with, to seed a memory
    // directly — router() takes ownership of the Arc<SqliteStore>, so the
    // test writes through a fresh connection to the same file.
    let org_path = org_dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, org_path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap(); // idempotent — also sets this connection's pragmas
    let org_store = SqliteStore::new(conn);
    org_store
        .store(&crate::store::NewMemoryRow {
            id: "mem_org1",
            title: "org title",
            content: "org content",
            tags: &[],
            token_count: None,
            layer: "org",
            memory_type: "project",
        })
        .await
        .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/memories/mem_org1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["title"], "org title");
}

#[tokio::test]
async fn list_conflicts_returns_empty() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(app, "GET", "/api/v1/conflicts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 0);
}

#[tokio::test]
async fn save_sync_settings_returns_not_saved() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(
        app,
        "POST",
        "/api/v1/settings/sync",
        Some(serde_json::json!({"enabled": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["saved"], false);
}

#[tokio::test]
async fn save_tag_settings_rejects_malformed_body() {
    let (app, _dir) = test_router().await;
    let (status, _) = req(
        app,
        "POST",
        "/api/v1/settings/tags",
        Some(json!({ "area": { "color": 5 } })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn save_tag_settings_rejects_missing_values_array() {
    let (app, _dir) = test_router().await;
    let (status, _) = req(
        app,
        "POST",
        "/api/v1/settings/tags",
        Some(json!({ "area": { "color": "#fff" } })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn save_tag_settings_rejects_non_bool_single_value() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(
        app,
        "POST",
        "/api/v1/settings/tags",
        Some(json!({ "area": { "color": "#fff", "values": [], "single_value": "yes" } })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"].as_str().unwrap().contains("single_value"));
}

#[tokio::test]
async fn save_tag_settings_rejects_non_string_description() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(
        app,
        "POST",
        "/api/v1/settings/tags",
        Some(json!({ "area": { "color": "#fff", "values": [], "description": 5 } })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"].as_str().unwrap().contains("description"));
}

#[tokio::test]
async fn save_tag_settings_rejects_invalid_values_mode() {
    let (app, _dir) = test_router().await;
    let (status, _) = req(
        app,
        "POST",
        "/api/v1/settings/tags",
        Some(json!({ "status": { "color": "#fff", "values": [], "values_mode": "strict" } })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn save_tag_settings_accepts_fixed_values_mode() {
    // Guard disabled — this test is about format acceptance for a custom
    // namespace shape, not about the predefined-namespace guard.
    let (app, _dir) = test_router_with_guard(false).await;
    let (status, _) = req(
        app,
        "POST",
        "/api/v1/settings/tags",
        Some(json!({ "status": { "color": "#fff", "values": ["idea", "done"], "values_mode": "fixed" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn get_tag_settings_returns_seeded_defaults_when_unset() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(app, "GET", "/api/v1/settings/tags", None).await;
    assert_eq!(status, StatusCode::OK);
    let ns = &body["namespaces"];
    assert!(ns["project"]["color"].is_string());
    assert!(ns["lang"]["color"].is_string());
    assert!(ns["topic"]["color"].is_string());
    assert!(ns["status"]["color"].is_string());
    assert!(ns["project"]["description"].is_string());
    assert_eq!(ns["project"]["values"], json!([]));
    assert_eq!(ns["part"]["values"], json!(["index", "fragment"]));
    assert_eq!(ns["part"]["values_mode"], "fixed");

    let predefined = body["predefined"].as_array().unwrap();
    for name in [
        "project", "topic", "status", "lang", "kind", "scope", "part",
    ] {
        assert!(
            predefined.iter().any(|v| v == name),
            "{name} should be listed as predefined"
        );
    }
    assert_eq!(body["guard_predefined_namespaces"], true);
}

#[tokio::test]
async fn save_tag_settings_rejects_deleting_a_predefined_namespace() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(app, "GET", "/api/v1/settings/tags", None).await;
    assert_eq!(status, StatusCode::OK);
    let mut namespaces = body["namespaces"].clone();
    namespaces.as_object_mut().unwrap().remove("project");

    let (app, _dir2) = test_router().await;
    let (status, _) = req(app, "POST", "/api/v1/settings/tags", Some(namespaces)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn save_tag_settings_rejects_modifying_a_predefined_namespace() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(app.clone(), "GET", "/api/v1/settings/tags", None).await;
    assert_eq!(status, StatusCode::OK);
    let mut namespaces = body["namespaces"].clone();
    namespaces["project"]["color"] = json!("#000000");

    let (status, _) = req(app, "POST", "/api/v1/settings/tags", Some(namespaces)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn save_tag_settings_allows_editing_a_custom_namespace() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(app.clone(), "GET", "/api/v1/settings/tags", None).await;
    assert_eq!(status, StatusCode::OK);
    let mut namespaces = body["namespaces"].clone();
    namespaces["mycustomns"] = json!({ "color": "#123456", "values": ["a"] });

    let (status, _) = req(app, "POST", "/api/v1/settings/tags", Some(namespaces)).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn save_tag_settings_allows_predefined_edits_when_guard_disabled() {
    let (app, _dir) = test_router_with_guard(false).await;
    let (status, body) = req(app.clone(), "GET", "/api/v1/settings/tags", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["guard_predefined_namespaces"], false);
    let mut namespaces = body["namespaces"].clone();
    namespaces["project"]["color"] = json!("#000000");

    let (status, _) = req(app, "POST", "/api/v1/settings/tags", Some(namespaces)).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn count_tokens_returns_zero_for_empty_input() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(
        app,
        "POST",
        "/api/v1/memories/count-tokens",
        Some(json!({ "title": "", "content": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["tokens"], 0);
    assert_eq!(body["max_content_tokens"], 1500);
}

#[tokio::test]
async fn count_tokens_counts_title_and_content_together() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(
        app,
        "POST",
        "/api/v1/memories/count-tokens",
        Some(json!({ "title": "a short title", "content": "some body content here" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["tokens"].as_i64().unwrap() > 0);
}

#[tokio::test]
async fn count_tokens_reflects_custom_max_content_tokens() {
    let (app, _dir) = test_router().await;
    let (status, _) = req(
        app.clone(),
        "POST",
        "/api/v1/settings/content-limits",
        Some(json!({ "max_content_tokens": 42 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = req(
        app,
        "POST",
        "/api/v1/memories/count-tokens",
        Some(json!({ "title": "", "content": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["max_content_tokens"], 42);
}

#[tokio::test]
async fn save_tag_settings_persists_and_get_returns_it() {
    // Guard disabled — this test is about the persist/round-trip mechanics,
    // not the predefined-namespace guard, so it's free to replace project/lang.
    let (app, _dir) = test_router_with_guard(false).await;
    let custom = json!({
        "project": { "color": "#4a9eff", "values": ["hivemind", "oxhive"] },
        "lang": { "color": "#e0607e", "values": ["rust"] },
    });
    let (status, saved) = req(
        app.clone(),
        "POST",
        "/api/v1/settings/tags",
        Some(custom.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["saved"], true);

    let (status, body) = req(app, "GET", "/api/v1/settings/tags", None).await;
    assert_eq!(status, StatusCode::OK);
    // The registry backfills any default namespace missing from the stored
    // blob (see tag_namespace_registry's merge), so it's a superset of what
    // was submitted, not an exact match — check the submitted keys survived
    // as-is rather than asserting the whole object.
    assert_eq!(body["namespaces"]["project"], custom["project"]);
    assert_eq!(body["namespaces"]["lang"], custom["lang"]);
}

#[tokio::test]
async fn get_content_limit_settings_returns_default_when_unset() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(app, "GET", "/api/v1/settings/content-limits", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["max_content_tokens"], 1500);
}

#[tokio::test]
async fn save_content_limit_settings_rejects_non_positive_value() {
    let (app, _dir) = test_router().await;
    let (status, _) = req(
        app,
        "POST",
        "/api/v1/settings/content-limits",
        Some(json!({ "max_content_tokens": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn save_content_limit_settings_rejects_missing_field() {
    let (app, _dir) = test_router().await;
    let (status, _) = req(
        app,
        "POST",
        "/api/v1/settings/content-limits",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn save_content_limit_settings_persists_and_get_returns_it() {
    let (app, _dir) = test_router().await;
    let (status, saved) = req(
        app.clone(),
        "POST",
        "/api/v1/settings/content-limits",
        Some(json!({ "max_content_tokens": 800 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["saved"], true);

    let (status, body) = req(app, "GET", "/api/v1/settings/content-limits", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["max_content_tokens"], 800);
}

#[test]
fn localhost_origins_with_localhost_url() {
    let result = localhost_origins("http://localhost:3457");
    // Should produce two origins: localhost and 127.0.0.1 sibling
    // We just verify the call succeeds and returns something
    let _ = result;
}

#[test]
fn localhost_origins_with_unrecognized_origin() {
    let result = localhost_origins("https://example.com");
    let _ = result;
}

#[test]
fn localhost_origins_with_empty_string() {
    let result = localhost_origins("");
    let _ = result;
}

#[tokio::test]
async fn get_memory_returns_404_for_missing_id() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(app, "GET", "/api/v1/memories/mem_missing", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].as_str().unwrap().contains("no memory"));
}

#[tokio::test]
async fn create_memory_rejects_invalid_memory_type() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(
        app,
        "POST",
        "/api/v1/memories",
        Some(json!({ "title": "t", "content": "c", "memory_type": "bogus" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"].as_str().is_some());
}

#[tokio::test]
async fn create_and_list_feedback() {
    let (app, _dir) = test_router().await;

    let (st, created_mem) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("ref mem", "content", &[])),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let mem_id = created_mem["id"].as_str().unwrap().to_string();

    let (st, fb) = req(
        app.clone(),
        "POST",
        "/api/v1/feedback",
        Some(serde_json::json!({"memory_id": mem_id, "signal": "positive", "note": "great"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    assert!(fb["id"].as_str().unwrap().starts_with("fb_"));

    let (st, list) = req(app, "GET", "/api/v1/feedback", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(list["count"], 1);
}

#[tokio::test]
async fn list_edges_filtered() {
    let (app, _dir) = test_router().await;

    let (_, ma) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("A", "a", &[])),
    )
    .await;
    let (_, mb) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("B", "b", &[])),
    )
    .await;
    let id_a = ma["id"].as_str().unwrap().to_string();
    let id_b = mb["id"].as_str().unwrap().to_string();

    let (st, _) = req(
        app.clone(),
        "POST",
        "/api/v1/edges",
        Some(serde_json::json!({"source_id": id_a, "target_id": id_b, "relationship": "sibling"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);

    let (st, filtered) = req(app, "GET", &format!("/api/v1/edges?memory_id={id_a}"), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(filtered["count"], 1);
}

#[tokio::test]
async fn create_edge_retries_against_org_on_missing_endpoint() {
    let (app, _dir, _org_dir) = test_router_with_org().await;
    // Two org-layer memories — primary store has neither, so primary's
    // create_edge returns MissingEndpoint and this should retry in org.
    let mut ids = vec![];
    for title in ["a", "b"] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/memories")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({ "title": title, "content": "c", "layer": "org" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        ids.push(json["id"].as_str().unwrap().to_string());
    }

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/edges")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "source_id": ids[0],
                        "target_id": ids[1],
                        "relationship": "sibling"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn list_edges_includes_org_edges_when_configured() {
    let (app, _dir, _org_dir) = test_router_with_org().await;
    let mut ids = vec![];
    for title in ["a", "b"] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/memories")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({ "title": title, "content": "c", "layer": "org" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        ids.push(json["id"].as_str().unwrap().to_string());
    }
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/edges")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "source_id": ids[0], "target_id": ids[1], "relationship": "sibling" })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/edges")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["count"], 1);
}

#[tokio::test]
async fn create_edge_rejects_duplicate_missing_endpoint_and_bad_relationship() {
    let (app, _dir) = test_router().await;
    let (_, ma) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("A", "a", &[])),
    )
    .await;
    let (_, mb) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("B", "b", &[])),
    )
    .await;
    let id_a = ma["id"].as_str().unwrap().to_string();
    let id_b = mb["id"].as_str().unwrap().to_string();

    let (st, _) = req(
        app.clone(),
        "POST",
        "/api/v1/edges",
        Some(json!({"source_id": id_a, "target_id": id_b, "relationship": "sibling"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);

    // Duplicate
    let (st, _) = req(
        app.clone(),
        "POST",
        "/api/v1/edges",
        Some(json!({"source_id": id_a, "target_id": id_b, "relationship": "sibling"})),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);

    // Missing endpoint (no org configured to retry against)
    let (st, _) = req(
        app.clone(),
        "POST",
        "/api/v1/edges",
        Some(
            json!({"source_id": id_a, "target_id": "mem_does_not_exist", "relationship": "parent"}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);

    // Invalid relationship
    let (st, body) = req(
        app,
        "POST",
        "/api/v1/edges",
        Some(json!({"source_id": id_a, "target_id": id_b, "relationship": "bogus"})),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("invalid relationship")
    );
}

#[tokio::test]
async fn patch_edge_status_not_found_when_no_org_configured() {
    let (app, _dir) = test_router().await;
    let (st, body) = req(
        app,
        "PATCH",
        "/api/v1/edges/edge_does_not_exist",
        Some(json!({"status": "active"})),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert!(body["error"].as_str().unwrap().contains("no edge"));
}

#[tokio::test]
async fn resolve_conflict_success() {
    let (app, store, _dir) = test_router_with_store().await;

    store
        .store(&crate::store::NewMemoryRow {
            id: "mem_rc",
            title: "RC Memory",
            content: "content",
            tags: &[],
            token_count: None,
            layer: "workspace",
            memory_type: "project",
        })
        .await
        .unwrap();
    let conflict = store
        .write_conflict("mem_rc", "remote content", "content", 2, 1)
        .await
        .unwrap();

    let (status, body) = req(
        app,
        "POST",
        &format!("/api/v1/conflicts/{}/resolve", conflict.id),
        Some(serde_json::json!({"resolution": "keep_local"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["resolved"], true);
    assert_eq!(body["resolution"], "keep_local");
}

#[tokio::test]
async fn resolve_conflict_rejects_invalid_resolution() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(
        app,
        "POST",
        "/api/v1/conflicts/cfl_whatever/resolve",
        Some(json!({ "resolution": "bogus" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"].as_str().unwrap().contains("keep_local"));
}

#[tokio::test]
async fn patch_feedback_rejects_invalid_status_and_missing_id() {
    let (app, _dir) = test_router().await;
    let (_, ma) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("A", "a", &[])),
    )
    .await;
    let (_, fb) = req(
        app.clone(),
        "POST",
        "/api/v1/feedback",
        Some(json!({ "memory_id": ma["id"], "signal": "outdated" })),
    )
    .await;

    let (st, body) = req(
        app.clone(),
        "PATCH",
        &format!("/api/v1/feedback/{}", fb["id"].as_str().unwrap()),
        Some(json!({"status": "bogus"})),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("pending|resolved|dismissed")
    );

    let (st, body) = req(
        app,
        "PATCH",
        "/api/v1/feedback/fb_missing",
        Some(json!({"status": "resolved"})),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert!(body["error"].as_str().unwrap().contains("no feedback"));
}

#[tokio::test]
async fn resolve_conflict_returns_404_for_missing() {
    let (app, _dir) = test_router().await;
    let (status, _) = req(
        app,
        "POST",
        "/api/v1/conflicts/cfl_missing/resolve",
        Some(json!({ "resolution": "keep_local" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn resolve_conflict_retries_against_org_when_not_found_in_primary() {
    let (app, _dir, org_dir) = test_router_with_org().await;

    let org_path = org_dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, org_path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    let org_store = SqliteStore::new(conn);
    org_store
        .store(&crate::store::NewMemoryRow {
            id: "mem_org_resolve",
            title: "org resolve memory",
            content: "content",
            tags: &[],
            token_count: None,
            layer: "org",
            memory_type: "project",
        })
        .await
        .unwrap();
    let conflict = org_store
        .write_conflict("mem_org_resolve", "remote content", "local content", 2, 1)
        .await
        .unwrap();

    let (status, body) = req(
        app,
        "POST",
        &format!("/api/v1/conflicts/{}/resolve", conflict.id),
        Some(json!({ "resolution": "keep_local" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["resolved"], true);
}

#[tokio::test]
async fn list_conflicts_includes_org_conflicts_stamped_with_layer() {
    let (app, _dir, org_dir) = test_router_with_org().await;

    // Seed a conflict directly into the org db (router() already holds the
    // org store; this reopens a fresh connection to the same file, same
    // pattern as get_memory_falls_back_to_org_store).
    let org_path = org_dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, org_path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    let org_store = SqliteStore::new(conn);
    org_store
        .store(&crate::store::NewMemoryRow {
            id: "mem_org_conflict",
            title: "org conflict memory",
            content: "content",
            tags: &[],
            token_count: None,
            layer: "org",
            memory_type: "project",
        })
        .await
        .unwrap();
    org_store
        .write_conflict("mem_org_conflict", "remote content", "local content", 2, 1)
        .await
        .unwrap();

    let (status, body) = req(app, "GET", "/api/v1/conflicts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 1);
    assert_eq!(body["conflicts"][0]["layer"], "org");
}

#[tokio::test]
async fn list_conflicts_primary_entries_have_no_layer_field() {
    let (app, store, _dir) = test_router_with_store().await;
    store
        .store(&crate::store::NewMemoryRow {
            id: "mem_primary_conflict",
            title: "primary conflict memory",
            content: "content",
            tags: &[],
            token_count: None,
            layer: "workspace",
            memory_type: "project",
        })
        .await
        .unwrap();
    store
        .write_conflict(
            "mem_primary_conflict",
            "remote content",
            "local content",
            2,
            1,
        )
        .await
        .unwrap();

    let (status, body) = req(app, "GET", "/api/v1/conflicts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 1);
    assert!(body["conflicts"][0].get("layer").is_none());
}

#[tokio::test]
async fn delete_all_memories_clears_store() {
    let (app, _dir) = test_router().await;
    req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("a", "x", &[])),
    )
    .await;
    req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("b", "y", &[])),
    )
    .await;
    let (st, body) = req(app.clone(), "DELETE", "/api/v1/memories/all", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["deleted"], 2);
    let (_, list) = req(app, "GET", "/api/v1/memories", None).await;
    assert_eq!(list["count"], 0);
}

#[tokio::test]
async fn export_import_roundtrip() {
    let (app, _dir) = test_router().await;
    req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("m1", "c1", &["t"])),
    )
    .await;
    let (st, dump) = req(app.clone(), "GET", "/api/v1/export", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(dump["memories"].as_array().unwrap().len(), 1);

    let (app2, _dir2) = test_router().await;
    let (st, res) = req(app2.clone(), "POST", "/api/v1/import", Some(dump)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(res["imported_memories"], 1);
    let (_, list) = req(app2, "GET", "/api/v1/memories", None).await;
    assert_eq!(list["memories"][0]["title"], "m1");
}

#[tokio::test]
async fn import_applies_defaults_for_omitted_layer_type_and_edge_status() {
    let (app, _dir) = test_router().await;
    let (st, res) = req(
        app.clone(),
        "POST",
        "/api/v1/import",
        Some(json!({
            "memories": [
                { "id": "mem_a", "title": "A", "content": "a" },
                { "id": "mem_b", "title": "B", "content": "b" }
            ],
            "edges": [
                { "source_id": "mem_a", "target_id": "mem_b", "relationship": "sibling" }
            ]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(res["imported_memories"], 2);
    assert_eq!(res["imported_edges"], 1);

    let (_, mem) = req(app.clone(), "GET", "/api/v1/memories/mem_a", None).await;
    assert_eq!(mem["layer"], "workspace");
    assert_eq!(mem["memory_type"], "project");

    let (_, edges) = req(app, "GET", "/api/v1/edges", None).await;
    assert_eq!(edges["edges"][0]["status"], "active");
}

#[tokio::test]
async fn export_import_roundtrip_preserves_edge_status_and_link_text() {
    let (app, store, _dir) = test_router_with_store().await;
    let tags: Vec<String> = vec![];
    store
        .store(&crate::store::NewMemoryRow {
            id: "mem_a",
            title: "A",
            content: "a",
            tags: &tags,
            token_count: None,
            layer: "workspace",
            memory_type: "project",
        })
        .await
        .unwrap();
    store
        .store(&crate::store::NewMemoryRow {
            id: "mem_b",
            title: "B",
            content: "b",
            tags: &tags,
            token_count: None,
            layer: "workspace",
            memory_type: "project",
        })
        .await
        .unwrap();
    store
        .create_edge_with_status(
            "mem_a",
            "mem_b",
            "sibling",
            "pending",
            Some("the phrase"),
            None,
        )
        .await
        .unwrap();

    let (st, dump) = req(app, "GET", "/api/v1/export", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(dump["edges"][0]["status"], "pending");
    assert_eq!(dump["edges"][0]["link_text"], "the phrase");

    let (app2, _dir2) = test_router().await;
    let (st, res) = req(app2.clone(), "POST", "/api/v1/import", Some(dump)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(res["imported_edges"], 1);

    let (_, edges) = req(app2, "GET", "/api/v1/edges", None).await;
    assert_eq!(edges["edges"][0]["status"], "pending");
    assert_eq!(edges["edges"][0]["link_text"], "the phrase");
}

#[tokio::test]
async fn patch_edge_and_feedback_status() {
    let (app, store, _dir) = test_router_with_store().await;
    let tags: Vec<String> = vec![];
    store
        .store(&crate::store::NewMemoryRow {
            id: "mem_a",
            title: "A",
            content: "a",
            tags: &tags,
            token_count: None,
            layer: "workspace",
            memory_type: "project",
        })
        .await
        .unwrap();
    store
        .store(&crate::store::NewMemoryRow {
            id: "mem_b",
            title: "B",
            content: "b",
            tags: &tags,
            token_count: None,
            layer: "workspace",
            memory_type: "project",
        })
        .await
        .unwrap();
    let crate::model::EdgeCreate::Created(edge_id) = store
        .create_edge("mem_a", "mem_b", "sibling")
        .await
        .unwrap()
    else {
        panic!()
    };
    let (st, body) = req(
        app.clone(),
        "PATCH",
        &format!("/api/v1/edges/{edge_id}"),
        Some(json!({"status": "rejected"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["status"], "rejected");
    let (st, _) = req(
        app.clone(),
        "PATCH",
        &format!("/api/v1/edges/{edge_id}"),
        Some(json!({"status": "bogus"})),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);

    let fb = store
        .create_feedback("mem_a", "outdated", None)
        .await
        .unwrap();
    let (st, body) = req(
        app,
        "PATCH",
        &format!("/api/v1/feedback/{}", fb.id),
        Some(json!({"status": "dismissed"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["status"], "dismissed");
}

#[tokio::test]
async fn edge_patch_broadcasts_typed_changed_event() {
    let (app, mut rx, _dir) = test_router_with_events().await;
    let (_, ma) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("A", "a", &[])),
    )
    .await;
    let (_, mb) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(memory_body("B", "b", &[])),
    )
    .await;
    let id_a = ma["id"].as_str().unwrap().to_string();
    let id_b = mb["id"].as_str().unwrap().to_string();
    let (_, edge) = req(
        app.clone(),
        "POST",
        "/api/v1/edges",
        Some(json!({"source_id": id_a, "target_id": id_b, "relationship": "sibling"})),
    )
    .await;
    let edge_id = edge["id"].as_str().unwrap().to_string();

    // Drain events emitted so far (memory creates, edge create) so we
    // observe the one from the PATCH below.
    while rx.try_recv().is_ok() {}

    let (st, _) = req(
        app,
        "PATCH",
        &format!("/api/v1/edges/{edge_id}"),
        Some(json!({"status": "rejected"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let evt = rx.recv().await.unwrap();
    assert_eq!(evt["type"], "changed");
}

#[tokio::test]
async fn status_includes_sync_details() {
    let (app, store, _dir) = test_router_with_store().await;
    store
        .set_meta("last_synced_at", "1751600000")
        .await
        .unwrap();
    let (st, body) = req(app, "GET", "/api/v1/status", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["sync"]["last_synced_at"], 1751600000_i64);
    assert_eq!(body["sync"]["conflict_count"], 0);
}

#[tokio::test]
async fn suggest_session_start_status_end_roundtrip() {
    let (app, _dir) = test_router().await;

    let (st, body) = req(app.clone(), "POST", "/api/v1/suggest-sessions", None).await;
    assert_eq!(st, StatusCode::ACCEPTED);
    assert_eq!(body["started"], true);

    let (st, _) = req(app.clone(), "POST", "/api/v1/suggest-sessions", None).await;
    assert_eq!(st, StatusCode::CONFLICT);

    let (st, status) = req(app.clone(), "GET", "/api/v1/suggest-sessions/current", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(status["active"], true);

    let (st, ended) = req(
        app.clone(),
        "DELETE",
        "/api/v1/suggest-sessions/current",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(ended["ended"], true);
}

#[tokio::test]
async fn revise_validates_session_and_edge() {
    // The manager checks the edge exists before checking session state
    // (see suggest_session::revise), so exercising the "no active
    // session" 409 needs a real edge_id; a bogus id would 404 either way.
    let (app, store, _dir) = test_router_with_store().await;
    let tags: Vec<String> = vec![];
    store
        .store(&crate::store::NewMemoryRow {
            id: "mem_a",
            title: "A",
            content: "a",
            tags: &tags,
            token_count: None,
            layer: "workspace",
            memory_type: "project",
        })
        .await
        .unwrap();
    store
        .store(&crate::store::NewMemoryRow {
            id: "mem_b",
            title: "B",
            content: "b",
            tags: &tags,
            token_count: None,
            layer: "workspace",
            memory_type: "project",
        })
        .await
        .unwrap();
    let crate::model::EdgeCreate::Created(edge_id) = store
        .create_edge_with_status("mem_a", "mem_b", "sibling", "pending", None, None)
        .await
        .unwrap()
    else {
        panic!("expected EdgeCreate::Created");
    };

    let (st, _) = req(
        app.clone(),
        "POST",
        "/api/v1/suggest-sessions/current/revise",
        Some(json!({ "edge_id": edge_id, "feedback": "make it parent" })),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);

    let (st, _) = req(app.clone(), "POST", "/api/v1/suggest-sessions", None).await;
    assert_eq!(st, StatusCode::ACCEPTED);

    let (st, _) = req(
        app.clone(),
        "POST",
        "/api/v1/suggest-sessions/current/revise",
        Some(json!({ "edge_id": "edge_bogus", "feedback": "make it parent" })),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn create_memory_with_org_layer_routes_to_org_store_when_configured() {
    let (app, _dir, _org_dir) = test_router_with_org().await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/memories")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "title": "t", "content": "c", "layer": "org" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    let id = json["id"].as_str().unwrap().to_string();

    // Confirm it landed in org, not primary, by checking it's retrievable
    // (get_memory's org fallback from Task 3 makes this ambiguous on its
    // own, so this test instead directly inspects the response's layer).
    let get_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/memories/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let get_body = get_resp.into_body().collect().await.unwrap().to_bytes();
    let get_json: Value = serde_json::from_slice(&get_body).unwrap();
    assert_eq!(get_json["layer"], "org");
}

#[tokio::test]
async fn create_memory_with_org_layer_errors_when_not_configured() {
    let (app, _dir) = test_router().await;
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/memories")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "title": "t", "content": "c", "layer": "org" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["error"],
        "org layer not configured — set [org_sync] in the global config"
    );
}

#[tokio::test]
async fn list_memories_includes_org_entries_when_configured() {
    let (app, _dir, _org_dir) = test_router_with_org().await;
    // Create one workspace memory and one org memory via the API itself.
    for layer in ["workspace", "org"] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/memories")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({ "title": layer, "content": "c", "layer": layer }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/memories")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    let titles: Vec<&str> = json["memories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["title"].as_str().unwrap())
        .collect();
    assert!(titles.contains(&"workspace"));
    assert!(titles.contains(&"org"));
}

#[tokio::test]
async fn search_includes_org_entries_when_configured() {
    let (app, _dir, _org_dir) = test_router_with_org().await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/memories")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "title": "orgsearchable", "content": "unique org content", "layer": "org" })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/search?q=orgsearchable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["count"], 1);
    assert_eq!(json["results"][0]["title"], "orgsearchable");
}

// Only GET /api/v1/update is exercised here — POST /api/v1/update/apply
// spawns a real `cargo binstall` + process-replacing restart on success
// (see update::do_update), which must never run inside a test.
#[tokio::test]
async fn get_update_state_reports_idle_by_default() {
    let (app, _dir) = test_router().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/update")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "idle");
    assert_eq!(json["available"], false);
}

#[tokio::test]
async fn sse_events_endpoint_responds_with_event_stream_headers() {
    let (app, _dir) = test_router().await;
    // Only check status/headers — the body is an infinite keep-alive stream,
    // so it must never be collected/awaited to completion in a test.
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "text/event-stream");
}
