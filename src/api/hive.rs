use super::*;
use crate::hive::pairing::PairingCodeStore;
use crate::hive::roster::{JoinRecord, RosterEntry, RosterStatus, verify_join_record};
use std::sync::Arc;

#[derive(Deserialize)]
pub(super) struct PairRequestBody {
    code: String,
    join_record: JoinRecord,
}

pub(super) async fn hive_pair(
    State(store): State<Store>,
    Extension(pairing_codes): Extension<Arc<PairingCodeStore>>,
    Json(body): Json<PairRequestBody>,
) -> Result<Json<Value>, ApiError> {
    // Check the (stateless) signature before consuming the (single-use)
    // code, so a well-meaning joiner that sent a malformed record doesn't
    // burn the code and force the inviter to issue a fresh one.
    if !verify_join_record(&body.join_record) {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "join record signature invalid".to_string(),
        ));
    }
    let now = chrono::Utc::now().timestamp();
    if !pairing_codes.validate_and_consume(&body.code, now) {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "invalid or expired pairing code".to_string(),
        ));
    }

    let new_entry = RosterEntry {
        device_id: body.join_record.device_id.clone(),
        public_key: body.join_record.public_key.clone(),
        name: body.join_record.name.clone(),
        status: RosterStatus::Active,
        joined_at: body.join_record.joined_at,
        revoked_at: None,
        revoked_by: None,
        join_record: body.join_record.clone(),
        revocation_record: None,
    };
    let merged = store.hive_merge_roster(vec![new_entry]).await?;

    // `merge_roster` never un-revokes: a previously revoked device that
    // somehow obtained a fresh pairing code stays Revoked. Tell it so,
    // rather than handing back a roster that reads as a successful join.
    let joined_active = merged
        .iter()
        .any(|e| e.device_id == body.join_record.device_id && e.status == RosterStatus::Active);
    if !joined_active {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "this device has been revoked from the hive and cannot re-pair".to_string(),
        ));
    }

    Ok(Json(json!({ "roster": merged })))
}

#[derive(Deserialize)]
pub(super) struct JoinHiveBody {
    peer_address: String,
    pairing_code: String,
    // Obtained out-of-band alongside the pairing code itself (e.g. shown next
    // to the code on the peer's own screen) -- see issue #26. Pinning the
    // pairing TLS connection to this key, rather than accepting any
    // certificate, closes the on-path MITM gap in the original blind-trust
    // bootstrap: an attacker impersonating the peer during pairing now fails
    // the TLS handshake instead of being silently trusted.
    peer_public_key: String,
}

pub(super) async fn hive_join(
    State(store): State<Store>,
    Extension(identity): Extension<Arc<crate::hive::identity::DeviceIdentity>>,
    Json(body): Json<JoinHiveBody>,
) -> Result<Json<Value>, ApiError> {
    let join_record = crate::hive::roster::create_join_record(
        &identity,
        &identity.device_id,
        chrono::Utc::now().timestamp(),
    );
    let pair_body = json!({
        "code": body.pairing_code,
        "join_record": {
            "device_id": join_record.device_id, "public_key": join_record.public_key,
            "name": join_record.name, "joined_at": join_record.joined_at, "signature": join_record.signature,
        }
    });

    let target_public_key: [u8; 32] = hex::decode(&body.peer_public_key)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| {
            ApiError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "peer_public_key must be 32 bytes of hex".to_string(),
            )
        })?;
    let verifier = std::sync::Arc::new(crate::hive::tls_verify::PinnedServerCertVerifier::new(
        target_public_key,
    ));
    // Pinned, not blind-trust: the pairing endpoint itself needs no client
    // cert (its server-TLS-only listener uses `with_no_client_auth()`), but
    // the server cert it presents must match the expected peer's key exactly.
    let tls_config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(rustls::DEFAULT_VERSIONS)
    .map_err(|e| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to select TLS protocol versions: {e}"),
        )
    })?
    .dangerous()
    .with_custom_certificate_verifier(verifier)
    .with_no_client_auth();
    let pinned_client = reqwest::Client::builder()
        .use_preconfigured_tls(tls_config)
        .connect_timeout(crate::hive::client::CONNECT_TIMEOUT)
        .timeout(crate::hive::client::REQUEST_TIMEOUT)
        .build()
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let resp = pinned_client
        .post(format!("https://{}/api/v1/hive/pair", body.peer_address))
        .json(&pair_body)
        .send()
        .await
        .map_err(|e| {
            ApiError(
                StatusCode::BAD_GATEWAY,
                format!("could not reach peer: {e}"),
            )
        })?;
    if !resp.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "peer rejected the pairing code".to_string(),
        ));
    }
    let pair_response: Value = resp
        .json()
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    let remote_roster: Vec<crate::hive::roster::RosterEntry> =
        serde_json::from_value(pair_response["roster"].clone()).map_err(|e| {
            ApiError(
                StatusCode::BAD_GATEWAY,
                format!("malformed roster in pairing response: {e}"),
            )
        })?;

    let merged = store.hive_merge_roster(remote_roster).await?;

    // Eager first-sync against every newly-known Active peer, rather than
    // waiting up to sync_interval_seconds for the next timer tick.
    let roster_size = merged.len();
    let store_for_sync = store.clone();
    let identity_for_sync = (*identity).clone();
    tokio::spawn(async move {
        for peer in merged.iter().filter(|e| {
            e.status == crate::hive::roster::RosterStatus::Active
                && e.device_id != identity_for_sync.device_id
        }) {
            if let Some(address) = crate::hive::peer_status::resolve_address(&peer.device_id)
                && let Ok(client) =
                    crate::hive::client::HiveClient::new(&identity_for_sync, &peer.public_key)
            {
                let _ = crate::hive::sync_loop::pull_from_peer(
                    &client,
                    &format!("https://{address}"),
                    &store_for_sync,
                    &peer.device_id,
                )
                .await;
            }
        }
    });

    Ok(Json(json!({ "joined": true, "roster_size": roster_size })))
}

pub(super) async fn hive_roster(State(store): State<Store>) -> Result<Json<Value>, ApiError> {
    let roster = store.hive_list_roster().await?;
    Ok(Json(json!({ "roster": roster })))
}

pub(super) async fn hive_manifest(State(store): State<Store>) -> Result<Json<Value>, ApiError> {
    let manifest = store.hive_manifest().await?;
    Ok(Json(json!({
        "memories": manifest.memories,
        "tombstones": manifest.tombstones,
        "settings": { "hash": manifest.settings.0, "updated_at": manifest.settings.1 },
        "tag_namespaces": { "hash": manifest.tag_namespaces.0, "updated_at": manifest.tag_namespaces.1 },
    })))
}

pub(super) async fn hive_issue_pairing_code(
    Extension(pairing_codes): Extension<Arc<PairingCodeStore>>,
    Extension(pairing_window): Extension<Arc<crate::hive::pairing_window::PairingWindow>>,
    Extension(identity): Extension<Arc<crate::hive::identity::DeviceIdentity>>,
) -> Json<Value> {
    let now = chrono::Utc::now().timestamp();
    let pairing = pairing_codes.issue(now);
    // Open the server-TLS-only pairing listener for exactly as long as this
    // code is valid; it auto-closes when the window elapses.
    pairing_window.open_for(std::time::Duration::from_secs(
        (pairing.expires_at - now).max(0) as u64,
    ));
    // `public_key` is meant to travel alongside `code` out-of-band (shown on
    // this device's own screen) so the joiner can pin the pairing TLS
    // connection to it instead of blindly trusting whatever cert answers on
    // `peer_address` -- see issue #26 / `JoinHiveBody::peer_public_key`.
    Json(json!({
        "code": pairing.code,
        "expires_at": pairing.expires_at,
        "public_key": crate::hive::identity::public_key_hex(&identity),
    }))
}

pub(super) async fn hive_get_memory(
    State(store): State<Store>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    match store.recall_by_id(&id).await? {
        None => Err(ApiError(StatusCode::NOT_FOUND, format!("no memory {id}"))),
        Some(e) => {
            let hash = crate::store::compute_hive_content_hash(
                &e.title,
                &e.content,
                &e.tags,
                &e.layer,
                &e.memory_type,
            );
            Ok(Json(json!({
                "id": e.id, "title": e.title, "content": e.content, "tags": e.tags,
                "layer": e.layer, "memory_type": e.memory_type, "updated_at": e.updated_at,
                "hive_content_hash": hash,
            })))
        }
    }
}

#[derive(Deserialize)]
pub(super) struct AddTrustedNetworkBody {
    id: String,
    label: Option<String>,
}

pub(super) async fn hive_get_trusted_networks(
    State(store): State<Store>,
) -> Result<Json<Value>, ApiError> {
    let trusted = store.hive_trusted_networks().await?;
    let current_network = crate::hive::network::current_network_key_async().await;
    Ok(Json(
        json!({ "current_network": current_network, "trusted": trusted }),
    ))
}

pub(super) async fn hive_add_trusted_network(
    State(store): State<Store>,
    Json(body): Json<AddTrustedNetworkBody>,
) -> Result<Json<Value>, ApiError> {
    store.add_hive_trusted_network(&body.id, body.label).await?;
    Ok(Json(
        json!({ "trusted": store.hive_trusted_networks().await? }),
    ))
}

pub(super) async fn hive_remove_trusted_network(
    State(store): State<Store>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    store.remove_hive_trusted_network(&id).await?;
    Ok(Json(
        json!({ "trusted": store.hive_trusted_networks().await? }),
    ))
}

pub(super) async fn hive_status(
    State(store): State<Store>,
    Extension(hive): Extension<HivePushConfig>,
    Extension(sync_port): Extension<HiveSyncPort>,
) -> Result<Json<Value>, ApiError> {
    let identity_json = hive.identity.as_ref().map(|identity| {
        json!({
            "device_id": identity.device_id,
            "name": identity.device_id,
            "public_key": crate::hive::identity::public_key_hex(identity),
        })
    });
    let self_device_id = hive.identity.as_ref().map(|i| i.device_id.as_str());
    let roster = store.hive_list_roster().await?;
    let mut roster_json = Vec::with_capacity(roster.len());
    for entry in &roster {
        let status = store.hive_get_peer_status(&entry.device_id).await?;
        roster_json.push(json!({
            "device_id": entry.device_id,
            "name": entry.name,
            // Lets the dashboard mark this device's own roster entry and
            // hide the Revoke action for it (the API rejects self-revoke).
            "is_self": Some(entry.device_id.as_str()) == self_device_id,
            "status": match entry.status {
                crate::hive::roster::RosterStatus::Active => "active",
                crate::hive::roster::RosterStatus::Revoked => "revoked",
            },
            "online": status.as_ref().map(|s| s.online).unwrap_or(false),
            "last_synced_at": status.as_ref().and_then(|s| s.last_synced_at),
            "pending_conflict_count": status.as_ref().map(|s| s.pending_conflict_count).unwrap_or(0),
            "joined_at": entry.joined_at,
        }));
    }
    Ok(Json(json!({
        "enabled": hive.enabled,
        "identity": identity_json,
        "sync_port": if sync_port.0 == 0 { Value::Null } else { json!(sync_port.0) },
        "pending_conflict_count": store.pending_conflict_count().await?,
        "roster": roster_json,
    })))
}

pub(super) async fn hive_revoke_device(
    State(store): State<Store>,
    Extension(identity): Extension<Arc<crate::hive::identity::DeviceIdentity>>,
    Path(device_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    // A self-revocation verifies against this device's own key and is
    // sticky, so it would lock this device out of its own hive permanently
    // with no way back from this device. The dashboard hides the action for
    // the self entry; refuse it here too so nothing else can trip it.
    if device_id == identity.device_id {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "a device cannot revoke itself; revoke it from another hive member".to_string(),
        ));
    }
    let local_roster = store.hive_list_roster().await?;
    let Some(target) = local_roster.iter().find(|e| e.device_id == device_id) else {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("no roster entry for {device_id}"),
        ));
    };
    if target.status == crate::hive::roster::RosterStatus::Revoked {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("{device_id} is already revoked"),
        ));
    }

    let revocation = crate::hive::roster::create_revocation_record(
        &identity,
        &device_id,
        chrono::Utc::now().timestamp(),
    );
    let mut revoked_entry = target.clone();
    revoked_entry.revocation_record = Some(revocation);
    let merged = store.hive_merge_roster(vec![revoked_entry]).await?;

    // `merge_roster` only actually flips the target to Revoked if the local
    // device is itself a trusted (Active, pre-merge) roster member -- a
    // deliberate gossip-safety gate, not something to route around here. If
    // that gate rejected the revocation (e.g. this device hasn't completed
    // its own self-join yet, such as right after a paused/auto-paused hive
    // start), report failure instead of a false `{"revoked": true}` success.
    let actually_revoked = merged
        .iter()
        .find(|e| e.device_id == device_id)
        .map(|e| e.status == crate::hive::roster::RosterStatus::Revoked)
        .unwrap_or(false);
    if !actually_revoked {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "revocation was not applied -- this device may not yet be an Active member of its own roster (hive may still be starting up)".to_string(),
        ));
    }

    let store_for_push = store.clone();
    let identity_for_push = (*identity).clone();
    tokio::spawn(crate::hive::sync_loop::push_revocation_to_online_peers(
        store_for_push,
        identity_for_push,
    ));

    Ok(Json(json!({ "revoked": true })))
}

#[derive(Deserialize)]
pub(super) struct SetHiveEnabledBody {
    enabled: bool,
}

pub(super) async fn hive_set_enabled(
    State(store): State<Store>,
    Extension(restart_notify): Extension<Arc<tokio::sync::Notify>>,
    Extension(sync): Extension<crate::config::SyncSettings>,
    Json(body): Json<SetHiveEnabledBody>,
) -> Result<Json<Value>, ApiError> {
    // config.toml refuses `[sync] enabled` + `[hive] enabled` at load time;
    // the DB override must not be a way around that. (`run_up` also refuses
    // to start the hive stack in that state, so accepting this would just
    // trigger a restart into "hive still off".)
    if body.enabled && sync.enabled {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Hive Mode cannot be enabled while [sync] cloud sync is enabled; disable [sync] in config.toml first".to_string(),
        ));
    }
    store.set_hive_enabled_override(body.enabled).await?;
    // `run_up`'s tail races this Notify against its normal serve-awaits and
    // performs the same abort-listeners+re-exec sequence the TUI's detach
    // key already uses -- the re-exec'd child re-reads the override on its
    // own fresh boot, so no other state needs to travel through the restart.
    restart_notify.notify_one();
    Ok(Json(json!({ "restarting": true })))
}

pub(super) async fn hive_get_settings(State(store): State<Store>) -> Result<Json<Value>, ApiError> {
    let (sync_s, ping_s, updated_at) = store
        .hive_settings_override()
        .await?
        .unwrap_or((300, 60, 0));
    Ok(Json(json!({
        "sync_interval_seconds": sync_s, "ping_interval_seconds": ping_s, "updated_at": updated_at,
    })))
}

pub(super) async fn hive_get_tag_namespaces(
    State(store): State<Store>,
) -> Result<Json<Value>, ApiError> {
    let namespaces = store.tag_namespace_registry().await;
    let updated_at: i64 = store
        .get_meta("tag_namespaces_updated_at")
        .await?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok(Json(
        json!({ "namespaces": namespaces, "updated_at": updated_at }),
    ))
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum HivePushBody {
    Memory {
        id: String,
        title: String,
        content: String,
        tags: Vec<String>,
        layer: String,
        memory_type: String,
        updated_at: i64,
        /// Accepted for wire compatibility (peers send it alongside the
        /// manifest hash) but never trusted: `apply_incoming_memory`
        /// recomputes the hash from the content it actually received.
        #[allow(dead_code)]
        hive_content_hash: String,
    },
    Tombstone {
        memory_id: String,
        deleted_at: i64,
    },
    Settings {
        sync_interval_seconds: u64,
        ping_interval_seconds: u64,
        updated_at: i64,
    },
    TagNamespaces {
        namespaces: Value,
        updated_at: i64,
    },
    Roster {
        roster: Vec<crate::hive::roster::RosterEntry>,
    },
}

pub(super) async fn hive_push(
    State(store): State<Store>,
    Json(body): Json<HivePushBody>,
) -> Result<Json<Value>, ApiError> {
    match body {
        HivePushBody::Memory {
            id,
            title,
            content,
            tags,
            layer,
            memory_type,
            updated_at,
            // Sent by peers for symmetry with the manifest, but not trusted:
            // `apply_incoming_memory` recomputes the hash from the content.
            hive_content_hash: _,
        } => {
            let incoming = crate::store::MemoryEntry {
                id,
                title,
                content,
                tags,
                created_at: updated_at,
                updated_at,
                token_count: None,
                layer,
                memory_type,
            };
            let outcome = store.apply_incoming_memory(&incoming, None).await?;
            Ok(Json(json!({ "outcome": format!("{outcome:?}") })))
        }
        HivePushBody::Tombstone {
            memory_id,
            deleted_at,
        } => {
            if !crate::store::hive_timestamp_is_plausible(deleted_at) {
                return Err(implausible_timestamp("deleted_at"));
            }
            store
                .apply_incoming_tombstone(&memory_id, deleted_at)
                .await?;
            Ok(Json(json!({ "outcome": "tombstone_processed" })))
        }
        HivePushBody::Settings {
            sync_interval_seconds,
            ping_interval_seconds,
            updated_at,
        } => {
            if !crate::store::hive_timestamp_is_plausible(updated_at) {
                return Err(implausible_timestamp("updated_at"));
            }
            if !crate::hive::interval_in_range(sync_interval_seconds)
                || !crate::hive::interval_in_range(ping_interval_seconds)
            {
                return Err(ApiError(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    format!(
                        "sync/ping intervals must be between {} and {} seconds",
                        crate::hive::MIN_INTERVAL_SECONDS,
                        crate::hive::MAX_INTERVAL_SECONDS
                    ),
                ));
            }
            let current = store.hive_settings_override().await?;
            let should_apply = current
                .map(|(_, _, cur_updated_at)| updated_at > cur_updated_at)
                .unwrap_or(true);
            if should_apply {
                store
                    .set_hive_settings_override(
                        sync_interval_seconds,
                        ping_interval_seconds,
                        updated_at,
                    )
                    .await?;
            }
            Ok(Json(
                json!({ "outcome": if should_apply { "applied" } else { "kept_local" } }),
            ))
        }
        HivePushBody::TagNamespaces {
            namespaces,
            updated_at,
        } => {
            if !crate::store::hive_timestamp_is_plausible(updated_at) {
                return Err(implausible_timestamp("updated_at"));
            }
            // Same rules as the dashboard's own save: a malformed registry
            // must not be persisted verbatim on a peer's say-so.
            crate::api::validate_tag_namespaces(&namespaces)
                .map_err(|e| ApiError(StatusCode::UNPROCESSABLE_ENTITY, e))?;
            let current_updated_at: i64 = store
                .get_meta("tag_namespaces_updated_at")
                .await?
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if updated_at > current_updated_at {
                store
                    .set_meta("tag_namespaces", &namespaces.to_string())
                    .await?;
                store
                    .set_meta("tag_namespaces_updated_at", &updated_at.to_string())
                    .await?;
            }
            Ok(Json(
                json!({ "outcome": if updated_at > current_updated_at { "applied" } else { "kept_local" } }),
            ))
        }
        HivePushBody::Roster { roster } => {
            store.hive_merge_roster(roster).await?;
            Ok(Json(json!({ "outcome": "merged" })))
        }
    }
}

/// A peer-supplied last-write-wins timestamp further in the future than the
/// clock-skew tolerance. Accepting it would let that one record win every
/// comparison against every legitimate later write from every device.
fn implausible_timestamp(field: &str) -> ApiError {
    ApiError(
        StatusCode::UNPROCESSABLE_ENTITY,
        format!(
            "{field} is more than {}s in the future",
            crate::store::HIVE_CLOCK_SKEW_TOLERANCE_SECONDS
        ),
    )
}
