use crate::hive::identity::DeviceIdentity;
use crate::hive::roster::RosterStatus;
use crate::store::SqliteStore;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

/// Process-wide map of device_id -> last-seen "ip:port" from mDNS discovery.
/// A `Mutex<HashMap<..>>` behind a `once_cell`-style static is the simplest
/// way to share this between the mDNS browse task (which only writes to it)
/// and the ping/sync loops (which only read from it) without threading a
/// new parameter through every function that might need an address.
static DISCOVERED_ADDRESSES: std::sync::OnceLock<Mutex<HashMap<String, String>>> =
    std::sync::OnceLock::new();

fn discovered_addresses() -> &'static Mutex<HashMap<String, String>> {
    DISCOVERED_ADDRESSES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn record_discovered_address(device_id: &str, address: String) {
    discovered_addresses()
        .lock()
        .unwrap()
        .insert(device_id.to_string(), address);
}

pub fn resolve_address(device_id: &str) -> Option<String> {
    discovered_addresses()
        .lock()
        .unwrap()
        .get(device_id)
        .cloned()
}

pub struct PeerStatus {
    pub device_id: String,
    pub public_key: String,
    pub address: Option<String>,
    pub online: bool,
    pub last_synced_at: Option<i64>,
}

/// Peers this device currently believes are reachable, for the push-on-change
/// path to target. `address` comes from the mDNS discovery table
/// (`resolve_address`); every push helper skips a peer without one, so
/// leaving it `None` here (as an earlier revision did) silently turned the
/// whole push-on-change feature into a no-op.
pub async fn online_peers(store: &SqliteStore) -> anyhow::Result<Vec<PeerStatus>> {
    let roster = store.hive_list_roster().await?;
    let mut out = Vec::new();
    for entry in roster
        .into_iter()
        .filter(|e| e.status == RosterStatus::Active)
    {
        if let Some(status) = store.hive_get_peer_status(&entry.device_id).await?
            && status.online
        {
            out.push(PeerStatus {
                address: resolve_address(&entry.device_id),
                device_id: entry.device_id,
                public_key: entry.public_key,
                online: true,
                last_synced_at: status.last_synced_at,
            });
        }
    }
    Ok(out)
}

pub async fn run_ping_loop(
    store: Arc<SqliteStore>,
    identity: DeviceIdentity,
    interval_seconds_default: u64,
    sync_tls_config: axum_server::tls_rustls::RustlsConfig,
    rebuild_sync_server_config: impl Fn(
        Vec<crate::hive::roster::RosterEntry>,
    ) -> anyhow::Result<rustls::ServerConfig>
    + Send
    + Sync
    + 'static,
    events: tokio::sync::broadcast::Sender<serde_json::Value>,
) {
    loop {
        let interval_seconds = store
            .hive_settings_override()
            .await
            .ok()
            .flatten()
            .map(|(_, ping_s, _)| ping_s)
            .unwrap_or(interval_seconds_default);
        tokio::time::sleep(Duration::from_secs(crate::hive::clamp_interval(
            interval_seconds,
        )))
        .await;
        ping_once(&store, &identity, &events).await;

        // `ping_once` above already re-merged every reachable peer's roster
        // view (gossip refresh, including revocations). Rebuild the sync
        // listener's client-cert verifier from the now-current roster and
        // hot-swap it in (no rebind), so a revoked device stops being
        // accepted, and a newly-paired device starts being accepted, within
        // one ping interval instead of requiring a process restart.
        if let Ok(current_roster) = store.hive_list_roster().await {
            match rebuild_sync_server_config(current_roster) {
                Ok(new_config) => {
                    sync_tls_config.reload_from_config(std::sync::Arc::new(new_config))
                }
                Err(e) => tracing::warn!("failed to rebuild hive sync TLS config: {e:#}"),
            }
        }
    }
}

async fn ping_once(
    store: &Arc<SqliteStore>,
    identity: &DeviceIdentity,
    events: &tokio::sync::broadcast::Sender<serde_json::Value>,
) {
    let Ok(roster) = store.hive_list_roster().await else {
        return;
    };
    for peer in roster
        .iter()
        .filter(|e| e.status == RosterStatus::Active && e.device_id != identity.device_id)
    {
        let previously_online = store
            .hive_get_peer_status(&peer.device_id)
            .await
            .ok()
            .flatten()
            .map(|s| s.online);

        let now_online = match resolve_address(&peer.device_id) {
            None => {
                let _ = store
                    .hive_upsert_peer_status(&peer.device_id, false, None)
                    .await;
                false
            }
            Some(address) => {
                let Ok(client) = crate::hive::client::HiveClient::new(identity, &peer.public_key)
                else {
                    continue;
                };
                match client
                    .get(&format!("https://{address}/api/v1/hive/roster"))
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        // Liveness only: `last_synced_at` is left as-is
                        // (`None` preserves the stored value) -- the sync
                        // loop stamps it when a real pull round completes.
                        let _ = store
                            .hive_upsert_peer_status(&peer.device_id, true, None)
                            .await;
                        // Re-merge this peer's roster view, continuing Plan 1's
                        // gossip propagation (including revocations) on the same
                        // round-trip as the liveness check.
                        if let Ok(body) = resp.json::<serde_json::Value>().await
                            && let Some(remote_roster_json) = body["roster"].as_array()
                            && let Ok(remote_roster) =
                                serde_json::from_value::<Vec<crate::hive::roster::RosterEntry>>(
                                    serde_json::Value::Array(remote_roster_json.clone()),
                                )
                            && let Err(e) = store.hive_merge_roster(remote_roster).await
                        {
                            tracing::warn!(
                                "failed to merge roster gossiped by {}: {e:#}",
                                peer.device_id
                            );
                        }
                        true
                    }
                    _ => {
                        let _ = store
                            .hive_upsert_peer_status(&peer.device_id, false, None)
                            .await;
                        false
                    }
                }
            }
        };

        // Only a real flip from a previously-*known* state emits -- the
        // very first observation of a peer (no prior hive_peer_status row)
        // is not a "transition" worth a toast.
        if let Some(previous) = previously_online
            && previous != now_online
        {
            let _ = events.send(serde_json::json!({ "type": "hive_peer_status_changed" }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_store() -> crate::store::SqliteStore {
        let sync = crate::config::SyncSettings::default();
        let database = crate::db::open_database(&sync, ":memory:").await.unwrap();
        let conn = database.connect().unwrap();
        crate::db::run_migrations(&conn).await.unwrap();
        crate::store::SqliteStore::new(conn)
    }

    #[tokio::test]
    async fn online_peers_lists_only_active_and_online_entries() {
        let store = test_store().await;
        let online = crate::hive::identity::generate();
        let offline = crate::hive::identity::generate();
        let revoked = crate::hive::identity::generate();
        for (identity, name) in [
            (&online, "online-peer"),
            (&offline, "offline-peer"),
            (&revoked, "revoked-peer"),
        ] {
            let join = crate::hive::roster::create_join_record(identity, name, 1000);
            store
                .hive_upsert_roster_entry(&crate::hive::roster::RosterEntry {
                    device_id: identity.device_id.clone(),
                    public_key: crate::hive::identity::public_key_hex(identity),
                    name: name.to_string(),
                    status: if identity.device_id == revoked.device_id {
                        RosterStatus::Revoked
                    } else {
                        RosterStatus::Active
                    },
                    joined_at: 1000,
                    revoked_at: None,
                    revoked_by: None,
                    join_record: join,
                    revocation_record: None,
                })
                .await
                .unwrap();
        }
        store
            .hive_upsert_peer_status(&online.device_id, true, Some(5000))
            .await
            .unwrap();
        store
            .hive_upsert_peer_status(&offline.device_id, false, None)
            .await
            .unwrap();
        store
            .hive_upsert_peer_status(&revoked.device_id, true, Some(6000))
            .await
            .unwrap();

        let peers = online_peers(&store).await.unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].device_id, online.device_id);
        assert!(peers[0].online);
        assert_eq!(peers[0].last_synced_at, Some(5000));
    }

    #[test]
    fn record_and_resolve_discovered_address_round_trips() {
        record_discovered_address("hive_addrtest", "10.0.0.5:9999".to_string());
        assert_eq!(
            resolve_address("hive_addrtest"),
            Some("10.0.0.5:9999".to_string())
        );
        assert_eq!(resolve_address("hive_never_recorded"), None);
    }

    #[tokio::test]
    async fn ping_once_emits_event_only_on_an_actual_online_flip() {
        let store = std::sync::Arc::new(test_store().await);
        let identity = crate::hive::identity::generate();
        let peer = crate::hive::identity::generate();
        let join = crate::hive::roster::create_join_record(&peer, "peer", 1000);
        store
            .hive_upsert_roster_entry(&crate::hive::roster::RosterEntry {
                device_id: peer.device_id.clone(),
                public_key: crate::hive::identity::public_key_hex(&peer),
                name: "peer".to_string(),
                status: crate::hive::roster::RosterStatus::Active,
                joined_at: 1000,
                revoked_at: None,
                revoked_by: None,
                join_record: join,
                revocation_record: None,
            })
            .await
            .unwrap();
        // No mDNS address recorded for this peer -- ping_once will mark it
        // offline. Starting with no hive_peer_status row at all, the first
        // ping_once call transitions "unknown" -> "offline", which is NOT a
        // flip from a known online state and must not emit.
        let (events, mut rx) = tokio::sync::broadcast::channel(16);
        ping_once(&store, &identity, &events).await;
        assert!(
            rx.try_recv().is_err(),
            "first-ever offline status must not emit (no prior known state)"
        );

        // Force it online directly (bypassing a real network contact, which
        // this unit test has no interest in exercising), then run ping_once
        // again -- still offline (no address), so this is a real online ->
        // offline flip and must emit.
        store
            .hive_upsert_peer_status(&peer.device_id, true, Some(1234))
            .await
            .unwrap();
        ping_once(&store, &identity, &events).await;
        let evt = rx
            .try_recv()
            .expect("online->offline flip must emit hive_peer_status_changed");
        assert_eq!(evt["type"], "hive_peer_status_changed");

        // Immediately calling it again with no change must not emit again.
        ping_once(&store, &identity, &events).await;
        assert!(rx.try_recv().is_err(), "no-change tick must not emit");
    }
}
