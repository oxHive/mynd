pub mod cert;
pub mod client;
pub mod discovery;
pub mod gossip;
pub mod identity;
pub mod keyring_store;
pub mod network;
pub mod network_guard;
pub mod pairing;
pub mod pairing_window;
pub mod peer_status;
pub mod roster;
pub mod sync_loop;
pub mod tls_verify;

/// Bounds on the sync/ping cadence, applied wherever an interval is read --
/// the TOML default, the DB override, and anything a peer pushes. The lower
/// bound stops a `0`/tiny interval turning the loops into a busy-spin against
/// the DB and every peer; the upper bound matters for security, not just
/// tidiness: the ping loop is also what hot-swaps the sync listener's
/// roster-backed client-cert verifier, so a peer that could push
/// `ping_interval_seconds = 10 years` would effectively freeze revocations
/// out of every other device's TLS gate.
pub const MIN_INTERVAL_SECONDS: u64 = 5;
pub const MAX_INTERVAL_SECONDS: u64 = 3600;

pub fn clamp_interval(seconds: u64) -> u64 {
    seconds.clamp(MIN_INTERVAL_SECONDS, MAX_INTERVAL_SECONDS)
}

pub fn interval_in_range(seconds: u64) -> bool {
    (MIN_INTERVAL_SECONDS..=MAX_INTERVAL_SECONDS).contains(&seconds)
}

/// The two hive listener ports derived from the plaintext API port:
/// `(sync_port, pairing_port)` = `(port + 1, port + 2)`. Checked rather than
/// bare `+` so a `port` near `u16::MAX` is a clean config error instead of a
/// debug-build panic / release-build wraparound onto some unrelated low port.
pub fn hive_ports(api_port: u16) -> anyhow::Result<(u16, u16)> {
    let sync = api_port
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("hive sync port (server port + 1) overflows u16"))?;
    let pairing = api_port
        .checked_add(2)
        .ok_or_else(|| anyhow::anyhow!("hive pairing port (server port + 2) overflows u16"))?;
    Ok((sync, pairing))
}

/// Formats an mDNS-discovered peer address as a `host:port` authority usable
/// in a URL, bracketing IPv6 literals (`[fe80::1]:3457`) -- a bare
/// `fe80::1:3457` is not a valid URL authority and every request to such a
/// peer would fail to even parse. Callers should prefer an IPv4 address when
/// the peer advertises one.
pub fn format_peer_authority(ip: &str, port: u16) -> String {
    if ip.contains(':') {
        format!("[{ip}]:{port}")
    } else {
        format!("{ip}:{port}")
    }
}

/// Loads this device's persisted Ed25519 identity, or generates and
/// persists a fresh one on first run. The device_id is stored in the
/// store's `_meta` table (same pattern as `tag_namespaces`); the private
/// key lives in the OS keyring, addressed by that same device_id. Both must
/// be present and consistent for the persisted identity to be reused —
/// either one missing or unreadable falls through to generating (and
/// persisting) a brand new identity.
pub async fn bootstrap_self_identity(
    store: &crate::store::SqliteStore,
    key_store: &dyn keyring_store::HiveKeyStore,
) -> anyhow::Result<identity::DeviceIdentity> {
    if let Some(device_id) = store.get_meta("hive_device_id").await? {
        if let Some(signing_key_hex) = key_store.load(&device_id)?
            && let Some(existing) = identity::from_signing_key_hex(&signing_key_hex)
        {
            return Ok(existing);
        }
        tracing::warn!(
            "hive_device_id {device_id} present but its keyring entry is missing \
             or invalid; generating a new device identity"
        );
    }
    let fresh = identity::generate();
    key_store.save(&fresh.device_id, &hex::encode(fresh.signing_key.to_bytes()))?;
    store.set_meta("hive_device_id", &fresh.device_id).await?;
    Ok(fresh)
}

#[cfg(test)]
mod helper_tests {
    use super::*;

    #[test]
    fn clamp_interval_bounds_both_ends() {
        assert_eq!(clamp_interval(0), MIN_INTERVAL_SECONDS);
        assert_eq!(clamp_interval(u64::MAX), MAX_INTERVAL_SECONDS);
        assert_eq!(clamp_interval(300), 300);
        assert!(interval_in_range(300));
        assert!(!interval_in_range(0));
        assert!(!interval_in_range(MAX_INTERVAL_SECONDS + 1));
    }

    #[test]
    fn hive_ports_derive_from_api_port_and_reject_overflow() {
        assert_eq!(hive_ports(3456).unwrap(), (3457, 3458));
        assert!(hive_ports(u16::MAX).is_err());
        assert!(hive_ports(u16::MAX - 1).is_err());
        assert_eq!(hive_ports(u16::MAX - 2).unwrap(), (u16::MAX - 1, u16::MAX));
    }

    #[test]
    fn format_peer_authority_brackets_ipv6_only() {
        assert_eq!(format_peer_authority("10.0.0.5", 3457), "10.0.0.5:3457");
        assert_eq!(format_peer_authority("fe80::1", 3457), "[fe80::1]:3457");
    }
}

#[cfg(test)]
mod bootstrap_tests {
    use super::*;
    use keyring_store::{FakeHiveKeyStore, HiveKeyStore};

    #[tokio::test]
    async fn bootstrap_generates_and_persists_on_first_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let sync = crate::config::SyncSettings::default();
        let database = crate::db::open_database(&sync, path.to_str().unwrap())
            .await
            .unwrap();
        let conn = database.connect().unwrap();
        crate::db::run_migrations(&conn).await.unwrap();
        let store = crate::store::SqliteStore::new(conn);
        let key_store = FakeHiveKeyStore::new();

        let identity = bootstrap_self_identity(&store, &key_store).await.unwrap();
        assert!(identity.device_id.starts_with("hive_"));
        assert_eq!(
            store.get_meta("hive_device_id").await.unwrap(),
            Some(identity.device_id.clone())
        );
        assert!(key_store.load(&identity.device_id).unwrap().is_some());
    }

    #[tokio::test]
    async fn bootstrap_reuses_persisted_identity_on_second_call() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let sync = crate::config::SyncSettings::default();
        let database = crate::db::open_database(&sync, path.to_str().unwrap())
            .await
            .unwrap();
        let conn = database.connect().unwrap();
        crate::db::run_migrations(&conn).await.unwrap();
        let store = crate::store::SqliteStore::new(conn);
        let key_store = FakeHiveKeyStore::new();

        let first = bootstrap_self_identity(&store, &key_store).await.unwrap();
        let second = bootstrap_self_identity(&store, &key_store).await.unwrap();
        assert_eq!(first.device_id, second.device_id);
        assert_eq!(
            identity::public_key_hex(&first),
            identity::public_key_hex(&second)
        );
    }
}
