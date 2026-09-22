use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

const SERVICE_TYPE: &str = "_hivemind._tcp.local.";

pub fn service_name(device_id: &str) -> String {
    format!("{device_id}.{SERVICE_TYPE}")
}

#[derive(Clone)]
pub struct HiveDiscovery {
    daemon: ServiceDaemon,
}

impl HiveDiscovery {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            daemon: ServiceDaemon::new()?,
        })
    }

    pub fn advertise(&self, device_id: &str, name: &str, port: u16) -> anyhow::Result<()> {
        let host_ip = local_ip_address::local_ip()?.to_string();
        let info = ServiceInfo::new(
            SERVICE_TYPE,
            device_id,
            &format!("{device_id}.local."),
            host_ip,
            port,
            &[("name", name)][..],
        )?;
        self.daemon.register(info)?;
        Ok(())
    }

    pub fn browse(&self) -> anyhow::Result<mdns_sd::Receiver<ServiceEvent>> {
        Ok(self.daemon.browse(SERVICE_TYPE)?)
    }

    /// Stops the mDNS daemon entirely -- both advertising this device's
    /// presence and any in-flight browse. Used by the network guard to fully
    /// stop leaking this device's `device_id` when off a trusted network;
    /// resuming re-creates a fresh `HiveDiscovery` rather than restarting
    /// this one, since `ServiceDaemon` has no documented re-open-after-
    /// shutdown guarantee.
    pub fn shutdown(&self) -> anyhow::Result<()> {
        self.daemon.shutdown()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_name_includes_device_id_and_service_type() {
        let name = service_name("hive_abc123");
        assert_eq!(name, "hive_abc123._hivemind._tcp.local.");
    }

    #[test]
    fn new_creates_and_shuts_down_a_daemon() {
        let discovery = HiveDiscovery::new().expect("daemon creation should succeed sandboxed");
        assert!(discovery.shutdown().is_ok());
    }

    #[test]
    fn browse_returns_a_receiver_before_shutdown() {
        let discovery = HiveDiscovery::new().unwrap();
        assert!(discovery.browse().is_ok());
        let _ = discovery.shutdown();
    }

    #[test]
    fn advertise_registers_without_error() {
        let discovery = HiveDiscovery::new().unwrap();
        assert!(
            discovery
                .advertise("hive_advtest", "advtest", 34567)
                .is_ok()
        );
        let _ = discovery.shutdown();
    }
}
