use crate::{
    config::{AgentSettings, SyncSettings},
    store::SqliteStore,
    suggest_session::{ReviseError, StartError, SuggestSessionManager},
    update::{SharedUpdateState, UpdateStatus},
};
use axum::{
    Json, Router,
    extract::{ConnectInfo, Extension, Path, Query, State},
    http::{Method, StatusCode, header},
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::{Stream, StreamExt, wrappers::BroadcastStream};
use tower_http::cors::{AllowOrigin, CorsLayer};

type Store = Arc<SqliteStore>;
type Events = broadcast::Sender<serde_json::Value>;

/// Whether predefined tag namespaces can be deleted/modified via
/// `save_tag_settings` — wrapped so it's a distinct Extension type rather
/// than a bare `bool`. See `config::ServerSettings::guard_predefined_namespaces`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GuardPredefinedNamespaces(pub bool);

/// The hive mTLS sync port (`settings.port + 1`), threaded through as its own
/// Extension type (not a bare `u16`) so `GET /api/v1/hive/status` can surface
/// it to the dashboard's invite-QR payload without ambiguity against any
/// other `u16` extension. `0` when hive was never enabled at boot (no port
/// was ever chosen) -- `hive_status` reports it as `null` in that case.
///
/// `pub` (not `pub(crate)` like `GuardPredefinedNamespaces`) because it appears
/// directly in `router()`'s public signature, so out-of-crate callers -- the
/// `tests/api_integration.rs` integration harness -- must be able to name and
/// construct it.
#[derive(Debug, Clone, Copy)]
pub struct HiveSyncPort(pub u16);

/// Whether push-on-change (Plan 2 Task 11) should fire after a successful
/// memory write, and the identity to sign/authenticate those pushes with.
/// `identity` is only `Some` when `enabled` is true (see `http::run_up`,
/// which bootstraps it once and threads it through here).
#[derive(Clone)]
pub(crate) struct HivePushConfig {
    pub enabled: bool,
    pub identity: Option<crate::hive::identity::DeviceIdentity>,
}

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

/// Rejects any request whose real TCP peer address isn't loopback, regardless
/// of what host the server itself bound to. Used to gate the trusted-networks
/// auto-pause control endpoints (see issue #27): they live on the plaintext
/// app router alongside routes that are only ever safe when bound to
/// loopback, but auto-pause specifically protects a device that may have
/// bound to a wider host -- so it must not be flippable by whoever it's
/// meant to defend against. Reads the real peer address via `ConnectInfo`
/// (populated by `into_make_service_with_connect_info`, not a spoofable
/// header), so this can't be bypassed with a forged `X-Forwarded-For` or
/// `Host` header.
async fn require_loopback(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: axum::extract::Request,
    next: Next,
) -> Result<Response, ApiError> {
    // `to_canonical()` folds an IPv4-mapped IPv6 peer (`::ffff:127.0.0.1`,
    // which is how a 127.0.0.1 client shows up when the server bound `::`)
    // back to plain IPv4, so a dual-stack bind doesn't lock the local
    // dashboard out of its own hive controls. It fails closed for
    // everything else.
    if addr.ip().to_canonical().is_loopback() {
        Ok(next.run(req).await)
    } else {
        Err(ApiError(
            StatusCode::FORBIDDEN,
            "this endpoint is only reachable from localhost".to_string(),
        ))
    }
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
            "http://127.0.0.1:3459",
        ))
    } else {
        AllowOrigin::list(origins)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn router(
    store: Store,
    sync: SyncSettings,
    dashboard_origin: &str,
    events: Events,
    suggest: Arc<SuggestSessionManager>,
    update_state: SharedUpdateState,
    agent: AgentSettings,
    guard_predefined_namespaces: bool,
    hive_enabled: bool,
    hive_identity: Option<crate::hive::identity::DeviceIdentity>,
    pairing_codes: Arc<crate::hive::pairing::PairingCodeStore>,
    pairing_window: Option<Arc<crate::hive::pairing_window::PairingWindow>>,
    hive_sync_port: HiveSyncPort,
) -> Router {
    let hive_push_config = HivePushConfig {
        enabled: hive_enabled,
        identity: hive_identity.clone(),
    };
    let identity_extension = hive_identity.map(Arc::new);
    let router = Router::new()
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
        .with_state(store.clone())
        .layer(Extension(sync.clone()))
        .layer(Extension(events))
        .layer(Extension(suggest))
        .layer(Extension(update_state))
        .layer(Extension(agent))
        .layer(Extension(GuardPredefinedNamespaces(
            guard_predefined_namespaces,
        )))
        .layer(Extension(hive_push_config.clone()))
        .layer(Extension(hive_sync_port));
    let router = if let Some(identity) = identity_extension.clone() {
        router.layer(Extension(identity))
    } else {
        router
    };
    let router = router.layer(
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
    );

    // Merged in as its own sub-router (rather than chained into the routes
    // above) so `require_loopback` -- via `route_layer`, which applies to
    // every route already registered in the *same* router value -- only
    // gates these two routes, not the entire plaintext API (see issue #27).
    // `hive_revoke_device` needs `Extension<Arc<DeviceIdentity>>` to sign the
    // revocation. The main router's `.layer(...)` extensions don't propagate
    // across `merge` into this separately-built sub-router (same finding as
    // `hive_status`'s push-config/sync-port below), so the identity is
    // re-supplied directly onto this router. The first `identity_extension`
    // local was already cloned onto the main `router` above; this reuses the
    // same clone.
    let identity_extension_for_trusted_networks = identity_extension;
    let trusted_networks_router = Router::new()
        // Loopback-gated: `hive_join` dials an attacker-choosable
        // `peer_address`/`peer_public_key` and merges whatever roster the
        // response claims -- reachable only from the local user's own
        // dashboard, never a remote peer (mirrors the other hive-sensitive
        // routes on this sub-router; see issue #27 and the security review
        // that flagged this route being left off this gate).
        .route("/api/v1/hive/join", post(hive_join))
        // Loopback-gated: issuing a code both opens the pairing listener and
        // returns the code *and* this device's public key -- everything a
        // caller needs to pair a device of their own into the hive and pull
        // every memory from every member. On a non-loopback bind this must
        // not be reachable by whoever can reach the plaintext API (the same
        // reasoning as `hive_join`/revoke/enabled above); only the local
        // user's dashboard issues invites. Issuing a code opens the
        // time-limited pairing-window listener (see PairingWindow).
        .route("/api/v1/hive/pairing-code", post(hive_issue_pairing_code))
        .route(
            "/api/v1/hive/trusted-networks",
            get(hive_get_trusted_networks).post(hive_add_trusted_network),
        )
        .route(
            "/api/v1/hive/trusted-networks/{id}",
            axum::routing::delete(hive_remove_trusted_network),
        )
        .route("/api/v1/hive/status", get(hive_status))
        .route(
            "/api/v1/hive/roster/{device_id}/revoke",
            post(hive_revoke_device),
        )
        // Loopback-gated (dashboard-only) live enable/disable toggle: it
        // persists the DB override (Task 3) and signals `run_up`'s restart
        // race via the `Arc<tokio::sync::Notify>` Extension. That Extension is
        // deliberately NOT layered here on the sub-router (nor anywhere inside
        // `router()`): the restart lifecycle belongs to the process owner
        // (`http::app_router`/`run_up`), which layers it on the fully-composed
        // outer app. Layering it inside `router()` would make it an innermost
        // Extension that shadows any outer one a caller supplies -- see
        // `api::tests::set_hive_enabled_persists_override_and_signals_restart`,
        // which layers its own Notify to observe the signal.
        .route("/api/v1/hive/enabled", post(hive_set_enabled))
        .route_layer(middleware::from_fn(require_loopback))
        // `hive_status` needs the hive push-config (for its identity/enabled
        // state) and the sync port; the main router's `.layer(...)` extensions
        // don't propagate across `merge` into this separately-built sub-router,
        // so they're re-supplied here. Likewise `hive_set_enabled` needs the
        // cloud-sync settings (to refuse enabling hive alongside [sync]) and
        // `hive_issue_pairing_code` needs the code store.
        .layer(Extension(hive_push_config))
        .layer(Extension(hive_sync_port))
        .layer(Extension(sync))
        .layer(Extension(pairing_codes))
        .with_state(store);
    let trusted_networks_router = if let Some(identity) = identity_extension_for_trusted_networks {
        trusted_networks_router.layer(Extension(identity))
    } else {
        trusted_networks_router
    };
    // The pairing-code issue handler needs the window coordinator; it's only
    // present when hive is enabled (built in `http::run_up`). When absent, the
    // route still exists but returns 500 if hit — acceptable, since issuing a
    // pairing code is meaningless with hive disabled.
    let trusted_networks_router = if let Some(pairing_window) = pairing_window {
        trusted_networks_router.layer(Extension(pairing_window))
    } else {
        trusted_networks_router
    };

    router.merge(trusted_networks_router)
}

/// Server-TLS-only (no client-cert verifier) — this is where a brand-new,
/// not-yet-in-anyone's-roster device pairs. Bound only for a limited window
/// (see `hive::pairing_window::PairingWindow`, opened per pairing code
/// issued), never always-on, since it has no roster-membership check of its
/// own. Never merged into the plaintext `router()`.
pub fn hive_pairing_router(
    store: Store,
    pairing_codes: Arc<crate::hive::pairing::PairingCodeStore>,
) -> Router {
    Router::new()
        .route("/api/v1/hive/pair", post(hive_pair))
        .with_state(store)
        .layer(Extension(pairing_codes))
}

/// Mandatory mutual-TLS (client cert must match an `Active` roster member) —
/// every route here assumes the caller is already a paired hive member.
/// Always bound while hive is enabled (this is the mDNS-advertised port real
/// peer sync uses continuously); never merged into the plaintext `router()`.
pub fn hive_sync_router(store: Store) -> Router {
    Router::new()
        .route("/api/v1/hive/roster", get(hive_roster))
        .route("/api/v1/hive/manifest", get(hive_manifest))
        .route("/api/v1/hive/memories/{id}", get(hive_get_memory))
        .route("/api/v1/hive/settings", get(hive_get_settings))
        .route("/api/v1/hive/tag-namespaces", get(hive_get_tag_namespaces))
        .route("/api/v1/hive/push", post(hive_push))
        .with_state(store)
}

async fn sse_events(
    Extension(events): Extension<Events>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let stream = BroadcastStream::new(events.subscribe())
        .filter_map(|msg| msg.ok().map(|v| Ok(Event::default().data(v.to_string()))));
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Spawns a best-effort push-on-change attempt (Plan 2 Task 11) after a
/// successful store()/update()/add_tags()/remove_tags() call, if Hive Mode
/// is enabled. Spawned rather than awaited so a slow/unreachable peer never
/// adds latency to the user-facing write response.
pub(crate) fn spawn_hive_push(hive: &HivePushConfig, store: &Store, memory_id: &str) {
    if hive.enabled
        && let Some(identity) = hive.identity.clone()
    {
        tokio::spawn(crate::hive::sync_loop::push_memory_change_to_online_peers(
            store.clone(),
            identity,
            memory_id.to_string(),
        ));
    }
}

/// Best-effort push of a just-changed tag-namespace registry to online peers
/// (Finding I3), mirroring `spawn_hive_push` for memories. Spawned, never
/// awaited, so a slow/unreachable peer never delays the settings-save response.
pub(crate) fn spawn_hive_tag_namespaces_push(hive: &HivePushConfig, store: &Store) {
    if hive.enabled
        && let Some(identity) = hive.identity.clone()
    {
        tokio::spawn(
            crate::hive::sync_loop::push_tag_namespaces_change_to_online_peers(
                store.clone(),
                identity,
            ),
        );
    }
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
mod hive;
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
use hive::*;
use memories::*;
use settings::*;
// Shared with the hive receive paths (`hive_push`, `sync_loop::pull_from_peer`)
// so a peer-supplied tag-namespace registry is held to the same rules as a
// dashboard save.
pub(crate) use settings::validate_tag_namespaces;
use status::*;
use suggest::*;
use transfer::*;
use update::*;
