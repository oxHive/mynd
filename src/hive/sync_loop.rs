#[cfg(test)]
use crate::store::HiveManifest;
use crate::store::SqliteStore;
use anyhow::Result;

pub struct PullSummary {
    pub memories_pulled: usize,
    pub tombstones_applied: usize,
    pub conflicts: usize,
}

/// Pure diff/apply core: given a remote manifest and read access to the
/// remote store (a `HiveClient` GET in production, an in-process second
/// `SqliteStore` in this task's tests), figures out what differs and
/// applies it to `local`. Kept free of any HTTP/TLS specifics so it's
/// testable without a live network. Test-only: production sync goes
/// through `pull_from_peer` below, which needs the HTTP-specific handling
/// (partial-failure skips, JSON field mapping) this pure version doesn't do.
#[cfg(test)]
async fn diff_and_apply(
    local: &SqliteStore,
    remote: &SqliteStore,
    remote_manifest: &HiveManifest,
) -> Result<PullSummary> {
    let local_manifest = local.hive_manifest().await?;
    let mut summary = PullSummary {
        memories_pulled: 0,
        tombstones_applied: 0,
        conflicts: 0,
    };

    for (id, (remote_hash, _remote_updated_at)) in &remote_manifest.memories {
        let differs = match local_manifest.memories.get(id) {
            Some((local_hash, _)) => local_hash != remote_hash,
            None => true,
        };
        if !differs {
            continue;
        }
        let Some(remote_entry) = remote.recall_by_id(id).await? else {
            continue;
        };
        match local.apply_incoming_memory(&remote_entry, None).await? {
            crate::store::ApplyOutcome::Applied => summary.memories_pulled += 1,
            crate::store::ApplyOutcome::Conflicted => summary.conflicts += 1,
            crate::store::ApplyOutcome::KeptLocal => {}
        }
    }

    for (id, remote_deleted_at) in &remote_manifest.tombstones {
        if local
            .apply_incoming_tombstone(id, *remote_deleted_at)
            .await?
        {
            summary.tombstones_applied += 1;
        }
    }

    Ok(summary)
}

use crate::hive::client::HiveClient;

pub async fn pull_from_peer(
    client: &HiveClient,
    base_url: &str,
    local: &SqliteStore,
    source_device_id: &str,
) -> Result<PullSummary> {
    let manifest_resp = client
        .get(&format!("{base_url}/api/v1/hive/manifest"))
        .await?;
    let manifest_json: serde_json::Value = manifest_resp.json().await?;

    let mut summary = PullSummary {
        memories_pulled: 0,
        tombstones_applied: 0,
        conflicts: 0,
    };
    let local_manifest = local.hive_manifest().await?;

    let remote_memories = manifest_json["memories"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    for (id, entry) in &remote_memories {
        let remote_hash = entry[0].as_str().unwrap_or_default();
        let differs = match local_manifest.memories.get(id) {
            Some((local_hash, _)) => local_hash != remote_hash,
            None => true,
        };
        if !differs {
            continue;
        }
        let mem_resp = client
            .get(&format!("{base_url}/api/v1/hive/memories/{id}"))
            .await?;
        if !mem_resp.status().is_success() {
            continue; // peer no longer has it or a transient error -- skip, next round retries
        }
        let mem_json: serde_json::Value = mem_resp.json().await?;
        let remote_entry = crate::store::MemoryEntry {
            id: id.clone(),
            title: mem_json["title"].as_str().unwrap_or_default().to_string(),
            content: mem_json["content"].as_str().unwrap_or_default().to_string(),
            tags: mem_json["tags"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|t| t.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            created_at: mem_json["updated_at"].as_i64().unwrap_or_default(),
            updated_at: mem_json["updated_at"].as_i64().unwrap_or_default(),
            token_count: None,
            layer: mem_json["layer"]
                .as_str()
                .unwrap_or("workspace")
                .to_string(),
            memory_type: mem_json["memory_type"]
                .as_str()
                .unwrap_or("project")
                .to_string(),
        };
        match local
            .apply_incoming_memory(&remote_entry, Some(source_device_id))
            .await?
        {
            crate::store::ApplyOutcome::Applied => summary.memories_pulled += 1,
            crate::store::ApplyOutcome::Conflicted => summary.conflicts += 1,
            crate::store::ApplyOutcome::KeptLocal => {}
        }
    }

    let remote_tombstones = manifest_json["tombstones"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    for (id, deleted_at) in &remote_tombstones {
        let Some(deleted_at) = deleted_at.as_i64() else {
            continue;
        };
        // A far-future deleted_at would block every later re-creation of
        // this id from every device; a peer's clock can't be that far off.
        if !crate::store::hive_timestamp_is_plausible(deleted_at) {
            tracing::warn!(
                "hive peer {source_device_id} advertised an implausible deleted_at for {id}; ignoring"
            );
            continue;
        }
        if local.apply_incoming_tombstone(id, deleted_at).await? {
            summary.tombstones_applied += 1;
        }
    }

    // Finding I3: pull the peer's hive settings override if theirs is newer.
    // The manifest already advertises a hash + updated_at for it; a full fetch
    // + last-write-wins-by-timestamp closes the gap that made this dead code.
    // The same plausibility/range checks as the push path (`hive_push`)
    // apply: an implausible timestamp or an out-of-range interval is skipped
    // rather than adopted.
    let remote_settings = &manifest_json["settings"];
    if let Some(remote_updated_at) = remote_settings["updated_at"].as_i64()
        && crate::store::hive_timestamp_is_plausible(remote_updated_at)
    {
        let local_settings_updated_at = local
            .hive_settings_override()
            .await
            .ok()
            .flatten()
            .map(|(_, _, updated_at)| updated_at)
            .unwrap_or(0);
        if remote_updated_at > local_settings_updated_at
            && let Ok(resp) = client
                .get(&format!("{base_url}/api/v1/hive/settings"))
                .await
            && resp.status().is_success()
            && let Ok(body) = resp.json::<serde_json::Value>().await
            && let (Some(sync_s), Some(ping_s)) = (
                body["sync_interval_seconds"].as_u64(),
                body["ping_interval_seconds"].as_u64(),
            )
        {
            if crate::hive::interval_in_range(sync_s) && crate::hive::interval_in_range(ping_s) {
                let _ = local
                    .set_hive_settings_override(sync_s, ping_s, remote_updated_at)
                    .await;
            } else {
                tracing::warn!(
                    "hive peer {source_device_id} advertised out-of-range sync/ping intervals ({sync_s}s/{ping_s}s); ignoring"
                );
            }
        }
    }

    // Finding I3: pull the peer's tag-namespace registry if theirs is newer.
    // Validated with the same rules the dashboard's own save goes through --
    // a malformed registry from a peer would otherwise be persisted verbatim
    // and silently reset the registry to defaults on the next read.
    let remote_tag_namespaces = &manifest_json["tag_namespaces"];
    if let Some(remote_updated_at) = remote_tag_namespaces["updated_at"].as_i64()
        && crate::store::hive_timestamp_is_plausible(remote_updated_at)
    {
        let local_tag_namespaces_updated_at: i64 = local
            .get_meta("tag_namespaces_updated_at")
            .await
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if remote_updated_at > local_tag_namespaces_updated_at
            && let Ok(resp) = client
                .get(&format!("{base_url}/api/v1/hive/tag-namespaces"))
                .await
            && resp.status().is_success()
            && let Ok(body) = resp.json::<serde_json::Value>().await
        {
            match crate::api::validate_tag_namespaces(&body["namespaces"]) {
                Ok(()) => {
                    let _ = local
                        .set_meta("tag_namespaces", &body["namespaces"].to_string())
                        .await;
                    let _ = local
                        .set_meta("tag_namespaces_updated_at", &remote_updated_at.to_string())
                        .await;
                }
                Err(e) => tracing::warn!(
                    "hive peer {source_device_id} served an invalid tag-namespace registry ({e}); ignoring"
                ),
            }
        }
    }

    Ok(summary)
}

use crate::hive::identity::DeviceIdentity;
use std::sync::Arc;
use std::time::Duration;

pub async fn run_sync_loop(
    store: Arc<SqliteStore>,
    identity: DeviceIdentity,
    interval_seconds_default: u64,
) {
    loop {
        let interval_seconds = store
            .hive_settings_override()
            .await
            .ok()
            .flatten()
            .map(|(sync_s, _, _)| sync_s)
            .unwrap_or(interval_seconds_default);
        tokio::time::sleep(Duration::from_secs(crate::hive::clamp_interval(
            interval_seconds,
        )))
        .await;
        sync_once(&store, &identity).await;
    }
}

async fn sync_once(store: &Arc<SqliteStore>, identity: &DeviceIdentity) {
    let Ok(roster) = store.hive_list_roster().await else {
        return;
    };
    for peer in roster.iter().filter(|e| {
        e.status == crate::hive::roster::RosterStatus::Active && e.device_id != identity.device_id
    }) {
        let Some(address) = crate::hive::peer_status::resolve_address(&peer.device_id) else {
            continue;
        };
        let Ok(client) = crate::hive::client::HiveClient::new(identity, &peer.public_key) else {
            continue;
        };
        let base_url = format!("https://{address}");
        match pull_from_peer(&client, &base_url, store, &peer.device_id).await {
            Ok(summary) => {
                // This is the only place a real sync round completes, so it
                // is what `last_synced_at` (surfaced per peer in the
                // dashboard) should reflect -- not the ping loop's liveness
                // check, which never moves any data.
                let _ = store
                    .hive_upsert_peer_status(
                        &peer.device_id,
                        true,
                        Some(crate::store::chrono_now()),
                    )
                    .await;
                if summary.conflicts > 0 {
                    tracing::warn!(
                        "{} hive sync conflict(s) with {}; review in the dashboard",
                        summary.conflicts,
                        peer.device_id
                    );
                }
            }
            Err(e) => tracing::warn!("hive sync with {} failed: {e:#}", peer.device_id),
        }
    }
}

/// Best-effort push of a just-changed memory to every peer this device
/// currently believes is online. Takes owned values, not references: this
/// function is always invoked via `tokio::spawn` (the REST/MCP write
/// handlers in `src/api/memories.rs` / `src/server.rs`), and a spawned
/// future must be `'static` -- it cannot borrow from the caller's stack
/// frame, which is why every argument here is owned (`Arc` for the store,
/// an owned `DeviceIdentity` clone, an owned `String` for the id) rather
/// than a borrow.
pub async fn push_memory_change_to_online_peers(
    store: Arc<SqliteStore>,
    identity: DeviceIdentity,
    memory_id: String,
) {
    let Ok(Some(payload)) = store.hive_push_payload_for(&memory_id).await else {
        return;
    };
    let Ok(online_peers) = crate::hive::peer_status::online_peers(&store).await else {
        return;
    };
    for peer in online_peers {
        let Ok(client) = crate::hive::client::HiveClient::new(&identity, &peer.public_key) else {
            continue;
        };
        let Some(address) = peer.address else {
            continue;
        };
        let _ = client
            .post_json(&format!("https://{address}/api/v1/hive/push"), &payload)
            .await;
        // Best-effort: a failed push is silently dropped, per this plan's
        // spec decision -- the peer's own next pull round is the backstop.
    }
}

/// Best-effort push of this device's hive settings override to every online
/// peer (Finding I3), mirroring `push_memory_change_to_online_peers`. NOTE:
/// nothing calls this yet — there is no local HTTP endpoint that writes the
/// hive settings override (only the peer-receive `hive_push` handler does), so
/// there is no local "settings changed" event to trigger from. It's defined
/// here so whichever later work adds a settings-save endpoint (e.g. Plan 3's
/// dashboard) can wire it up exactly like the tag-namespace push below.
pub async fn push_settings_change_to_online_peers(
    store: Arc<SqliteStore>,
    identity: DeviceIdentity,
) {
    let Ok(Some((sync_s, ping_s, updated_at))) = store.hive_settings_override().await else {
        return;
    };
    let payload = serde_json::json!({
        "kind": "settings",
        "sync_interval_seconds": sync_s,
        "ping_interval_seconds": ping_s,
        "updated_at": updated_at,
    });
    let Ok(online_peers) = crate::hive::peer_status::online_peers(&store).await else {
        return;
    };
    for peer in online_peers {
        let Ok(client) = crate::hive::client::HiveClient::new(&identity, &peer.public_key) else {
            continue;
        };
        let Some(address) = peer.address else {
            continue;
        };
        let _ = client
            .post_json(&format!("https://{address}/api/v1/hive/push"), &payload)
            .await;
    }
}

/// Best-effort push of this device's tag-namespace registry to every online
/// peer (Finding I3). Triggered from `save_tag_settings` after it persists.
pub async fn push_tag_namespaces_change_to_online_peers(
    store: Arc<SqliteStore>,
    identity: DeviceIdentity,
) {
    let namespaces = store.tag_namespace_registry().await;
    let updated_at: i64 = store
        .get_meta("tag_namespaces_updated_at")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let payload = serde_json::json!({
        "kind": "tag_namespaces",
        "namespaces": namespaces,
        "updated_at": updated_at,
    });
    let Ok(online_peers) = crate::hive::peer_status::online_peers(&store).await else {
        return;
    };
    for peer in online_peers {
        let Ok(client) = crate::hive::client::HiveClient::new(&identity, &peer.public_key) else {
            continue;
        };
        let Some(address) = peer.address else {
            continue;
        };
        let _ = client
            .post_json(&format!("https://{address}/api/v1/hive/push"), &payload)
            .await;
    }
}

/// Best-effort push of a just-created revocation to every peer this device
/// currently believes is online, mirroring `push_memory_change_to_online_peers`.
/// Peers that don't get this push directly still receive the revocation on
/// their own next ping/sync contact with any Active member (Finding I2's
/// existing gossip-on-ping path), so this is purely a latency optimization,
/// not the only propagation path.
pub async fn push_revocation_to_online_peers(store: Arc<SqliteStore>, identity: DeviceIdentity) {
    let Ok(roster) = store.hive_list_roster().await else {
        return;
    };
    let Ok(online_peers) = crate::hive::peer_status::online_peers(&store).await else {
        return;
    };
    let payload = serde_json::json!({ "kind": "roster", "roster": roster });
    for peer in online_peers {
        let Ok(client) = crate::hive::client::HiveClient::new(&identity, &peer.public_key) else {
            continue;
        };
        let Some(address) = peer.address else {
            continue;
        };
        let _ = client
            .post_json(&format!("https://{address}/api/v1/hive/push"), &payload)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::NewMemoryRow;

    async fn test_store() -> crate::store::SqliteStore {
        let sync = crate::config::SyncSettings::default();
        let database = crate::db::open_database(&sync, ":memory:").await.unwrap();
        let conn = database.connect().unwrap();
        crate::db::run_migrations(&conn).await.unwrap();
        crate::store::SqliteStore::new(conn)
    }

    #[tokio::test]
    async fn diff_and_apply_pulls_a_memory_missing_locally() {
        let local_store = test_store().await;
        let remote_store = test_store().await;
        remote_store
            .store(&NewMemoryRow {
                id: "mem_pulltest0000000000000000001",
                title: "remote-only",
                content: "c",
                tags: &[],
                token_count: None,
                layer: "workspace",
                memory_type: "project",
            })
            .await
            .unwrap();
        let remote_manifest = remote_store.hive_manifest().await.unwrap();

        let summary = diff_and_apply(&local_store, &remote_store, &remote_manifest)
            .await
            .unwrap();
        assert_eq!(summary.memories_pulled, 1);
        assert!(
            local_store
                .recall_by_id("mem_pulltest0000000000000000001")
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn diff_and_apply_applies_a_tombstone_for_an_older_local_copy() {
        let local_store = test_store().await;
        let remote_store = test_store().await;
        local_store
            .store(&NewMemoryRow {
                id: "mem_pulltest0000000000000000002",
                title: "will be deleted",
                content: "c",
                tags: &[],
                token_count: None,
                layer: "workspace",
                memory_type: "project",
            })
            .await
            .unwrap();
        remote_store
            .store(&NewMemoryRow {
                id: "mem_pulltest0000000000000000002",
                title: "will be deleted",
                content: "c",
                tags: &[],
                token_count: None,
                layer: "workspace",
                memory_type: "project",
            })
            .await
            .unwrap();
        // `chrono_now()` (store.rs) is second-resolution, and this test's
        // tombstone check is a strict `<` on those timestamps: without a
        // gap, storing local + remote + deleting remote can all land in the
        // same wall-clock second, making local_entry.updated_at ==
        // remote_deleted_at and failing the `<` comparison intermittently.
        // Force a real gap so the assertion is deterministic.
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        remote_store
            .delete("mem_pulltest0000000000000000002")
            .await
            .unwrap();
        let remote_manifest = remote_store.hive_manifest().await.unwrap();

        let summary = diff_and_apply(&local_store, &remote_store, &remote_manifest)
            .await
            .unwrap();
        assert_eq!(summary.tombstones_applied, 1);
        assert!(
            local_store
                .recall_by_id("mem_pulltest0000000000000000002")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn diff_and_apply_conflict_is_unattributed_for_the_test_helper() {
        // diff_and_apply is the pure-function test helper (no HTTP/TLS) and
        // has no peer identity concept of its own -- it always passes None,
        // proven here so a future refactor doesn't accidentally wire a real
        // device_id through it without a test noticing the behavior change.
        let local_store = test_store().await;
        let remote_store = test_store().await;
        local_store
            .store(&NewMemoryRow {
                id: "mem_diffconflict00000000000000001",
                title: "local",
                content: "local content",
                tags: &[],
                token_count: None,
                layer: "workspace",
                memory_type: "project",
            })
            .await
            .unwrap();
        remote_store
            .store(&NewMemoryRow {
                id: "mem_diffconflict00000000000000001",
                title: "remote",
                content: "remote content",
                tags: &[],
                token_count: None,
                layer: "workspace",
                memory_type: "project",
            })
            .await
            .unwrap();
        // `store()` always stamps `updated_at` with real wall-clock seconds
        // (no way to inject a fixed time via NewMemoryRow) -- a conflict only
        // arises when both sides' timestamps tie exactly, which two
        // back-to-back `store()` calls only guarantee if no second boundary
        // ticks between them. Force the tie directly instead of relying on
        // that timing luck, so this test can't flake.
        let local_updated_at = local_store
            .recall_by_id("mem_diffconflict00000000000000001")
            .await
            .unwrap()
            .unwrap()
            .updated_at;
        remote_store
            .conn
            .execute(
                "UPDATE memories SET updated_at = ?1 WHERE id = 'mem_diffconflict00000000000000001'",
                libsql::params![local_updated_at],
            )
            .await
            .unwrap();
        let remote_manifest = remote_store.hive_manifest().await.unwrap();
        let summary = diff_and_apply(&local_store, &remote_store, &remote_manifest)
            .await
            .unwrap();
        assert_eq!(summary.conflicts, 1);
        let conflicts = local_store.list_conflicts(None).await.unwrap();
        assert!(conflicts[0].source_device_id.is_none());
    }
}
