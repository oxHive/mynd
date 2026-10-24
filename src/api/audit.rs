use super::*;

// --- audit sessions ---

#[derive(Deserialize)]
pub(super) struct StartAuditBody {
    project: String,
    #[serde(default)]
    layer: Option<String>,
}

pub(super) async fn start_audit_session(
    Extension(mgr): Extension<Arc<AuditSessionManager>>,
    Json(b): Json<StartAuditBody>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let project = b.project.trim().to_lowercase();
    if project.is_empty() {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "project must not be empty".into(),
        ));
    }
    let layer = b.layer.unwrap_or_else(|| "workspace".to_string());
    match mgr.start(project, layer).await {
        Ok(()) => Ok((StatusCode::ACCEPTED, Json(json!({ "started": true })))),
        Err(StartError::AlreadyActive) => Err(ApiError(
            StatusCode::CONFLICT,
            "an audit session is already active".into(),
        )),
        Err(StartError::Failed(msg)) => Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, msg)),
    }
}

pub(super) async fn audit_session_status(
    Extension(mgr): Extension<Arc<AuditSessionManager>>,
) -> Json<Value> {
    Json(mgr.status().await)
}

#[derive(Deserialize)]
pub(super) struct ReviseBody {
    proposal_id: String,
    feedback: String,
}

pub(super) async fn revise_audit_session(
    Extension(mgr): Extension<Arc<AuditSessionManager>>,
    Json(b): Json<ReviseBody>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    match mgr.revise(b.proposal_id.clone(), b.feedback).await {
        Ok(()) => Ok((
            StatusCode::ACCEPTED,
            Json(json!({ "queued": true, "proposal_id": b.proposal_id })),
        )),
        Err(ReviseError::NotActive) => Err(ApiError(
            StatusCode::CONFLICT,
            "no active audit session".into(),
        )),
        Err(ReviseError::UnknownProposal) => {
            Err(not_found(format!("no pending proposal {}", b.proposal_id)))
        }
    }
}

pub(super) async fn end_audit_session(
    Extension(mgr): Extension<Arc<AuditSessionManager>>,
) -> Json<Value> {
    mgr.end().await;
    Json(json!({ "ended": true }))
}

// --- audit proposals ---

#[derive(Deserialize)]
pub(super) struct ListProposalsParams {
    project: Option<String>,
    status: Option<String>,
}

pub(super) async fn list_audit_proposals(
    State(store): State<Store>,
    Query(p): Query<ListProposalsParams>,
) -> Result<Json<Value>, ApiError> {
    let project = p.project.map(|s| s.trim().to_lowercase());
    let status = p.status.as_deref().unwrap_or("pending");
    let proposals = store.list_proposals(project.as_deref(), status).await?;
    Ok(Json(json!({ "proposals": proposals })))
}

pub(super) async fn apply_audit_proposal(
    State(store): State<Store>,
    Extension(org_store): Extension<OrgStore>,
    Extension(events): Extension<Events>,
    Extension(hive): Extension<HivePushConfig>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let Some(p) = store.get_proposal(&id).await? else {
        return Err(not_found(format!("no proposal {id}")));
    };
    if p.status != "pending" {
        return Err(ApiError(
            StatusCode::CONFLICT,
            format!("proposal {id} is already {}", p.status),
        ));
    }
    if p.ops.is_empty() {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "this proposal is a report only; there is nothing to apply".into(),
        ));
    }
    let target = if p.layer == "org" {
        org_store.clone().ok_or_else(|| {
            ApiError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "org layer not configured: set [org_sync] in the global config".into(),
            )
        })?
    } else {
        store.clone()
    };
    let outcome = target
        .apply_proposal_ops(&p.ops, &p.base, &p.layer)
        .await
        .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;
    match outcome {
        crate::proposals::ApplyOutcome::Stale(ids) => Err(ApiError(
            StatusCode::CONFLICT,
            format!(
                "stale, re-run the audit: {} changed since this was proposed",
                ids.join(", ")
            ),
        )),
        crate::proposals::ApplyOutcome::Applied(touched) => {
            store.set_proposal_status(&id, "pending", "applied").await?;
            let _ = events.send(json!({ "type": "changed" }));
            let _ = events.send(json!({ "type": "audit_proposals" }));
            if p.layer != "org" {
                for t in &touched {
                    spawn_hive_push(&hive, &store, t);
                }
            }
            Ok(Json(
                json!({ "applied": true, "id": id, "touched": touched }),
            ))
        }
    }
}

pub(super) async fn reject_audit_proposal(
    State(store): State<Store>,
    Extension(events): Extension<Events>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if store
        .set_proposal_status(&id, "pending", "rejected")
        .await?
    {
        let _ = events.send(json!({ "type": "audit_proposals" }));
        Ok(Json(json!({ "rejected": true, "id": id })))
    } else if store.get_proposal(&id).await?.is_some() {
        Err(ApiError(
            StatusCode::CONFLICT,
            format!("proposal {id} is no longer pending"),
        ))
    } else {
        Err(not_found(format!("no proposal {id}")))
    }
}
