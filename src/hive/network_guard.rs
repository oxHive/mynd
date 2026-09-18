use crate::hive::identity::DeviceIdentity;
use crate::hive::network::TrustedNetwork;
use crate::store::SqliteStore;
use std::sync::Arc;

/// What the guard loop should do this tick, given whether the hive stack is
/// currently running, the trusted-network allowlist, and the current
/// network's identity key (`None` when unidentifiable, e.g. `whichnet`
/// returned `Unknown` or this platform isn't supported).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum GuardAction {
    None,
    Pause,
    Resume,
}

/// Pure decision function for the pause/resume state machine -- kept free of
/// any spawning/aborting side effects so it can be tested without real
/// listeners or mDNS. An empty trusted list means the feature is off: the
/// stack is left exactly as it was started (today's always-on behavior).
pub(crate) fn decide_action(
    stack_running: bool,
    trusted: &[TrustedNetwork],
    current: Option<&str>,
) -> GuardAction {
    if trusted.is_empty() {
        return GuardAction::None;
    }
    let is_trusted = current
        .map(|c| trusted.iter().any(|t| t.id == c))
        .unwrap_or(false);
    match (stack_running, is_trusted) {
        (true, false) => GuardAction::Pause,
        (false, true) => GuardAction::Resume,
        _ => GuardAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trusted(id: &str) -> TrustedNetwork {
        TrustedNetwork {
            id: id.to_string(),
            label: None,
            added_at: 0,
        }
    }

    #[test]
    fn empty_allowlist_never_acts() {
        assert_eq!(
            decide_action(true, &[], Some("ssid:cafe")),
            GuardAction::None
        );
        assert_eq!(
            decide_action(false, &[], Some("ssid:home")),
            GuardAction::None
        );
        assert_eq!(decide_action(false, &[], None), GuardAction::None);
    }

    #[test]
    fn running_on_untrusted_network_pauses() {
        let list = [trusted("ssid:home")];
        assert_eq!(
            decide_action(true, &list, Some("ssid:cafe")),
            GuardAction::Pause
        );
    }

    #[test]
    fn running_with_unidentifiable_network_pauses() {
        let list = [trusted("ssid:home")];
        assert_eq!(decide_action(true, &list, None), GuardAction::Pause);
    }

    #[test]
    fn paused_on_trusted_network_resumes() {
        let list = [trusted("ssid:home")];
        assert_eq!(
            decide_action(false, &list, Some("ssid:home")),
            GuardAction::Resume
        );
    }

    #[test]
    fn paused_on_untrusted_network_stays_paused() {
        let list = [trusted("ssid:home")];
        assert_eq!(
            decide_action(false, &list, Some("ssid:cafe")),
            GuardAction::None
        );
    }

    #[test]
    fn running_on_trusted_network_keeps_running() {
        let list = [trusted("ssid:home")];
        assert_eq!(
            decide_action(true, &list, Some("ssid:home")),
            GuardAction::None
        );
    }
}

/// Everything the untrusted-network guard needs to be able to stop: the
/// mandatory-mTLS sync listener, the mDNS advertise/browse task (plus the
/// `HiveDiscovery` itself, to shut its daemon down), and the sync/ping
/// loops. Built by `spawn_hive_stack`, torn down by `pause`.
pub struct HiveStackHandle {
    sync_listener: tokio::task::JoinHandle<()>,
    discovery: crate::hive::discovery::HiveDiscovery,
    discovery_browse: tokio::task::JoinHandle<()>,
    sync_loop: tokio::task::JoinHandle<()>,
    ping_loop: tokio::task::JoinHandle<()>,
}

impl HiveStackHandle {
    /// Aborts every spawned task and shuts down the mDNS daemon, fully
    /// stopping this device from advertising or serving hive sync traffic.
    pub fn pause(self) {
        self.sync_listener.abort();
        self.discovery_browse.abort();
        self.sync_loop.abort();
        self.ping_loop.abort();
        if let Err(e) = self.discovery.shutdown() {
            tracing::warn!("hive mDNS shutdown failed: {e:#}");
        }
    }
}

/// Bind address/port and sync/ping cadence for a hive stack -- grouped into
/// one struct (rather than four separate parameters) purely to keep
/// `spawn_hive_stack` and `run_guard_loop` under clippy's argument-count
/// lint; there's no independent meaning to the grouping beyond that.
#[derive(Clone)]
pub struct HiveStackParams {
    pub host: String,
    pub port: u16,
    pub sync_interval_seconds: u64,
    pub ping_interval_seconds: u64,
}

/// Starts the mandatory-mTLS sync listener, mDNS advertise/browse, and the
/// sync/ping loops -- the same startup sequence `http::run_up` used to run
/// inline, extracted here so both the initial boot and every guard-loop
/// resume share one code path. Also re-runs the idempotent self-join roster
/// upsert (safe to repeat: `merge_roster` never regresses an Active entry
/// or un-revokes a sticky revocation).
pub async fn spawn_hive_stack(
    store: Arc<SqliteStore>,
    identity: DeviceIdentity,
    params: HiveStackParams,
    events: tokio::sync::broadcast::Sender<serde_json::Value>,
) -> anyhow::Result<HiveStackHandle> {
    let HiveStackParams {
        host,
        port,
        sync_interval_seconds,
        ping_interval_seconds,
    } = params;
    let self_join = crate::hive::roster::create_join_record(
        &identity,
        &identity.device_id,
        chrono::Utc::now().timestamp(),
    );
    let self_entry = crate::hive::roster::RosterEntry {
        device_id: self_join.device_id.clone(),
        public_key: self_join.public_key.clone(),
        name: self_join.name.clone(),
        status: crate::hive::roster::RosterStatus::Active,
        joined_at: self_join.joined_at,
        revoked_at: None,
        revoked_by: None,
        join_record: self_join,
        revocation_record: None,
    };
    store.hive_merge_roster(vec![self_entry]).await?;

    let (sync_port, _) = crate::hive::hive_ports(port)?;
    let certified = crate::hive::cert::self_signed_cert(&identity)?;
    let build_sync_server_config = {
        let certified_cert_der = certified.cert.der().clone();
        let certified_key_der = certified.signing_key.serialize_der();
        move |roster: Vec<crate::hive::roster::RosterEntry>| -> anyhow::Result<rustls::ServerConfig> {
            let client_verifier = std::sync::Arc::new(
                crate::hive::tls_verify::RosterClientCertVerifier::new(roster),
            );
            Ok(rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_protocol_versions(rustls::DEFAULT_VERSIONS)
            .map_err(|e| anyhow::anyhow!("failed to select TLS protocol versions: {e}"))?
            .with_client_cert_verifier(client_verifier)
            .with_single_cert(
                vec![certified_cert_der.clone()],
                rustls::pki_types::PrivateKeyDer::try_from(certified_key_der.clone())
                    .map_err(|e| anyhow::anyhow!("invalid private key DER: {e}"))?,
            )?)
        }
    };
    let initial_roster = store.hive_list_roster().await?;
    let sync_server_config = build_sync_server_config(initial_roster)?;
    let sync_tls_config =
        axum_server::tls_rustls::RustlsConfig::from_config(std::sync::Arc::new(sync_server_config));
    let sync_tls_config_for_reload = sync_tls_config.clone();
    let sync_only_app = crate::api::hive_sync_router(store.clone());
    let sync_addr: std::net::SocketAddr = format!("{host}:{sync_port}").parse()?;
    let sync_listener = tokio::spawn(async move {
        if let Err(e) = axum_server::bind_rustls(sync_addr, sync_tls_config)
            .serve(sync_only_app.into_make_service())
            .await
        {
            tracing::error!("hive sync TLS listener failed to bind/serve: {e:#}");
        }
    });
    tracing::info!("Hive sync (mTLS): https://{host}:{sync_port}");

    let discovery = crate::hive::discovery::HiveDiscovery::new()?;
    discovery.advertise(&identity.device_id, &identity.device_id, sync_port)?;
    tracing::info!("Hive mDNS: advertising as {}", identity.device_id);
    let discovery_for_browse = discovery.clone();
    let discovery_browse = tokio::spawn(async move {
        match discovery_for_browse.browse() {
            Ok(receiver) => {
                while let Ok(event) = receiver.recv_async().await {
                    if let mdns_sd::ServiceEvent::ServiceResolved(info) = event {
                        tracing::info!("hive peer discovered: {}", info.get_fullname());
                        let fullname = info.get_fullname();
                        if let Some(device_id) = fullname.strip_suffix("._hivemind._tcp.local.") {
                            // Prefer an IPv4 address; fall back to the first
                            // IPv6 one, bracketed so it forms a valid URL
                            // authority (a bare `fe80::1:3457` never parses).
                            let ip = info
                                .get_addresses_v4()
                                .into_iter()
                                .next()
                                .map(|v4| v4.to_string())
                                .or_else(|| {
                                    info.get_addresses().iter().next().map(|a| a.to_string())
                                });
                            if let Some(ip) = ip {
                                let hive_addr =
                                    crate::hive::format_peer_authority(&ip, info.get_port());
                                crate::hive::peer_status::record_discovered_address(
                                    device_id, hive_addr,
                                );
                            }
                        }
                    }
                }
            }
            Err(e) => tracing::error!("hive mDNS browse failed: {e:#}"),
        }
    });

    let sync_loop = tokio::spawn(crate::hive::sync_loop::run_sync_loop(
        store.clone(),
        identity.clone(),
        sync_interval_seconds,
    ));
    let ping_loop = tokio::spawn(crate::hive::peer_status::run_ping_loop(
        store.clone(),
        identity.clone(),
        ping_interval_seconds,
        sync_tls_config_for_reload,
        build_sync_server_config,
        events,
    ));

    Ok(HiveStackHandle {
        sync_listener,
        discovery,
        discovery_browse,
        sync_loop,
        ping_loop,
    })
}

/// Polls every 15 seconds, comparing the current network against the
/// trusted-networks list, and pauses/resumes the hive stack via
/// `decide_action`. A no-op every tick while the trusted list is empty (the
/// feature-off steady state), so it's always safe to spawn this whenever
/// hive is enabled -- not just once a trusted network has been configured.
pub async fn run_guard_loop(
    store: Arc<SqliteStore>,
    identity: DeviceIdentity,
    params: HiveStackParams,
    events: tokio::sync::broadcast::Sender<serde_json::Value>,
    stack: Arc<tokio::sync::Mutex<Option<HiveStackHandle>>>,
) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        let trusted = match store.hive_trusted_networks().await {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!("hive_trusted_networks read failed: {e:#}");
                continue;
            }
        };
        let current = crate::hive::network::current_network_key_async().await;
        let mut guard = stack.lock().await;
        let action = decide_action(guard.is_some(), &trusted, current.as_deref());
        match action {
            GuardAction::Pause => {
                tracing::warn!("network changed to an untrusted network; pausing Hive Mode");
                if let Some(handle) = guard.take() {
                    handle.pause();
                }
            }
            GuardAction::Resume => {
                tracing::info!("trusted network detected; resuming Hive Mode");
                match spawn_hive_stack(
                    store.clone(),
                    identity.clone(),
                    params.clone(),
                    events.clone(),
                )
                .await
                {
                    Ok(handle) => *guard = Some(handle),
                    Err(e) => tracing::error!("failed to resume hive stack: {e:#}"),
                }
            }
            GuardAction::None => {}
        }
    }
}
