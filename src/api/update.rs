use super::*;

pub(super) async fn get_update_state(
    Extension(update_state): Extension<SharedUpdateState>,
) -> Result<Json<Value>, ApiError> {
    let s = update_state.read().await;
    Ok(Json(
        serde_json::to_value(&*s).unwrap_or_else(|_| json!({})),
    ))
}

/// Applying an update replaces the running binary and re-execs the
/// process, so it must never be reachable by accident. The JSON body
/// (`{"confirm": true}`) means a browser can only send this with a CORS
/// preflight, which the allowlist rejects for foreign origins; the
/// `api::guard` Origin check is the second line.
#[derive(Deserialize)]
pub(super) struct ApplyBody {
    #[serde(default)]
    confirm: bool,
}

pub(super) async fn apply_update(
    Extension(update_state): Extension<SharedUpdateState>,
    Extension(events): Extension<Events>,
    Json(body): Json<ApplyBody>,
) -> Result<Json<Value>, ApiError> {
    if !body.confirm {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "applying an update requires {\"confirm\": true}".into(),
        ));
    }
    {
        let mut s = update_state.write().await;
        if !s.apply_enabled {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "self-update via the API is disabled ([update] allow_apply_from_api = false); \
                 run `mynd update apply` instead"
                    .into(),
            ));
        }
        if s.status == UpdateStatus::Updating {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "update already in progress".into(),
            ));
        }
        s.status = UpdateStatus::Updating;
        s.update_started_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        );
        s.error = None;
    }
    let _ = events.send(json!({
        "type": "update_progress",
        "status": "updating",
        "started_at": update_state.read().await.update_started_at,
    }));

    let state = update_state.clone();
    let events2 = events.clone();
    tokio::spawn(async move {
        crate::update::run_update(state, events2).await;
    });

    Ok(Json(json!({"status": "updating"})))
}
