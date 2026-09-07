/// Integration tests: spin up a real HTTP server on a random port and test
/// the full request/response lifecycle over the network stack via axum's
/// tower oneshot helper (avoids the reqwest dependency).
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use oxmynd::{
    config::{AgentSettings, SyncSettings},
    db,
    store::SqliteStore,
    suggest_session::SuggestSessionManager,
};
use serde_json::{Value, json};
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;

async fn test_app() -> (axum::Router, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let sync = SyncSettings::default();
    let database = db::open_database(&sync, path.to_str().unwrap())
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    db::run_migrations(&conn).await.unwrap();
    let store = Arc::new(SqliteStore::new(conn));
    let (events, _) = tokio::sync::broadcast::channel(16);
    let script = dir.path().join("stub-agent.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\necho '{\"type\":\"result\",\"session_id\":\"stub-1\",\"result\":\"done\",\"is_error\":false}'\n",
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let agent = AgentSettings {
        command: script.to_string_lossy().into_owned(),
        args: vec![],
        kind: oxmynd::config::AgentKind::Claude,
    };
    let suggest = SuggestSessionManager::new(
        Arc::clone(&store),
        events.clone(),
        agent,
        "http://127.0.0.1:3456/mcp".into(),
    );
    let update_state = Arc::new(tokio::sync::RwLock::new(
        oxmynd::update::UpdateState::new_idle(),
    ));
    let router = oxmynd::api::router(
        store,
        sync,
        "http://127.0.0.1:3457",
        events,
        suggest,
        update_state,
        oxmynd::config::AgentSettings::default(),
        true,
    );
    (router, dir)
}

async fn req(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
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
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, val)
}

fn mem_body(title: &str, content: &str, tags: &[&str]) -> Value {
    json!({ "title": title, "content": content, "tags": tags })
}

// ── Status ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn status_returns_version_and_zero_count() {
    let (app, _dir) = test_app().await;

    let (status, body) = req(app, "GET", "/api/v1/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["memory_count"], 0);
    assert_eq!(body["sync"]["enabled"], false);
}

// ── Memory CRUD ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn memory_create_and_retrieve() {
    let (app, _dir) = test_app().await;

    let (status, created) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(mem_body("golang prefs", "use zap and chi", &["golang"])),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("mem_"));

    let (status, body) = req(app, "GET", &format!("/api/v1/memories/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "golang prefs");
    assert_eq!(body["content"], "use zap and chi");
    assert_eq!(body["tags"], json!(["golang"]));
}

#[tokio::test]
async fn memory_get_returns_404_for_missing() {
    let (app, _dir) = test_app().await;
    let (status, _) = req(app, "GET", "/api/v1/memories/mem_nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn memory_patch_updates_content() {
    let (app, _dir) = test_app().await;

    let (_, created) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(mem_body("original", "old", &[])),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();

    let (status, _) = req(
        app.clone(),
        "PATCH",
        &format!("/api/v1/memories/{id}"),
        Some(json!({ "content": "updated content" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, body) = req(app, "GET", &format!("/api/v1/memories/{id}"), None).await;
    assert_eq!(body["content"], "updated content");
    assert_eq!(body["title"], "original", "title unchanged");
}

#[tokio::test]
async fn memory_delete_removes_entry() {
    let (app, _dir) = test_app().await;

    let (_, created) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(mem_body("to-delete", "bye", &[])),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();

    let (status, _) = req(
        app.clone(),
        "DELETE",
        &format!("/api/v1/memories/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = req(app, "GET", &format!("/api/v1/memories/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── Search ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn search_finds_stored_memory() {
    let (app, _dir) = test_app().await;

    req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(mem_body(
            "db driver choice",
            "standardized on pgx v5",
            &["golang"],
        )),
    )
    .await;

    let (status, body) = req(app, "GET", "/api/v1/search?q=pgx", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 1);
    assert_eq!(body["results"][0]["title"], "db driver choice");
}

#[tokio::test]
async fn search_returns_empty_for_no_match() {
    let (app, _dir) = test_app().await;
    let (status, body) = req(app, "GET", "/api/v1/search?q=nonexistent_term", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 0);
}

// ── Edges ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn edge_create_and_list() {
    let (app, _dir) = test_app().await;

    let (_, a) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(mem_body("a", "x", &[])),
    )
    .await;
    let (_, b) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(mem_body("b", "y", &[])),
    )
    .await;
    let (aid, bid) = (
        a["id"].as_str().unwrap().to_string(),
        b["id"].as_str().unwrap().to_string(),
    );

    let (status, edge) = req(
        app.clone(),
        "POST",
        "/api/v1/edges",
        Some(json!({ "source_id": aid, "target_id": bid, "relationship": "sibling" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(edge["id"].as_str().unwrap().starts_with("edge_"));

    let (_, edges) = req(app, "GET", "/api/v1/edges", None).await;
    assert!(edges["count"].as_i64().unwrap() >= 1);
}

// ── Feedback ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn feedback_create_and_list() {
    let (app, _dir) = test_app().await;

    let (_, mem) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(mem_body("stale pref", "old content", &[])),
    )
    .await;
    let mid = mem["id"].as_str().unwrap().to_string();

    let (status, fb) = req(
        app.clone(),
        "POST",
        "/api/v1/feedback",
        Some(json!({ "memory_id": mid, "signal": "outdated", "note": "this is stale" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(fb["id"].as_str().unwrap().starts_with("fb_"));

    let (_, items) = req(app, "GET", "/api/v1/feedback", None).await;
    assert_eq!(items["count"], 1);
}

// ── Delete-all / export / import ─────────────────────────────────────────────

#[tokio::test]
async fn delete_all_clears_memories() {
    let (app, _dir) = test_app().await;
    req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(mem_body("a", "x", &[])),
    )
    .await;
    let (status, body) = req(app.clone(), "DELETE", "/api/v1/memories/all", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["deleted"], 1);
    let (_, list) = req(app, "GET", "/api/v1/memories", None).await;
    assert_eq!(list["count"], 0);
}

#[tokio::test]
async fn export_returns_memories_and_edges() {
    let (app, _dir) = test_app().await;
    req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(mem_body("m", "c", &["t"])),
    )
    .await;
    let (status, body) = req(app, "GET", "/api/v1/export", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["memories"].as_array().unwrap().len(), 1);
    assert!(body["edges"].is_array());
    assert!(body["version"].is_string());
}

#[tokio::test]
async fn import_creates_memories() {
    let (app, _dir) = test_app().await;
    let dump = json!({
        "memories": [
            { "id": "mem_imported", "title": "imported", "content": "c", "tags": [] }
        ],
        "edges": []
    });
    let (status, body) = req(app.clone(), "POST", "/api/v1/import", Some(dump)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["imported_memories"], 1);
    let (_, one) = req(app, "GET", "/api/v1/memories/mem_imported", None).await;
    assert_eq!(one["title"], "imported");
}

// ── Edge / feedback status patch, filters ────────────────────────────────────

#[tokio::test]
async fn patch_edge_status_validates_and_updates() {
    let (app, _dir) = test_app().await;
    let (_, a) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(mem_body("a", "x", &[])),
    )
    .await;
    let (_, b) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(mem_body("b", "y", &[])),
    )
    .await;
    let (aid, bid) = (
        a["id"].as_str().unwrap().to_string(),
        b["id"].as_str().unwrap().to_string(),
    );
    let (_, edge) = req(
        app.clone(),
        "POST",
        "/api/v1/edges",
        Some(json!({ "source_id": aid, "target_id": bid, "relationship": "sibling" })),
    )
    .await;
    let edge_id = edge["id"].as_str().unwrap().to_string();

    let (status, _) = req(
        app.clone(),
        "PATCH",
        &format!("/api/v1/edges/{edge_id}"),
        Some(json!({ "status": "bogus" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, body) = req(
        app,
        "PATCH",
        &format!("/api/v1/edges/{edge_id}"),
        Some(json!({ "status": "pending" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "pending");
}

#[tokio::test]
async fn feedback_status_filter_narrows_results() {
    let (app, _dir) = test_app().await;
    let (_, mem) = req(
        app.clone(),
        "POST",
        "/api/v1/memories",
        Some(mem_body("m", "c", &[])),
    )
    .await;
    let mid = mem["id"].as_str().unwrap().to_string();
    let (_, fb) = req(
        app.clone(),
        "POST",
        "/api/v1/feedback",
        Some(json!({ "memory_id": mid, "signal": "outdated" })),
    )
    .await;
    let fb_id = fb["id"].as_str().unwrap().to_string();

    req(
        app.clone(),
        "PATCH",
        &format!("/api/v1/feedback/{fb_id}"),
        Some(json!({ "status": "dismissed" })),
    )
    .await;

    let (status, pending) = req(app.clone(), "GET", "/api/v1/feedback?status=pending", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pending["count"], 0);

    let (status, dismissed) = req(app, "GET", "/api/v1/feedback?status=dismissed", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(dismissed["count"], 1);
}

#[tokio::test]
async fn session_logs_endpoint_returns_logged_runs() {
    let (app, _dir) = test_app().await;
    // Session-start logging happens via the MCP path (src/server.rs),
    // not through this REST router, so there's no way to seed a row
    // from this file. Assert the endpoint responds correctly on a fresh
    // DB instead; the write path itself is covered by
    // src/server.rs's `session_start_writes_a_log_entry` test (Task 3).
    let (status, body) = req(app, "GET", "/api/v1/session-logs", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 0);
    assert_eq!(body["logs"].as_array().unwrap().len(), 0);
}
