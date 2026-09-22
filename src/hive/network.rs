use serde::{Deserialize, Serialize};

/// A per-device trusted-network entry. Local state only — never pushed to
/// peers (unlike `hive_settings_override`), since each device's trusted
/// networks are its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrustedNetwork {
    pub id: String,
    pub label: Option<String>,
    pub added_at: i64,
}

/// Maps a `whichnet` reading to the canonical string key stored in the
/// trusted-networks list and compared against on every guard-loop tick.
/// `Unknown` has no key -- a device whose network can't be identified is
/// never considered trusted, so it stays paused whenever the allowlist is
/// non-empty rather than silently matching everything.
pub fn identity_key(identity: &whichnet::NetworkIdentity) -> Option<String> {
    match identity {
        whichnet::NetworkIdentity::Ssid(s) => Some(format!("ssid:{s}")),
        whichnet::NetworkIdentity::GatewayMac { mac, .. } => {
            Some(format!("mac:{}", hex::encode(mac)))
        }
        whichnet::NetworkIdentity::Unknown => None,
    }
}

pub fn current_network_key() -> Option<String> {
    identity_key(&whichnet::current_identity())
}

/// Async wrapper around `current_network_key` for callers running on the
/// tokio runtime. `whichnet::current_identity()` does blocking OS work (a
/// netlink SSID query and an ARP/gateway-MAC lookup on Linux, CoreWLAN on
/// macOS); calling it directly from an async task stalls a runtime worker
/// thread for the duration, so offload it to the blocking pool instead.
pub async fn current_network_key_async() -> Option<String> {
    tokio::task::spawn_blocking(current_network_key)
        .await
        .unwrap_or(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssid_maps_to_ssid_prefixed_key() {
        let id = whichnet::NetworkIdentity::Ssid("home-wifi".to_string());
        assert_eq!(identity_key(&id), Some("ssid:home-wifi".to_string()));
    }

    #[test]
    fn gateway_mac_maps_to_mac_prefixed_hex_key() {
        let id = whichnet::NetworkIdentity::GatewayMac {
            mac: [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
            gateway_ip: None,
        };
        assert_eq!(identity_key(&id), Some("mac:aabbccddeeff".to_string()));
    }

    #[test]
    fn unknown_maps_to_none() {
        assert_eq!(identity_key(&whichnet::NetworkIdentity::Unknown), None);
    }
}
