use super::*;

// --- session logs ---

#[derive(Deserialize)]
pub(super) struct SessionLogsParams {
    limit: Option<i64>,
}

pub(super) async fn list_session_logs(
    State(store): State<Store>,
    Query(p): Query<SessionLogsParams>,
) -> Result<Json<Value>, ApiError> {
    let limit = p.limit.unwrap_or(50).clamp(1, 200);
    let logs = store.list_session_logs(limit).await?;
    Ok(Json(json!({ "count": logs.len(), "logs": logs })))
}

pub(super) async fn server_status(
    State(store): State<Store>,
    Extension(org_store): Extension<OrgStore>,
    Extension(sync): Extension<SyncSettings>,
    Extension(org_sync): Extension<Option<SyncSettings>>,
    Extension(agent): Extension<crate::config::AgentSettings>,
) -> Result<Json<Value>, ApiError> {
    let count = store.count().await?;
    let last_synced_at = store
        .get_meta("last_synced_at")
        .await?
        .and_then(|v| v.parse::<i64>().ok());
    let conflict_count = store.pending_conflict_count().await?;

    let org = match &org_store {
        Some(org) => {
            let org_last_synced_at = match org.get_meta("last_synced_at").await {
                Ok(v) => v.and_then(|v| v.parse::<i64>().ok()),
                Err(e) => {
                    tracing::warn!("org store get_meta(last_synced_at) failed: {e:#}");
                    None
                }
            };
            let org_conflict_count = match org.pending_conflict_count().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("org store pending_conflict_count failed: {e:#}");
                    0
                }
            };
            let org_count = match org.count().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("org store count failed: {e:#}");
                    0
                }
            };
            json!({
                "configured": true,
                "enabled": org_sync.as_ref().is_some_and(|s| s.enabled),
                "last_synced_at": org_last_synced_at,
                "conflict_count": org_conflict_count,
                "count": org_count,
            })
        }
        None => json!({ "configured": false }),
    };

    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "memory_count": count,
        "db_path": crate::db::resolve_db_path(),
        "sync": {
            "enabled": sync.enabled,
            "last_synced_at": last_synced_at,
            "conflict_count": conflict_count,
        },
        "org": org,
        "agent": {
            "kind": agent.kind.as_str(),
            "command": agent.command,
        },
    })))
}
