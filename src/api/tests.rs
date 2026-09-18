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
        SyncSettings::default(),
        "http://127.0.0.1:3457",
        events,
        suggest,
        test_update_state(),
        test_agent_settings(),
        guard_predefined_namespaces,
        false,
        None,
        Arc::new(crate::hive::pairing::PairingCodeStore::new()),
        None,
        crate::api::HiveSyncPort(0),
    );
    (r, dir)
}

async fn test_router_with_events() -> (Router, broadcast::Receiver<Value>, TempDir) {
    let (store, dir) = test_store().await;
    let (events, rx) = broadcast::channel(16);
    let suggest = test_suggest_manager(Arc::clone(&store), dir.path(), events.clone());
    let r = router(
        store,
        SyncSettings::default(),
        "http://127.0.0.1:3457",
        events,
        suggest,
        test_update_state(),
        test_agent_settings(),
        true,
        false,
        None,
        Arc::new(crate::hive::pairing::PairingCodeStore::new()),
        None,
        crate::api::HiveSyncPort(0),
    );
    (r, rx, dir)
}

async fn test_router_with_store() -> (Router, Arc<SqliteStore>, TempDir) {
    let (store, dir) = test_store().await;
    let (events, _) = broadcast::channel(16);
    let suggest = test_suggest_manager(Arc::clone(&store), dir.path(), events.clone());
    let r = router(
        Arc::clone(&store),
        SyncSettings::default(),
        "http://127.0.0.1:3457",
        events,
        suggest,
        test_update_state(),
        test_agent_settings(),
        true,
        false,
        None,
        Arc::new(crate::hive::pairing::PairingCodeStore::new()),
        None,
        crate::api::HiveSyncPort(0),
    );
    (r, store, dir)
}

/// Like `test_router_with_store`, but with hive enabled and `identity` wired
/// through as the `Extension<Arc<DeviceIdentity>>`/`HivePushConfig` -- needed
/// by handlers (`hive_status`) that read `hive.identity` rather than degrading
/// to the "disabled, no identity" branch `test_router_with_store` exercises.
async fn test_router_with_hive_identity(
    identity: crate::hive::identity::DeviceIdentity,
) -> (Router, Arc<SqliteStore>, TempDir) {
    let (store, dir) = test_store().await;
    let (events, _) = broadcast::channel(16);
    let suggest = test_suggest_manager(Arc::clone(&store), dir.path(), events.clone());
    let r = router(
        Arc::clone(&store),
        SyncSettings::default(),
        "http://127.0.0.1:3457",
        events,
        suggest,
        test_update_state(),
        test_agent_settings(),
        true,
        true,
        Some(identity),
        Arc::new(crate::hive::pairing::PairingCodeStore::new()),
        None,
        crate::api::HiveSyncPort(4570),
    );
    (r, store, dir)
}

async fn test_router_with_pairing_code(dir: &std::path::Path) -> (Router, String) {
    let (store, _dir) = test_store().await;
    let _ = dir; // kept for call-site compatibility; test_store() makes its own tempdir
    let pairing_codes = Arc::new(crate::hive::pairing::PairingCodeStore::new());
    // hive_pair validates against chrono::Utc::now() (real wall-clock time),
    // so the code must be issued relative to that same clock, not `0` --
    // otherwise it reads as already-expired against any real epoch timestamp.
    let issued = pairing_codes.issue(chrono::Utc::now().timestamp());
    // `/pair` now lives on the server-TLS-only pairing router (Finding C1).
    let r = hive_pairing_router(store, pairing_codes);
    (r, issued.code)
}

/// The mandatory-mTLS sync router (roster/manifest/memories/settings/
/// tag-namespaces/push). `/pair` and `/pairing-code` are NOT here — they moved
/// to `hive_pairing_router` and the plaintext `router()` respectively.
async fn test_hive_router() -> (Router, Arc<crate::hive::pairing::PairingCodeStore>, TempDir) {
    let (store, dir) = test_store().await;
    let pairing_codes = Arc::new(crate::hive::pairing::PairingCodeStore::new());
    let r = hive_sync_router(store);
    (r, pairing_codes, dir)
}

/// Like `test_hive_router`, but also hands back the underlying store so
/// tests can seed state or assert on it directly.
async fn test_hive_router_with_store() -> (Router, Arc<SqliteStore>, TempDir) {
    let (store, dir) = test_store().await;
    let r = hive_sync_router(Arc::clone(&store));
    (r, store, dir)
}

/// A pairing router (server-TLS-only `/pair`) sharing a fresh code store, for
/// the pairing-rejection test that doesn't need a pre-issued valid code.
async fn test_pairing_router() -> (Router, Arc<crate::hive::pairing::PairingCodeStore>, TempDir) {
    let (store, dir) = test_store().await;
    let pairing_codes = Arc::new(crate::hive::pairing::PairingCodeStore::new());
    let r = hive_pairing_router(store, pairing_codes.clone());
    (r, pairing_codes, dir)
}

async fn req(app: Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(uri);
    // `.oneshot()` bypasses `into_make_service_with_connect_info` entirely
    // (no real accepted connection), so `ConnectInfo` would otherwise be
    // absent for every test request. Attach a loopback address by default --
    // matching how every real dashboard/local request actually arrives -- so
    // ordinary tests don't need to know about `require_loopback` at all; the
    // one test that needs a non-loopback address builds its own request.
    let loopback = axum::extract::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 0)));
    let request = match body {
        Some(v) => builder
            .header("content-type", "application/json")
            .extension(loopback)
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => builder.extension(loopback).body(Body::empty()).unwrap(),
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
async fn settings_sync_returns_defaults() {
    let (app, _dir) = test_router().await;
    let (status, body) = req(app, "GET", "/api/v1/settings/sync", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], false);
    assert_eq!(body["interval_seconds"], 300);
}

#[tokio::test]
async fn delete_memory_returns_404_when_not_found() {
    let (app, _dir) = test_router().await;
    let (status, _) = req(app, "DELETE", "/api/v1/memories/mem_nonexistent", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
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
        .write_conflict("mem_rc", "remote content", "content", 2, 1, None)
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
async fn hive_pair_rejects_unknown_code() {
    let (app, _codes, _dir) = test_pairing_router().await;
    // A genuinely valid join record, so the only thing wrong with this
    // request is the code: the signature is checked first (it's stateless),
    // then the single-use code is consumed.
    let identity = crate::hive::identity::generate();
    let join_record = crate::hive::roster::create_join_record(&identity, "x", 1000);
    let (status, _) = req(
        app,
        "POST",
        "/api/v1/hive/pair",
        Some(json!({
            "code": "NOTAREALCODE",
            "join_record": {
                "device_id": join_record.device_id, "public_key": join_record.public_key,
                "name": join_record.name, "joined_at": join_record.joined_at,
                "signature": join_record.signature,
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn hive_pair_does_not_burn_the_code_on_a_bad_join_record() {
    let (app, code) = test_router_with_pairing_code(std::path::Path::new(".")).await;
    let (status, _) = req(
        app.clone(),
        "POST",
        "/api/v1/hive/pair",
        Some(json!({
            "code": code,
            "join_record": { "device_id": "hive_x", "public_key": "00", "name": "x", "joined_at": 0, "signature": "00" }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // The same code still redeems with a proper record -- the malformed
    // attempt must not have consumed it.
    let identity = crate::hive::identity::generate();
    let join_record = crate::hive::roster::create_join_record(&identity, "x", 1000);
    let (status, _) = req(
        app,
        "POST",
        "/api/v1/hive/pair",
        Some(json!({
            "code": code,
            "join_record": {
                "device_id": join_record.device_id, "public_key": join_record.public_key,
                "name": join_record.name, "joined_at": join_record.joined_at,
                "signature": join_record.signature,
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn hive_pair_accepts_valid_code_and_join_record() {
    let identity = crate::hive::identity::generate();
    let join_record = crate::hive::roster::create_join_record(&identity, "bob-phone", 1000);

    let (app, code) = test_router_with_pairing_code(std::path::Path::new(".")).await;
    let (status, body) = req(
        app,
        "POST",
        "/api/v1/hive/pair",
        Some(json!({
            "code": code,
            "join_record": {
                "device_id": join_record.device_id,
                "public_key": join_record.public_key,
                "name": join_record.name,
                "joined_at": join_record.joined_at,
                "signature": join_record.signature,
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["roster"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["device_id"] == join_record.device_id)
    );
}

#[tokio::test]
async fn hive_roster_lists_current_members() {
    let (app, _codes, _dir) = test_hive_router().await;
    let (status, body) = req(app, "GET", "/api/v1/hive/roster", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["roster"], json!([]));
}

#[tokio::test]
async fn hive_manifest_endpoint_reports_stored_memories() {
    let (app, _codes, _dir) = test_hive_router().await;
    // test_hive_router's underlying store is empty; this test only checks
    // the endpoint's shape, not specific content, since seeding a memory
    // requires the store handle test_hive_router doesn't currently expose --
    // if a later task needs a seeded variant, extend test_hive_router rather
    // than duplicating its setup here.
    let (status, body) = req(app, "GET", "/api/v1/hive/manifest", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["memories"], json!({}));
    assert_eq!(body["tombstones"], json!({}));
    assert!(body["settings"]["hash"].is_string());
    assert!(body["tag_namespaces"]["hash"].is_string());
}

#[tokio::test]
async fn hive_pairing_code_endpoint_issues_a_redeemable_code() {
    // `/pairing-code` moved onto the plaintext app router (it's a local,
    // dashboard-triggered action, Finding C1) and issuing a code now also
    // opens the pairing-window listener. Build a minimal router exposing just
    // that endpoint with the extensions the handler needs, sharing the same
    // PairingCodeStore with the pairing router used to redeem below.
    let (store, _dir) = test_store().await;
    let pairing_codes = Arc::new(crate::hive::pairing::PairingCodeStore::new());
    let identity = crate::hive::identity::generate();
    let identity_public_key = crate::hive::identity::public_key_hex(&identity);
    // Port 0 → the OS assigns an ephemeral port, so opening the window binds a
    // real (throwaway) listener without clashing with anything.
    let pairing_window = Arc::new(crate::hive::pairing_window::PairingWindow::new(
        "127.0.0.1".to_string(),
        0,
        identity.clone(),
        crate::api::hive_pairing_router(Arc::clone(&store), pairing_codes.clone()),
    ));
    let issue_app = Router::new()
        .route(
            "/api/v1/hive/pairing-code",
            axum::routing::post(hive_issue_pairing_code),
        )
        .layer(Extension(pairing_codes.clone()))
        .layer(Extension(pairing_window))
        .layer(Extension(Arc::new(identity)));

    let (status, body) = req(issue_app, "POST", "/api/v1/hive/pairing-code", None).await;
    assert_eq!(status, StatusCode::OK);
    let code = body["code"].as_str().unwrap().to_string();
    assert_eq!(code.len(), 8);
    assert_eq!(body["public_key"].as_str().unwrap(), identity_public_key);

    let pair_app = hive_pairing_router(store, pairing_codes);
    let identity = crate::hive::identity::generate();
    let join_record = crate::hive::roster::create_join_record(&identity, "carol-tablet", 1000);
    let (status2, _) = req(
        pair_app,
        "POST",
        "/api/v1/hive/pair",
        Some(json!({
            "code": code,
            "join_record": {
                "device_id": join_record.device_id,
                "public_key": join_record.public_key,
                "name": join_record.name,
                "joined_at": join_record.joined_at,
                "signature": join_record.signature,
            }
        })),
    )
    .await;
    assert_eq!(
        status2,
        StatusCode::OK,
        "a code issued by the new endpoint must be redeemable via /pair"
    );
}

#[tokio::test]
async fn hive_get_memory_returns_404_for_unknown_id() {
    let (app, _codes, _dir) = test_hive_router().await;
    let (status, _) = req(
        app,
        "GET",
        "/api/v1/hive/memories/mem_doesnotexist00000000000001",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn hive_push_memory_applies_a_brand_new_memory() {
    let (app, _codes, _dir) = test_hive_router().await;
    let (status, body) = req(
        app.clone(),
        "POST",
        "/api/v1/hive/push",
        Some(json!({
            "kind": "memory", "id": "mem_pushtest00000000000000000001",
            "title": "from peer", "content": "pushed content", "tags": [],
            "layer": "workspace", "memory_type": "project",
            "updated_at": 1000,
            "hive_content_hash": crate::store::compute_hive_content_hash("from peer", "pushed content", &[], "workspace", "project"),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["outcome"], "Applied");

    let (status2, body2) = req(
        app,
        "GET",
        "/api/v1/hive/memories/mem_pushtest00000000000000000001",
        None,
    )
    .await;
    assert_eq!(status2, StatusCode::OK);
    assert_eq!(body2["title"], "from peer");
}

#[tokio::test]
async fn hive_status_reports_disabled_with_no_identity() {
    // test_router_with_store() builds the router with hive disabled (its
    // `hive_enabled` arg is `false`, `hive_identity` is `None`) -- confirms
    // the endpoint degrades gracefully rather than 500ing on a missing
    // Extension when hive was never turned on.
    let (app, _store, _dir) = test_router_with_store().await;
    let (status, body) = req(app, "GET", "/api/v1/hive/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], false);
    assert!(body["identity"].is_null());
    assert_eq!(body["roster"], json!([]));
    assert_eq!(body["pending_conflict_count"], 0);
}

#[tokio::test]
async fn trusted_networks_crud_roundtrip() {
    let (app, _store, _dir) = test_router_with_store().await;

    let (status, body) = req(app.clone(), "GET", "/api/v1/hive/trusted-networks", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["trusted"], json!([]));

    let (status, body) = req(
        app.clone(),
        "POST",
        "/api/v1/hive/trusted-networks",
        Some(json!({ "id": "ssid:home-wifi", "label": "Home" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["trusted"][0]["id"], "ssid:home-wifi");
    assert_eq!(body["trusted"][0]["label"], "Home");

    let (status, body) = req(app.clone(), "GET", "/api/v1/hive/trusted-networks", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["trusted"].as_array().unwrap().len(), 1);

    let (status, body) = req(
        app.clone(),
        "DELETE",
        "/api/v1/hive/trusted-networks/ssid:home-wifi",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["trusted"], json!([]));
}

#[tokio::test]
async fn trusted_networks_endpoint_rejects_non_loopback_peer() {
    // Issue #27: these routes control the auto-pause safety feature itself,
    // so they must not be reachable by whatever reached the plaintext API
    // over a non-loopback bind -- unlike `req()`'s default loopback
    // ConnectInfo, this builds the request with a real LAN-looking peer
    // address to prove `require_loopback` actually rejects it.
    let (app, _store, _dir) = test_router_with_store().await;
    let non_loopback =
        axum::extract::ConnectInfo(std::net::SocketAddr::from(([192, 168, 1, 50], 54321)));
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/hive/trusted-networks")
        .extension(non_loopback)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(request).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn hive_join_rejects_non_loopback_peer() {
    // A remote caller must not be able to make this device dial an
    // attacker-chosen peer_address/peer_public_key and merge in whatever
    // roster it returns -- see the security review that flagged `hive_join`
    // being reachable without `require_loopback`, unlike its sibling
    // hive-sensitive routes.
    let identity = crate::hive::identity::generate();
    let (app, _store, _dir) = test_router_with_hive_identity(identity).await;
    let non_loopback =
        axum::extract::ConnectInfo(std::net::SocketAddr::from(([192, 168, 1, 50], 54321)));
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/hive/join")
        .header("content-type", "application/json")
        .extension(non_loopback)
        .body(Body::from(
            json!({
                "peer_address": "127.0.0.1:1", "pairing_code": "ABCDEF",
                "peer_public_key": "00".repeat(32),
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(request).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn revoke_device_flips_roster_entry_to_revoked() {
    let (app, store, _dir) = test_router_with_store().await;
    let identity = crate::hive::identity::generate();
    let other = crate::hive::identity::generate();
    // Seed the local device as an Active roster member -- production always
    // does this via `network_guard::spawn_hive_stack`'s idempotent self-join
    // before any revoke can be issued. `merge_roster` only applies a
    // revocation when the *revoker* (this local identity) is a currently-Active
    // roster member in the pre-merge snapshot, so this precondition must hold
    // for the handler's gossip-based revoke to take effect.
    let self_join = crate::hive::roster::create_join_record(&identity, &identity.device_id, 900);
    store
        .hive_upsert_roster_entry(&crate::hive::roster::RosterEntry {
            device_id: identity.device_id.clone(),
            public_key: crate::hive::identity::public_key_hex(&identity),
            name: identity.device_id.clone(),
            status: crate::hive::roster::RosterStatus::Active,
            joined_at: 900,
            revoked_at: None,
            revoked_by: None,
            join_record: self_join,
            revocation_record: None,
        })
        .await
        .unwrap();
    let join = crate::hive::roster::create_join_record(&other, "bob-phone", 1000);
    store
        .hive_upsert_roster_entry(&crate::hive::roster::RosterEntry {
            device_id: other.device_id.clone(),
            public_key: crate::hive::identity::public_key_hex(&other),
            name: "bob-phone".to_string(),
            status: crate::hive::roster::RosterStatus::Active,
            joined_at: 1000,
            revoked_at: None,
            revoked_by: None,
            join_record: join,
            revocation_record: None,
        })
        .await
        .unwrap();

    // This handler needs a local identity Extension to sign the revocation
    // as -- test_router_with_store() builds with hive disabled (no
    // identity), so this test builds its own tiny router the same way
    // test_router_with_pairing_code does for a hive-specific extension need.
    let app = app.layer(axum::extract::Extension(std::sync::Arc::new(identity)));

    let (status, body) = req(
        app,
        "POST",
        &format!("/api/v1/hive/roster/{}/revoke", other.device_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["revoked"], true);

    let roster = store.hive_list_roster().await.unwrap();
    let entry = roster
        .iter()
        .find(|e| e.device_id == other.device_id)
        .unwrap();
    assert_eq!(entry.status, crate::hive::roster::RosterStatus::Revoked);
}

#[tokio::test]
async fn revoke_device_404s_for_unknown_device() {
    let (app, _store, _dir) = test_router_with_store().await;
    let identity = crate::hive::identity::generate();
    let app = app.layer(axum::extract::Extension(std::sync::Arc::new(identity)));
    let (status, _) = req(app, "POST", "/api/v1/hive/roster/hive_unknown/revoke", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn revoke_device_errors_when_local_device_not_yet_active_member() {
    let (app, store, _dir) = test_router_with_store().await;
    let identity = crate::hive::identity::generate();
    let other = crate::hive::identity::generate();
    // Deliberately do NOT seed the local identity as an Active roster member
    // -- this mirrors production before `network_guard::spawn_hive_stack`'s
    // self-join has run (e.g. hive started paused on an untrusted network).
    // `merge_roster` requires the revoker to already be Active in the
    // pre-merge snapshot, so the revocation below must be silently rejected.
    let join = crate::hive::roster::create_join_record(&other, "bob-phone", 1000);
    store
        .hive_upsert_roster_entry(&crate::hive::roster::RosterEntry {
            device_id: other.device_id.clone(),
            public_key: crate::hive::identity::public_key_hex(&other),
            name: "bob-phone".to_string(),
            status: crate::hive::roster::RosterStatus::Active,
            joined_at: 1000,
            revoked_at: None,
            revoked_by: None,
            join_record: join,
            revocation_record: None,
        })
        .await
        .unwrap();

    let app = app.layer(axum::extract::Extension(std::sync::Arc::new(identity)));

    let (status, body) = req(
        app,
        "POST",
        &format!("/api/v1/hive/roster/{}/revoke", other.device_id),
        None,
    )
    .await;
    // Must NOT be the false-success 200 `{"revoked": true}` -- the merge
    // silently no-oped because the local device isn't a trusted revoker yet.
    assert_eq!(status, StatusCode::CONFLICT);
    assert_ne!(body["revoked"], true);

    let roster = store.hive_list_roster().await.unwrap();
    let entry = roster
        .iter()
        .find(|e| e.device_id == other.device_id)
        .unwrap();
    assert_eq!(entry.status, crate::hive::roster::RosterStatus::Active);
}

#[tokio::test]
async fn revoke_device_404s_for_already_revoked_device() {
    let (app, store, _dir) = test_router_with_store().await;
    let identity = crate::hive::identity::generate();
    let other = crate::hive::identity::generate();
    // Seed the local device as an Active roster member, same as
    // `revoke_device_flips_roster_entry_to_revoked`.
    let self_join = crate::hive::roster::create_join_record(&identity, &identity.device_id, 900);
    store
        .hive_upsert_roster_entry(&crate::hive::roster::RosterEntry {
            device_id: identity.device_id.clone(),
            public_key: crate::hive::identity::public_key_hex(&identity),
            name: identity.device_id.clone(),
            status: crate::hive::roster::RosterStatus::Active,
            joined_at: 900,
            revoked_at: None,
            revoked_by: None,
            join_record: self_join,
            revocation_record: None,
        })
        .await
        .unwrap();

    // Seed the second device already in Revoked state, set directly on the
    // roster entry -- no need to go through a real revoke first.
    let join = crate::hive::roster::create_join_record(&other, "bob-phone", 1000);
    let revocation =
        crate::hive::roster::create_revocation_record(&identity, &other.device_id, 1100);
    store
        .hive_upsert_roster_entry(&crate::hive::roster::RosterEntry {
            device_id: other.device_id.clone(),
            public_key: crate::hive::identity::public_key_hex(&other),
            name: "bob-phone".to_string(),
            status: crate::hive::roster::RosterStatus::Revoked,
            joined_at: 1000,
            revoked_at: Some(1100),
            revoked_by: Some(identity.device_id.clone()),
            join_record: join,
            revocation_record: Some(revocation),
        })
        .await
        .unwrap();

    let app = app.layer(axum::extract::Extension(std::sync::Arc::new(identity)));

    let (status, _) = req(
        app,
        "POST",
        &format!("/api/v1/hive/roster/{}/revoke", other.device_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn set_hive_enabled_persists_override_and_signals_restart() {
    let (app, store, _dir) = test_router_with_store().await;
    let restart_notify = Arc::new(tokio::sync::Notify::new());
    let app = app.layer(axum::extract::Extension(restart_notify.clone()));

    let notified = restart_notify.notified();
    let (status, body) = req(
        app,
        "POST",
        "/api/v1/hive/enabled",
        Some(json!({ "enabled": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["restarting"], true);
    assert_eq!(store.hive_enabled_override().await.unwrap(), Some(true));

    // The handler must have called .notify_one() -- this resolves
    // immediately rather than hanging if it did.
    tokio::time::timeout(std::time::Duration::from_secs(1), notified)
        .await
        .expect("hive_set_enabled must signal the restart Notify");
}

#[tokio::test]
async fn hive_pair_rejects_invalid_join_record_signature() {
    let (app, code) = test_router_with_pairing_code(std::path::Path::new(".")).await;
    let (status, _) = req(
        app,
        "POST",
        "/api/v1/hive/pair",
        Some(json!({
            "code": code,
            // A well-formed but forged record: real device_id/public_key
            // shape, but a signature that cannot verify against it.
            "join_record": {
                "device_id": "hive_forged", "public_key": "00".repeat(32),
                "name": "forged", "joined_at": 1000, "signature": "00".repeat(64),
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn hive_get_settings_returns_defaults_then_reflects_override() {
    let (app, store, _dir) = test_hive_router_with_store().await;
    let (status, body) = req(app.clone(), "GET", "/api/v1/hive/settings", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sync_interval_seconds"], 300);
    assert_eq!(body["ping_interval_seconds"], 60);
    assert_eq!(body["updated_at"], 0);

    store
        .set_hive_settings_override(120, 30, 5000)
        .await
        .unwrap();
    let (status, body) = req(app, "GET", "/api/v1/hive/settings", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sync_interval_seconds"], 120);
    assert_eq!(body["ping_interval_seconds"], 30);
    assert_eq!(body["updated_at"], 5000);
}

#[tokio::test]
async fn hive_get_tag_namespaces_returns_registry_and_updated_at() {
    let (app, store, _dir) = test_hive_router_with_store().await;
    store
        .set_meta("tag_namespaces_updated_at", "1234")
        .await
        .unwrap();
    let (status, body) = req(app, "GET", "/api/v1/hive/tag-namespaces", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["updated_at"], 1234);
    assert!(body["namespaces"].is_object());
}

#[tokio::test]
async fn hive_push_tombstone_deletes_a_staler_local_copy() {
    let (app, store, _dir) = test_hive_router_with_store().await;
    store
        .store(&crate::store::NewMemoryRow {
            id: "mem_tombtest0000000000000000001",
            title: "will be tombstoned",
            content: "c",
            tags: &[],
            token_count: None,
            layer: "workspace",
            memory_type: "project",
        })
        .await
        .unwrap();
    // Newer than the local row, but inside the clock-skew tolerance -- a
    // deletion further in the future than that is rejected (see
    // `hive_push_rejects_tombstone_with_implausible_future_timestamp`).
    let slightly_ahead = chrono::Utc::now().timestamp() + 100;

    let (status, body) = req(
        app,
        "POST",
        "/api/v1/hive/push",
        Some(json!({
            "kind": "tombstone",
            "memory_id": "mem_tombtest0000000000000000001",
            "deleted_at": slightly_ahead,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["outcome"], "tombstone_processed");
    assert!(
        store
            .recall_by_id("mem_tombtest0000000000000000001")
            .await
            .unwrap()
            .is_none()
    );
    // The peer's deletion time is what gets recorded, not "when we heard".
    assert_eq!(
        store
            .hive_tombstone_for("mem_tombtest0000000000000000001")
            .await
            .unwrap(),
        Some(slightly_ahead)
    );
}

#[tokio::test]
async fn hive_push_tombstone_for_unknown_id_records_the_tombstone() {
    // Nothing to delete, but the tombstone is still recorded so a later push
    // of that memory from a peer that hasn't caught up can't resurrect it.
    let (app, store, _dir) = test_hive_router_with_store().await;
    let (status, body) = req(
        app,
        "POST",
        "/api/v1/hive/push",
        Some(json!({
            "kind": "tombstone",
            "memory_id": "mem_doesnotexist00000000000002",
            "deleted_at": 9999,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["outcome"], "tombstone_processed");
    assert_eq!(
        store
            .hive_tombstone_for("mem_doesnotexist00000000000002")
            .await
            .unwrap(),
        Some(9999)
    );
}

#[tokio::test]
async fn hive_push_rejects_tombstone_with_implausible_future_timestamp() {
    let (app, store, _dir) = test_hive_router_with_store().await;
    let far_future = chrono::Utc::now().timestamp() + 10_000;
    let (status, _) = req(
        app,
        "POST",
        "/api/v1/hive/push",
        Some(json!({
            "kind": "tombstone",
            "memory_id": "mem_futuretomb000000000000001",
            "deleted_at": far_future,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        store
            .hive_tombstone_for("mem_futuretomb000000000000001")
            .await
            .unwrap()
            .is_none(),
        "a far-future tombstone must not be recorded -- it would block every later re-creation"
    );
}

#[tokio::test]
async fn hive_push_rejects_settings_with_implausible_timestamp_or_interval() {
    let (app, store, _dir) = test_hive_router_with_store().await;
    let far_future = chrono::Utc::now().timestamp() + 10_000;
    let (status, _) = req(
        app.clone(),
        "POST",
        "/api/v1/hive/push",
        Some(json!({
            "kind": "settings", "sync_interval_seconds": 90, "ping_interval_seconds": 15,
            "updated_at": far_future,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // A zero interval would turn the sync/ping loops into a busy-spin; an
    // enormous ping interval would freeze the roster-verifier hot-reload
    // (and so revocations) on every device that accepted it.
    for (sync_s, ping_s) in [(0, 60), (300, 0), (300, 10_000_000)] {
        let (status, _) = req(
            app.clone(),
            "POST",
            "/api/v1/hive/push",
            Some(json!({
                "kind": "settings", "sync_interval_seconds": sync_s,
                "ping_interval_seconds": ping_s, "updated_at": 5000,
            })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "intervals ({sync_s}, {ping_s}) must be rejected"
        );
    }
    assert_eq!(store.hive_settings_override().await.unwrap(), None);
}

#[tokio::test]
async fn hive_push_rejects_malformed_tag_namespaces() {
    let (app, store, _dir) = test_hive_router_with_store().await;
    let (status, _) = req(
        app,
        "POST",
        "/api/v1/hive/push",
        Some(json!({
            "kind": "tag_namespaces",
            // `values` must be an array of strings; this would previously
            // have been persisted verbatim.
            "namespaces": { "project": { "color": "#000", "values": "not-an-array" } },
            "updated_at": 5000,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        store
            .get_meta("tag_namespaces_updated_at")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn hive_push_settings_applies_when_newer_keeps_local_when_older() {
    let (app, store, _dir) = test_hive_router_with_store().await;

    let (status, body) = req(
        app.clone(),
        "POST",
        "/api/v1/hive/push",
        Some(json!({
            "kind": "settings", "sync_interval_seconds": 90, "ping_interval_seconds": 15,
            "updated_at": 5000,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["outcome"], "applied");
    assert_eq!(
        store.hive_settings_override().await.unwrap(),
        Some((90, 15, 5000))
    );

    // An older update must not overwrite the newer local one.
    let (status, body) = req(
        app,
        "POST",
        "/api/v1/hive/push",
        Some(json!({
            "kind": "settings", "sync_interval_seconds": 999, "ping_interval_seconds": 999,
            "updated_at": 1,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["outcome"], "kept_local");
    assert_eq!(
        store.hive_settings_override().await.unwrap(),
        Some((90, 15, 5000))
    );
}

#[tokio::test]
async fn hive_push_tag_namespaces_applies_when_newer_keeps_local_when_older() {
    let (app, store, _dir) = test_hive_router_with_store().await;

    let (status, body) = req(
        app.clone(),
        "POST",
        "/api/v1/hive/push",
        Some(json!({
            "kind": "tag_namespaces",
            "namespaces": { "project": { "color": "#000", "values": ["x"] } },
            "updated_at": 5000,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["outcome"], "applied");
    assert_eq!(
        store
            .get_meta("tag_namespaces_updated_at")
            .await
            .unwrap()
            .unwrap(),
        "5000"
    );

    let (status, body) = req(
        app,
        "POST",
        "/api/v1/hive/push",
        Some(json!({
            "kind": "tag_namespaces", "namespaces": {}, "updated_at": 1,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["outcome"], "kept_local");
    assert_eq!(
        store
            .get_meta("tag_namespaces_updated_at")
            .await
            .unwrap()
            .unwrap(),
        "5000"
    );
}

#[tokio::test]
async fn hive_push_roster_merges_incoming_entries() {
    let (app, store, _dir) = test_hive_router_with_store().await;
    let peer = crate::hive::identity::generate();
    let join = crate::hive::roster::create_join_record(&peer, "peer-device", 1000);
    let entry = crate::hive::roster::RosterEntry {
        device_id: peer.device_id.clone(),
        public_key: crate::hive::identity::public_key_hex(&peer),
        name: "peer-device".to_string(),
        status: crate::hive::roster::RosterStatus::Active,
        joined_at: 1000,
        revoked_at: None,
        revoked_by: None,
        join_record: join,
        revocation_record: None,
    };

    let (status, body) = req(
        app,
        "POST",
        "/api/v1/hive/push",
        Some(json!({ "kind": "roster", "roster": [entry] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["outcome"], "merged");
    let roster = store.hive_list_roster().await.unwrap();
    assert!(roster.iter().any(|e| e.device_id == peer.device_id));
}

#[tokio::test]
async fn hive_status_reports_identity_and_roster_member_fields() {
    let identity = crate::hive::identity::generate();
    let other = crate::hive::identity::generate();
    let (app, store, _dir) = test_router_with_hive_identity(identity.clone()).await;

    let join = crate::hive::roster::create_join_record(&other, "bob-phone", 1000);
    store
        .hive_upsert_roster_entry(&crate::hive::roster::RosterEntry {
            device_id: other.device_id.clone(),
            public_key: crate::hive::identity::public_key_hex(&other),
            name: "bob-phone".to_string(),
            status: crate::hive::roster::RosterStatus::Active,
            joined_at: 1000,
            revoked_at: None,
            revoked_by: None,
            join_record: join,
            revocation_record: None,
        })
        .await
        .unwrap();
    store
        .hive_upsert_peer_status(&other.device_id, true, Some(2000))
        .await
        .unwrap();

    let (status, body) = req(app, "GET", "/api/v1/hive/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], true);
    assert_eq!(body["identity"]["device_id"], identity.device_id);
    assert_eq!(body["sync_port"], 4570);
    let roster = body["roster"].as_array().unwrap();
    let entry = roster
        .iter()
        .find(|e| e["device_id"] == other.device_id)
        .expect("pushed roster entry must be present");
    assert_eq!(entry["status"], "active");
    assert_eq!(entry["online"], true);
    assert_eq!(entry["last_synced_at"], 2000);
}

#[tokio::test]
async fn hive_join_rejects_malformed_peer_public_key() {
    let identity = crate::hive::identity::generate();
    let (app, _store, _dir) = test_router_with_hive_identity(identity).await;
    let (status, _) = req(
        app,
        "POST",
        "/api/v1/hive/join",
        Some(json!({
            "peer_address": "127.0.0.1:1", "pairing_code": "ABCDEF",
            "peer_public_key": "not-hex",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn hive_join_reports_bad_gateway_when_peer_unreachable() {
    let identity = crate::hive::identity::generate();
    let target = crate::hive::identity::generate();
    let (app, _store, _dir) = test_router_with_hive_identity(identity).await;
    let (status, _) = req(
        app,
        "POST",
        "/api/v1/hive/join",
        Some(json!({
            // Port 1 on loopback: nothing listens there, so this exercises
            // the "could not reach peer" BAD_GATEWAY branch without needing
            // a real peer TLS listener.
            "peer_address": "127.0.0.1:1", "pairing_code": "ABCDEF",
            "peer_public_key": crate::hive::identity::public_key_hex(&target),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn pairing_code_endpoint_rejects_non_loopback_peer() {
    // Issuing a code returns everything needed to pair an attacker-owned
    // device into the hive (the code AND this device's public key) and opens
    // the pairing listener -- so on a non-loopback bind it must be as
    // unreachable to LAN callers as `hive_join`/revoke/enabled are.
    let identity = crate::hive::identity::generate();
    let (app, _store, _dir) = test_router_with_hive_identity(identity).await;
    let non_loopback =
        axum::extract::ConnectInfo(std::net::SocketAddr::from(([192, 168, 1, 50], 54321)));
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/hive/pairing-code")
        .extension(non_loopback)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(request).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn require_loopback_accepts_ipv4_mapped_ipv6_loopback() {
    // A 127.0.0.1 client arriving on a server bound to `::` shows up as
    // `::ffff:127.0.0.1`; the local dashboard must not be locked out of its
    // own hive controls by a dual-stack bind.
    let (app, _store, _dir) = test_router_with_store().await;
    let mapped = axum::extract::ConnectInfo(std::net::SocketAddr::from((
        std::net::Ipv4Addr::LOCALHOST.to_ipv6_mapped(),
        54321,
    )));
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/hive/trusted-networks")
        .extension(mapped)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(request).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn revoke_device_refuses_to_revoke_self() {
    let identity = crate::hive::identity::generate();
    let (app, store, _dir) = test_router_with_hive_identity(identity.clone()).await;
    let self_join = crate::hive::roster::create_join_record(&identity, "self", 1000);
    store
        .hive_upsert_roster_entry(&crate::hive::roster::RosterEntry {
            device_id: identity.device_id.clone(),
            public_key: crate::hive::identity::public_key_hex(&identity),
            name: "self".to_string(),
            status: crate::hive::roster::RosterStatus::Active,
            joined_at: 1000,
            revoked_at: None,
            revoked_by: None,
            join_record: self_join,
            revocation_record: None,
        })
        .await
        .unwrap();

    let (status, _) = req(
        app,
        "POST",
        &format!("/api/v1/hive/roster/{}/revoke", identity.device_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let roster = store.hive_list_roster().await.unwrap();
    assert_eq!(roster[0].status, crate::hive::roster::RosterStatus::Active);
}

#[tokio::test]
async fn hive_status_marks_this_devices_own_roster_entry() {
    let identity = crate::hive::identity::generate();
    let other = crate::hive::identity::generate();
    let (app, store, _dir) = test_router_with_hive_identity(identity.clone()).await;
    for (who, name) in [(&identity, "self"), (&other, "other")] {
        store
            .hive_upsert_roster_entry(&crate::hive::roster::RosterEntry {
                device_id: who.device_id.clone(),
                public_key: crate::hive::identity::public_key_hex(who),
                name: name.to_string(),
                status: crate::hive::roster::RosterStatus::Active,
                joined_at: 1000,
                revoked_at: None,
                revoked_by: None,
                join_record: crate::hive::roster::create_join_record(who, name, 1000),
                revocation_record: None,
            })
            .await
            .unwrap();
    }
    let (status, body) = req(app, "GET", "/api/v1/hive/status", None).await;
    assert_eq!(status, StatusCode::OK);
    let roster = body["roster"].as_array().unwrap();
    let flag_for = |id: &str| {
        roster.iter().find(|e| e["device_id"] == id).unwrap()["is_self"]
            .as_bool()
            .unwrap()
    };
    assert!(flag_for(&identity.device_id));
    assert!(!flag_for(&other.device_id));
}

#[tokio::test]
async fn set_hive_enabled_refuses_when_cloud_sync_is_enabled() {
    // config.toml already refuses [sync] + [hive]; the DB override must not
    // be a way around that.
    let (store, dir) = test_store().await;
    let (events, _) = broadcast::channel(16);
    let suggest = test_suggest_manager(Arc::clone(&store), dir.path(), events.clone());
    let app = router(
        Arc::clone(&store),
        SyncSettings {
            enabled: true,
            ..SyncSettings::default()
        },
        "http://127.0.0.1:3457",
        events,
        suggest,
        test_update_state(),
        test_agent_settings(),
        true,
        false,
        None,
        Arc::new(crate::hive::pairing::PairingCodeStore::new()),
        None,
        crate::api::HiveSyncPort(0),
    )
    .layer(axum::extract::Extension(Arc::new(
        tokio::sync::Notify::new(),
    )));

    let (status, _) = req(
        app,
        "POST",
        "/api/v1/hive/enabled",
        Some(json!({ "enabled": true })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(store.hive_enabled_override().await.unwrap(), None);
}

#[tokio::test]
async fn hive_pair_refuses_a_revoked_device_and_keeps_it_revoked() {
    let (store, _dir) = test_store().await;
    let pairing_codes = Arc::new(crate::hive::pairing::PairingCodeStore::new());
    let issued = pairing_codes.issue(chrono::Utc::now().timestamp());
    let app = hive_pairing_router(Arc::clone(&store), pairing_codes);

    let revoked = crate::hive::identity::generate();
    let join_record = crate::hive::roster::create_join_record(&revoked, "revoked-laptop", 1000);
    store
        .hive_upsert_roster_entry(&crate::hive::roster::RosterEntry {
            device_id: revoked.device_id.clone(),
            public_key: crate::hive::identity::public_key_hex(&revoked),
            name: "revoked-laptop".to_string(),
            status: crate::hive::roster::RosterStatus::Revoked,
            joined_at: 1000,
            revoked_at: Some(2000),
            revoked_by: Some("hive_someoneelse".to_string()),
            join_record: join_record.clone(),
            revocation_record: None,
        })
        .await
        .unwrap();

    let (status, _) = req(
        app,
        "POST",
        "/api/v1/hive/pair",
        Some(json!({
            "code": issued.code,
            "join_record": {
                "device_id": join_record.device_id, "public_key": join_record.public_key,
                "name": join_record.name, "joined_at": join_record.joined_at,
                "signature": join_record.signature,
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let roster = store.hive_list_roster().await.unwrap();
    assert_eq!(roster[0].status, crate::hive::roster::RosterStatus::Revoked);
}
