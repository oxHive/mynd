use super::*;

// --- export / import ---

pub(super) async fn export(State(store): State<Store>) -> Result<Json<Value>, ApiError> {
    let memories = store.list_memories(100_000, 0).await?;
    let edges = store.list_edges(None).await?;
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "exported_at": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        "memories": memories.iter().map(entry_json).collect::<Vec<_>>(),
        "edges": edges,
    })))
}

#[derive(Deserialize)]
pub(super) struct ImportBody {
    #[serde(default)]
    memories: Vec<ImportMemory>,
    #[serde(default)]
    edges: Vec<ImportEdge>,
}

#[derive(Deserialize)]
pub(super) struct ImportMemory {
    id: String,
    title: String,
    content: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    token_count: Option<i64>,
    #[serde(default = "default_layer")]
    layer: String,
    #[serde(default = "default_memory_type")]
    memory_type: String,
}

fn default_layer() -> String {
    "workspace".into()
}

fn default_memory_type() -> String {
    "project".into()
}

#[derive(Deserialize)]
pub(super) struct ImportEdge {
    source_id: String,
    target_id: String,
    relationship: String,
    #[serde(default = "default_edge_status")]
    status: String,
    #[serde(default)]
    link_text: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

fn default_edge_status() -> String {
    "active".into()
}

/// Checks every memory in an import the way `store()` will, before any
/// row is written, so a bad export is rejected whole (422 naming the
/// offending ids) instead of leaving a half-applied import behind.
pub(crate) async fn validate_import_memories<'a>(
    store: &crate::store::SqliteStore,
    memories: impl Iterator<Item = (&'a str, &'a str, &'a str, &'a str, &'a str, &'a [String])>,
) -> Result<(), String> {
    let memories: Vec<_> = memories.collect();
    // Ids this batch rewrites without role:root. Import stores them first, so
    // their old root claims are gone before any new claim lands.
    let released: std::collections::HashSet<&str> = memories
        .iter()
        .filter(|m| crate::store::root_scope(m.5).is_none())
        .map(|m| m.0)
        .collect();
    let mut problems = Vec::new();
    // Root scopes claimed earlier in this batch, so two imported roots for one
    // project are caught before either is written.
    let mut batch_roots: std::collections::HashMap<Option<String>, &str> =
        std::collections::HashMap::new();
    for (id, title, content, layer, memory_type, tags) in memories {
        if let Err(e) = layer.parse::<crate::model::Layer>() {
            problems.push(format!("{id}: {e}"));
        }
        if let Err(e) = memory_type.parse::<crate::model::MemoryType>() {
            problems.push(format!("{id}: {e}"));
        }
        if let Err(e) = store.check_content_size(title, content).await {
            problems.push(format!("{id}: {e}"));
        }
        if let Err(e) = store.check_root_unique(id, tags).await {
            // ponytail: looks at the oldest existing root only; a scope with
            // sync-duplicated roots can still fail mid-import.
            let moved = match crate::store::root_scope(tags) {
                Some(scope) => store
                    .find_root(scope.as_deref())
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|r| released.contains(r.id.as_str())),
                None => false,
            };
            if !moved {
                problems.push(format!("{id}: {e}"));
            }
        }
        if let Some(scope) = crate::store::root_scope(tags) {
            match batch_roots.get(&scope) {
                Some(first) if *first != id => problems.push(format!(
                    "{id}: this import already has a root for that scope ({first})"
                )),
                _ => {
                    batch_roots.insert(scope, id);
                }
            }
        }
        if problems.len() >= 10 {
            problems.push("…".to_string());
            break;
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "import rejected, nothing was written: {}",
            problems.join("; ")
        ))
    }
}

pub(super) async fn import(
    State(store): State<Store>,
    Json(mut b): Json<ImportBody>,
) -> Result<Json<Value>, ApiError> {
    validate_import_memories(
        &store,
        b.memories.iter().map(|m| {
            (
                m.id.as_str(),
                m.title.as_str(),
                m.content.as_str(),
                m.layer.as_str(),
                m.memory_type.as_str(),
                m.tags.as_slice(),
            )
        }),
    )
    .await
    .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e))?;
    // Non-roots first, so a batch that moves a root claim drops the old one
    // before the new one is checked.
    b.memories
        .sort_by_key(|m| crate::store::root_scope(&m.tags).is_some());
    let mut mem_count = 0usize;
    for m in &b.memories {
        store
            .store(&crate::store::NewMemoryRow {
                id: &m.id,
                title: &m.title,
                content: &m.content,
                tags: &m.tags,
                token_count: m.token_count,
                layer: &m.layer,
                memory_type: &m.memory_type,
            })
            .await?;
        mem_count += 1;
    }
    let mut edge_count = 0usize;
    for e in &b.edges {
        if !["active", "pending", "rejected"].contains(&e.status.as_str()) {
            continue;
        }
        if matches!(
            store
                .create_edge_with_status(
                    &e.source_id,
                    &e.target_id,
                    &e.relationship,
                    &e.status,
                    e.link_text.as_deref(),
                    e.reason.as_deref(),
                )
                .await?,
            crate::model::EdgeCreate::Created(_)
        ) {
            edge_count += 1;
        }
    }
    Ok(Json(
        json!({ "imported_memories": mem_count, "imported_edges": edge_count }),
    ))
}
