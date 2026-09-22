use crate::hive::identity::DeviceIdentity;
use anyhow::Result;
use axum::Router;
use axum_server::Handle;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The concrete `Handle` type our pairing listener uses. `axum_server::Handle`
/// is generic over the bound `Address`; our listener always binds a plain
/// `SocketAddr`, so we pin the type parameter here so it can live in a struct
/// field and be shared across the spawn/timeout tasks.
type PairingHandle = Handle<SocketAddr>;

/// Coordinates the pairing-only TLS listener's lifecycle: bound only while a
/// pairing code is outstanding, torn down automatically afterward. This bounds
/// the exposure window of an unauthenticated (server-TLS-only) network port to
/// only when the local user actually requested a pairing code, rather than
/// leaving it open for the entire life of the process.
pub struct PairingWindow {
    host: String,
    port: u16,
    identity: DeviceIdentity,
    router: Router,
    /// `Some(handle)` exactly while a window is open. Shared behind an `Arc`
    /// so the auto-shutdown timeout task can clear it back to `None` when the
    /// window closes, allowing a *later* pairing-code request to open a fresh
    /// window rather than being permanently locked out after the first one.
    active_handle: Arc<Mutex<Option<PairingHandle>>>,
}

impl PairingWindow {
    pub fn new(host: String, port: u16, identity: DeviceIdentity, router: Router) -> Self {
        Self {
            host,
            port,
            identity,
            router,
            active_handle: Arc::new(Mutex::new(None)),
        }
    }

    /// Opens the pairing listener for `duration` if not already open
    /// (re-issuing a code while a window is already active just lets the
    /// existing window keep running rather than stacking listeners).
    /// Automatically shuts the listener down when `duration` elapses, and
    /// clears `active_handle` back to `None` so a subsequent call can open a
    /// new window later in the process's life.
    pub fn open_for(&self, duration: Duration) {
        let mut guard = self.active_handle.lock().unwrap();
        if guard.is_some() {
            return; // a window is already open; let it run its own course
        }
        let Ok(tls_config) = self.build_tls_config() else {
            tracing::error!("could not build pairing-port TLS config; pairing window not opened");
            return;
        };
        let handle: PairingHandle = Handle::new();
        let addr: SocketAddr = match format!("{}:{}", self.host, self.port).parse() {
            Ok(a) => a,
            Err(e) => {
                tracing::error!("invalid pairing port address: {e:#}");
                return;
            }
        };
        let router = self.router.clone();
        let handle_for_server = handle.clone();
        tokio::spawn(async move {
            if let Err(e) = axum_server::bind_rustls(addr, tls_config)
                .handle(handle_for_server)
                .serve(router.into_make_service())
                .await
            {
                tracing::error!("pairing TLS listener failed to bind/serve: {e:#}");
            }
        });
        tracing::info!(
            "Hive pairing window open for {duration:?} on port {}",
            self.port
        );
        *guard = Some(handle.clone());
        drop(guard);

        // Auto-shutdown after `duration`. The `active_handle` Arc is cloned in
        // here so the timeout task can both shut the listener down AND clear
        // the stored handle back to `None` -- without that reset, the very
        // first window would leave `active_handle` permanently `Some(_)` and
        // every later `open_for` call would short-circuit at the guard check
        // above, so a second pairing code issued later in the process's life
        // could never open a window again.
        let handle_for_timeout = handle;
        let active_handle = self.active_handle.clone();
        let host = self.host.clone();
        let port = self.port;
        tokio::spawn(async move {
            tokio::time::sleep(duration).await;
            handle_for_timeout.shutdown();
            *active_handle.lock().unwrap() = None;
            tracing::info!("Hive pairing window closed on {}:{}", host, port);
        });
    }

    fn build_tls_config(&self) -> Result<axum_server::tls_rustls::RustlsConfig> {
        let certified = crate::hive::cert::self_signed_cert(&self.identity)?;
        let server_config = rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(rustls::DEFAULT_VERSIONS)
        .map_err(|e| anyhow::anyhow!("failed to select TLS protocol versions: {e}"))?
        .with_no_client_auth()
        .with_single_cert(
            vec![certified.cert.der().clone()],
            rustls::pki_types::PrivateKeyDer::try_from(certified.signing_key.serialize_der())
                .map_err(|e| anyhow::anyhow!("invalid private key DER: {e}"))?,
        )?;
        Ok(axum_server::tls_rustls::RustlsConfig::from_config(
            std::sync::Arc::new(server_config),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hive::identity;

    /// Proves the auto-shutdown timeout resets `active_handle` back to `None`
    /// so a SECOND pairing window, opened later in the process's life,
    /// actually binds -- rather than being permanently locked out after the
    /// first window closes (the bug this fix closes). Port `0` lets the OS
    /// pick a fresh ephemeral port for each window, so two real listeners can
    /// bind back-to-back without a port clash.
    #[tokio::test]
    async fn second_pairing_window_opens_after_first_closes() {
        let identity = identity::generate();
        let pw = PairingWindow::new(
            "127.0.0.1".to_string(),
            0,
            identity,
            Router::new().route("/api/v1/hive/pair", axum::routing::post(|| async { "ok" })),
        );

        // First window opens.
        pw.open_for(Duration::from_millis(150));
        assert!(
            pw.active_handle.lock().unwrap().is_some(),
            "first window should be open immediately after open_for"
        );

        // Wait past the window duration; the timeout task must shut it down
        // AND clear the stored handle back to None.
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(
            pw.active_handle.lock().unwrap().is_none(),
            "active_handle must reset to None after the first window closes -- \
             this is the exact regression the reset fixes"
        );

        // A second window, later in the process's life, opens a fresh listener.
        pw.open_for(Duration::from_millis(150));
        assert!(
            pw.active_handle.lock().unwrap().is_some(),
            "a SECOND pairing window must open after the first one closed"
        );

        // Let the second window's timeout fire so the test leaves nothing
        // running, and re-confirm the reset works a second time too.
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(
            pw.active_handle.lock().unwrap().is_none(),
            "active_handle must reset to None after the second window closes as well"
        );
    }

    /// A re-issued code while a window is already open does not stack a second
    /// listener: the second call short-circuits and the same handle stays in
    /// place for the remainder of the original window.
    #[tokio::test]
    async fn reissuing_while_open_does_not_stack_a_second_listener() {
        let identity = identity::generate();
        let pw = PairingWindow::new(
            "127.0.0.1".to_string(),
            0,
            identity,
            Router::new().route("/api/v1/hive/pair", axum::routing::post(|| async { "ok" })),
        );

        pw.open_for(Duration::from_secs(30));
        assert!(pw.active_handle.lock().unwrap().is_some());

        // Re-issue while still open -- must be a no-op: the guard short-circuits
        // and the existing window keeps running rather than a second listener
        // being spawned (which would race to bind the same address).
        pw.open_for(Duration::from_secs(30));
        assert!(
            pw.active_handle.lock().unwrap().is_some(),
            "re-issue while open should retain the existing active window"
        );
    }
}
