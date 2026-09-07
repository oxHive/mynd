use crate::{
    config::{AgentSettings, SyncSettings},
    store::SqliteStore,
    suggest_session::{ReviseError, StartError, SuggestSessionManager},
    update::{SharedUpdateState, UpdateStatus},
};
use axum::{
    Json, Router,
    extract::{Extension, Path, Query, State},
    http::{Method, StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::{Stream, StreamExt, wrappers::BroadcastStream};
use tower_http::cors::{AllowOrigin, CorsLayer};

type Store = Arc<SqliteStore>;
type Events = broadcast::Sender<serde_json::Value>;
pub(crate) type OrgStore = Option<Arc<SqliteStore>>;

/// Whether predefined tag namespaces can be deleted/modified via
/// `save_tag_settings` — wrapped so it's a distinct Extension type rather
/// than a bare `bool`. See `config::ServerSettings::guard_predefined_namespaces`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GuardPredefinedNamespaces(pub bool);

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

fn not_found(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, msg.into())
}

/// Tries the primary store first, then org_store — mirrors
/// `HiveMind::find_owning_store` in `src/server.rs`. An org-store lookup
/// failure degrades to "not found in org" (never propagates), consistent
/// with this plan's Global Constraint that org failures never break
/// primary-store behavior.
pub(super) async fn find_owning_store<'a>(
    store: &'a Store,
    org_store: &'a OrgStore,
    id: &str,
) -> Result<Option<&'a Store>, ApiError> {
    if store.recall_by_id(id).await?.is_some() {
        return Ok(Some(store));
    }
    if let Some(org) = org_store {
        match org.recall_by_id(id).await {
            Ok(Some(_)) => return Ok(Some(org)),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(
                    "org store lookup failed for id {id}: {e:#}; treating as not found in org"
                );
            }
        }
    }
    Ok(None)
}

/// Returns an `AllowOrigin` that accepts both the configured dashboard origin and
/// its `localhost` / `127.0.0.1` counterpart, so the browser CORS check passes
/// regardless of which loopback hostname the user typed.
fn localhost_origins(origin: &str) -> AllowOrigin {
    let mut origins: Vec<axum::http::HeaderValue> = Vec::new();

    if let Ok(v) = origin.parse::<axum::http::HeaderValue>() {
        origins.push(v);
    }

    // Add the `localhost` ↔ `127.0.0.1` sibling so both hostnames are accepted.
    let sibling = if origin.contains("127.0.0.1") {
        origin.replace("127.0.0.1", "localhost")
    } else if origin.contains("localhost") {
        origin.replace("localhost", "127.0.0.1")
    } else {
        String::new()
    };
    if !sibling.is_empty()
        && let Ok(v) = sibling.parse::<axum::http::HeaderValue>()
    {
        origins.push(v);
    }

    if origins.is_empty() {
        AllowOrigin::exact(axum::http::HeaderValue::from_static(
            "http://127.0.0.1:3457",
        ))
    } else {
        AllowOrigin::list(origins)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn router(
    store: Store,
    org_store: OrgStore,
    sync: SyncSettings,
    org_sync: Option<SyncSettings>,
    dashboard_origin: &str,
    events: Events,
    suggest: Arc<SuggestSessionManager>,
    update_state: SharedUpdateState,
    agent: AgentSettings,
    guard_predefined_namespaces: bool,
) -> Router {
    Router::new()
        .route("/api/v1/memories", get(list_memories).post(create_memory))
        .route("/api/v1/memories/count-tokens", post(count_tokens))
        .route(
            "/api/v1/memories/all",
            axum::routing::delete(delete_all_memories),
        )
        .route(
            "/api/v1/memories/{id}",
            get(get_memory).patch(patch_memory).delete(delete_memory),
        )
        .route("/api/v1/memories/{id}/tags/add", post(add_memory_tags))
        .route(
            "/api/v1/memories/{id}/tags/remove",
            post(remove_memory_tags),
        )
        .route("/api/v1/export", get(export))
        .route("/api/v1/import", post(import))
        .route("/api/v1/search", get(search))
        .route("/api/v1/edges", get(list_edges).post(create_edge))
        .route("/api/v1/edges/{id}", axum::routing::patch(patch_edge))
        .route("/api/v1/feedback", get(list_feedback).post(create_feedback))
        .route(
            "/api/v1/feedback/{id}",
            axum::routing::patch(patch_feedback),
        )
        .route("/api/v1/conflicts", get(list_conflicts))
        .route(
            "/api/v1/conflicts/{id}/resolve",
            post(resolve_conflict_handler),
        )
        .route(
            "/api/v1/settings/sync",
            get(get_sync_settings).post(save_sync_settings),
        )
        .route(
            "/api/v1/settings/tags",
            get(get_tag_settings).post(save_tag_settings),
        )
        .route(
            "/api/v1/settings/content-limits",
            get(get_content_limit_settings).post(save_content_limit_settings),
        )
        .route("/api/v1/session-logs", get(list_session_logs))
        .route("/api/v1/status", get(server_status))
        .route("/api/v1/events", get(sse_events))
        .route("/api/v1/update", get(get_update_state))
        .route("/api/v1/update/apply", post(apply_update))
        .route("/api/v1/suggest-sessions", post(start_suggest_session))
        .route(
            "/api/v1/suggest-sessions/current",
            get(suggest_session_status).delete(end_suggest_session),
        )
        .route(
            "/api/v1/suggest-sessions/current/revise",
            post(revise_suggest_session),
        )
        .with_state(store)
        .layer(Extension(org_store))
        .layer(Extension(sync))
        .layer(Extension(org_sync))
        .layer(Extension(events))
        .layer(Extension(suggest))
        .layer(Extension(update_state))
        .layer(Extension(agent))
        .layer(Extension(GuardPredefinedNamespaces(
            guard_predefined_namespaces,
        )))
        .layer(
            CorsLayer::new()
                .allow_origin(localhost_origins(dashboard_origin))
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PATCH,
                    Method::DELETE,
                    Method::OPTIONS,
                ])
                .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]),
        )
}

async fn sse_events(
    Extension(events): Extension<Events>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let stream = BroadcastStream::new(events.subscribe())
        .filter_map(|msg| msg.ok().map(|v| Ok(Event::default().data(v.to_string()))));
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn entry_json(e: &crate::store::MemoryEntry) -> Value {
    json!({
        "id": e.id,
        "title": e.title,
        "content": e.content,
        "tags": e.tags,
        "created_at": e.created_at,
        "updated_at": e.updated_at,
        "token_count": e.token_count,
        "layer": e.layer,
        "memory_type": e.memory_type,
    })
}

mod edges;
mod feedback;
mod memories;
mod settings;
mod status;
mod suggest;
#[cfg(test)]
mod tests;
mod transfer;
mod update;

use edges::*;
use feedback::*;
use memories::*;
use settings::*;
use status::*;
use suggest::*;
use transfer::*;
use update::*;
