use super::*;

// --- memories ---

#[derive(Deserialize)]
pub(super) struct ListMemoriesParams {
    limit: Option<i64>,
    offset: Option<i64>,
}

pub(super) async fn list_memories(
    State(store): State<Store>,
    Extension(org_store): Extension<OrgStore>,
    Query(p): Query<ListMemoriesParams>,
) -> Result<Json<Value>, ApiError> {
    let limit = p.limit.unwrap_or(200).clamp(1, 1000);
    let offset = p.offset.unwrap_or(0).max(0);
    let mut entries = store.list_memories(limit, offset).await?;
    if let Some(org) = &org_store {
        match org.list_memories(limit, offset).await {
            Ok(mut org_entries) => entries.append(&mut org_entries),
            Err(e) => tracing::warn!("org store list_memories failed: {e:#}"),
        }
    }
    Ok(Json(json!({
        "count": entries.len(),
        "memories": entries.iter().map(entry_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub(super) struct CountTokensBody {
    #[serde(default)]
    title: String,
    #[serde(default)]
    content: String,
}

/// Live token count for the dashboard's memory editor, using the exact same
/// counting the memory_store/memory_update guardrail enforces — read-only,
/// no store write, so the UI can show the count while the user is still
/// typing.
pub(super) async fn count_tokens(
    State(store): State<Store>,
    Json(body): Json<CountTokensBody>,
) -> Json<Value> {
    let tokens = crate::budget::count_entry_tokens(&body.title, &body.content);
    Json(json!({
        "tokens": tokens,
        "max_content_tokens": store.max_content_tokens().await,
    }))
}

#[derive(Deserialize)]
pub(super) struct CreateMemoryBody {
    title: String,
    content: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    token_count: Option<i64>,
    #[serde(default)]
    layer: Option<String>,
    #[serde(default)]
    memory_type: Option<String>,
}

pub(super) async fn create_memory(
    State(store): State<Store>,
    Extension(org_store): Extension<OrgStore>,
    Extension(events): Extension<Events>,
    Json(b): Json<CreateMemoryBody>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let layer = match &b.layer {
        Some(l) => l
            .parse::<crate::model::Layer>()
            .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?
            .to_string(),
        None => "workspace".to_string(),
    };
    let memory_type = match &b.memory_type {
        Some(t) => t
            .parse::<crate::model::MemoryType>()
            .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?
            .to_string(),
        None => "project".to_string(),
    };
    let target_store = if layer == "org" {
        org_store.as_ref().ok_or_else(|| {
            ApiError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "org layer not configured — set [org_sync] in the global config".to_string(),
            )
        })?
    } else {
        &store
    };
    let id = format!("mem_{}", uuid::Uuid::new_v4().simple());
    target_store
        .store(&crate::store::NewMemoryRow {
            id: &id,
            title: &b.title,
            content: &b.content,
            tags: &b.tags,
            token_count: b.token_count,
            layer: &layer,
            memory_type: &memory_type,
        })
        .await?;
    let _ = events.send(json!({ "type": "changed" }));
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

pub(super) async fn get_memory(
    State(store): State<Store>,
    Extension(org_store): Extension<OrgStore>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let owning = find_owning_store(&store, &org_store, &id).await?;
    match owning {
        None => Err(not_found(format!("no memory {id}"))),
        Some(s) => match s.recall_by_id(&id).await? {
            None => Err(not_found(format!("no memory {id}"))),
            Some(e) => Ok(Json(entry_json(&e))),
        },
    }
}

#[derive(Deserialize)]
pub(super) struct PatchMemoryBody {
    title: Option<String>,
    content: Option<String>,
    tags: Option<Vec<String>>,
}

pub(super) async fn patch_memory(
    State(store): State<Store>,
    Extension(org_store): Extension<OrgStore>,
    Extension(events): Extension<Events>,
    Path(id): Path<String>,
    Json(b): Json<PatchMemoryBody>,
) -> Result<Json<Value>, ApiError> {
    let owning = find_owning_store(&store, &org_store, &id)
        .await?
        .ok_or_else(|| not_found(format!("no memory {id}")))?;
    let current = owning
        .recall_by_id(&id)
        .await?
        .ok_or_else(|| not_found(format!("no memory {id}")))?;
    let title = b.title.as_deref().unwrap_or(&current.title);
    let content = b.content.as_deref().unwrap_or(&current.content);
    let tags = b.tags.as_deref().unwrap_or(&current.tags);
    let updated = owning.update(&id, title, content, tags).await?;
    if !updated {
        return Err(not_found(format!("no memory {id}")));
    }
    let entry = owning
        .recall_by_id(&id)
        .await?
        .ok_or_else(|| not_found(format!("no memory {id}")))?;
    let _ = events.send(json!({ "type": "changed" }));
    Ok(Json(entry_json(&entry)))
}

#[derive(Deserialize)]
pub(super) struct TagsBody {
    tags: Vec<String>,
}

pub(super) async fn add_memory_tags(
    State(store): State<Store>,
    Extension(org_store): Extension<OrgStore>,
    Extension(events): Extension<Events>,
    Path(id): Path<String>,
    Json(b): Json<TagsBody>,
) -> Result<Json<Value>, ApiError> {
    let owning = find_owning_store(&store, &org_store, &id)
        .await?
        .ok_or_else(|| not_found(format!("no memory {id}")))?;
    if !owning.add_tags(&id, &b.tags).await? {
        return Err(not_found(format!("no memory {id}")));
    }
    let entry = owning
        .recall_by_id(&id)
        .await?
        .ok_or_else(|| not_found(format!("no memory {id}")))?;
    let _ = events.send(json!({ "type": "changed" }));
    Ok(Json(entry_json(&entry)))
}

pub(super) async fn remove_memory_tags(
    State(store): State<Store>,
    Extension(org_store): Extension<OrgStore>,
    Extension(events): Extension<Events>,
    Path(id): Path<String>,
    Json(b): Json<TagsBody>,
) -> Result<Json<Value>, ApiError> {
    let owning = find_owning_store(&store, &org_store, &id)
        .await?
        .ok_or_else(|| not_found(format!("no memory {id}")))?;
    if !owning.remove_tags(&id, &b.tags).await? {
        return Err(not_found(format!("no memory {id}")));
    }
    let entry = owning
        .recall_by_id(&id)
        .await?
        .ok_or_else(|| not_found(format!("no memory {id}")))?;
    let _ = events.send(json!({ "type": "changed" }));
    Ok(Json(entry_json(&entry)))
}

pub(super) async fn delete_memory(
    State(store): State<Store>,
    Extension(org_store): Extension<OrgStore>,
    Extension(events): Extension<Events>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let owning = find_owning_store(&store, &org_store, &id)
        .await?
        .ok_or_else(|| not_found(format!("no memory {id}")))?;
    if !owning.delete(&id).await? {
        return Err(not_found(format!("no memory {id}")));
    }
    let _ = events.send(json!({ "type": "changed" }));
    Ok(Json(json!({ "deleted": true, "id": id })))
}

pub(super) async fn delete_all_memories(
    State(store): State<Store>,
    Extension(events): Extension<Events>,
) -> Result<Json<Value>, ApiError> {
    let deleted = store.delete_all().await?;
    let _ = events.send(json!({ "type": "changed" }));
    Ok(Json(json!({ "deleted": deleted })))
}

// --- search ---

#[derive(Deserialize)]
pub(super) struct SearchParams {
    q: String,
    limit: Option<i64>,
}

pub(super) async fn search(
    State(store): State<Store>,
    Extension(org_store): Extension<OrgStore>,
    Query(p): Query<SearchParams>,
) -> Result<Json<Value>, ApiError> {
    let limit = p.limit.unwrap_or(20).clamp(1, 50);
    let mut hits = if crate::tag_query::looks_like_tag_expr(&p.q) {
        let expr = crate::tag_query::parse(&p.q)
            .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;
        let mut matches = store.find_by_tag_expr(&expr).await?;
        matches.truncate(limit as usize);
        matches
    } else {
        store.search(&p.q, limit).await?
    };
    if hits.len() < limit as usize
        && let Some(org) = &org_store
    {
        let remaining = limit - hits.len() as i64;
        let org_hits = if crate::tag_query::looks_like_tag_expr(&p.q) {
            let expr = crate::tag_query::parse(&p.q)
                .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;
            org.find_by_tag_expr(&expr).await
        } else {
            org.search(&p.q, remaining).await
        };
        match org_hits {
            Ok(mut org_hits) => {
                org_hits.truncate(remaining as usize);
                hits.append(&mut org_hits);
            }
            Err(e) => tracing::warn!("org store search failed: {e:#}"),
        }
    }
    let results: Vec<_> = hits.iter().map(entry_json).collect();
    Ok(Json(json!({ "count": results.len(), "results": results })))
}
